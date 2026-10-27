#!/usr/bin/env python3
"""f73 scripted run: Batch Process through the control channel.

    python3 tools/alpha/f73.py --exe <path to the release wavcraft>

Three small synthetic WAVs (stereo 16-bit, mono 24-bit, stereo 16-bit) and one unreadable file
are added; a normalize-to--1 dBFS step, an output folder and a name pattern are set; BATCH run;
polled with BATCH status. Checked: 3 ok, 1 failed with a reason, output peaks at -1 dBFS and
the right length, inputs byte-identical, a second run without overwrite fails every file,
nothing but the 3 outputs in the folder, QUIT, and no ERROR line in the app's log.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE = 48000


def write_wav(path, channels, width, frames, amp, hz):
    w = wave.open(path, "wb")
    w.setnchannels(channels); w.setsampwidth(width); w.setframerate(RATE)
    rows = bytearray()
    for i in range(frames):
        for c in range(channels):
            x = amp * (1 - 0.3 * c) * math.sin(2 * math.pi * hz * (c + 1) * i / RATE)
            if width == 2:
                rows += struct.pack("<h", round(x * 32767))
            else:
                rows += struct.pack("<i", round(x * 8388607))[:3]
    w.writeframes(bytes(rows)); w.close()


def read_peak(path):
    w = wave.open(path, "rb")
    n, ch, width = w.getnframes(), w.getnchannels(), w.getsampwidth()
    raw = w.readframes(n)
    peak = 0.0
    if width == 2:
        vals = struct.unpack("<%dh" % (len(raw) // 2), raw); peak = max(abs(v) for v in vals) / 32768
    elif width == 3:
        for i in range(0, len(raw), 3):
            v = int.from_bytes(raw[i:i + 3], "little", signed=True); peak = max(peak, abs(v) / 8388608)
    return n, ch, width, peak


class Control:
    def __init__(self, port, token):
        for _ in range(80):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60); break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        assert self.ask(f"TOKEN {token}") == "OK"

    def ask(self, line):
        self.f.write(line + "\n"); self.f.flush()
        return self.f.readline().strip()


def status(c):
    a = c.ask("BATCH status")
    head, _, res = a.partition(" results=")
    fields = dict(kv.split("=", 1) for kv in head.split()[1:] if "=" in kv)
    return fields, [r for r in res.split(" | ") if r]


def wait_done(c, total, limit):
    t0 = time.time()
    while time.time() - t0 < limit:
        f, res = status(c)
        if f.get("running") == "0" and int(f.get("done", 0)) == total:
            break
        time.sleep(0.3)
    return f, res


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--exe", required=True); ap.add_argument("--port", type=int, default=47173)
    a = ap.parse_args()
    home = tempfile.mkdtemp(prefix="f73.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
    fails = []
    def check(ok, msg):
        print(("PASS " if ok else "FAIL ") + msg)
        if not ok: fails.append(msg)
    proc = None
    try:
        logdir, sess, inp, out = (os.path.join(home, d) for d in ("log", "sessions", "in", "out"))
        for d in (logdir, sess, inp): os.makedirs(d)
        files = [os.path.join(inp, n) for n in ("a.wav", "b.wav", "c.wav")]
        write_wav(files[0], 2, 2, 100000, 0.25, 440)
        write_wav(files[1], 1, 3, 30000, 0.5, 220)
        bad = os.path.join(inp, "broken.wav"); open(bad, "wb").write(b"not audio")
        write_wav(files[2], 2, 2, 5000, 0.1, 880)
        order = [files[0], bad, files[1], files[2]]
        before = {p: open(p, "rb").read() for p in order}
        tokfile = os.path.join(home, "control.token")
        env = dict(os.environ, HOME=home, WAVCRAFT_LOG_DIR=logdir, WAVCRAFT_SESSION_DIR=sess,
                   WAVCRAFT_CONTROL_PORT=str(a.port), WAVCRAFT_CONTROL_TOKEN_FILE=tokfile)
        proc = subprocess.Popen([a.exe], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(80):
            if os.path.exists(tokfile): break
            time.sleep(0.25)
        c = Control(a.port, open(tokfile).read().strip())
        for p in order:
            check(c.ask(f"BATCH add {p}").startswith("OK"), f"add {os.path.basename(p)}")
        check(c.ask("BATCH op normalize peak=-1").startswith("OK"), "op normalize")
        check(c.ask("BATCH op normalize peak=5").startswith("ERR"), "a peak above 0 dBFS is refused")
        check(c.ask(f"BATCH out {out} pattern={{name}}_norm").startswith("OK"), "out folder and pattern")
        check(c.ask("BATCH run").startswith("OK"), "run")
        f, res = wait_done(c, 4, 120)
        print("status:", f, res)
        check(f.get("ok") == "3" and f.get("failed") == "1", "3 ok, 1 failed")
        check(any(r.startswith("broken.wav:failed:") for r in res), "the unreadable file is reported with a reason")
        target = 10 ** (-1 / 20)
        for name, chans, width, frames in [("a_norm.wav", 2, 2, 100000), ("b_norm.wav", 1, 3, 30000), ("c_norm.wav", 2, 2, 5000)]:
            n, ch, w, pk = read_peak(os.path.join(out, name))
            check(n == frames and ch == chans and w == width and abs(pk - target) < 2e-4, f"{name}: {n} frames, {ch} ch, {w * 8}-bit, peak {pk:.5f} vs {target:.5f}")
        check(sorted(os.listdir(out)) == ["a_norm.wav", "b_norm.wav", "c_norm.wav"], "only the three outputs in the folder")
        check(all(open(p, "rb").read() == b for p, b in before.items()), "inputs byte-identical")
        outs = {n: open(os.path.join(out, n), "rb").read() for n in os.listdir(out)}
        c.ask("BATCH run")
        f, res = wait_done(c, 4, 60)
        check(f.get("ok") == "0" and f.get("failed") == "4", "second run without overwrite: 4 failed")
        check(all(open(os.path.join(out, n), "rb").read() == b for n, b in outs.items()), "existing outputs untouched")
        check(not any(os.scandir(sess)), "no session stores left behind")
        check(c.ask("QUIT").startswith("OK"), "QUIT")
        proc.wait(timeout=30)
        log = "".join(open(os.path.join(logdir, n), errors="replace").read() for n in os.listdir(logdir))
        # The deliberately unreadable input makes the third-party decoder log one ERROR while probing it
        # ("probe reach EOF"); that line is expected and the only one allowed.
        errs = [l for l in log.splitlines() if "ERROR" in l and "symphonia_core::probe: probe reach EOF" not in l]
        check(not errs, "no ERROR line in the app's log (bar the decoder's probe line for the unreadable file)" + (": " + errs[0] if errs else ""))
        print("log lines mentioning batch:", [l for l in log.splitlines() if "batch" in l.lower()][:6])
    finally:
        if proc and proc.poll() is None: proc.kill()
        shutil.rmtree(home, ignore_errors=True)
    print("RESULT", "FAIL" if fails else "PASS")
    sys.exit(1 if fails else 0)


main()
