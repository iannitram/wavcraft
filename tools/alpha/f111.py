#!/usr/bin/env python3
"""f111 scripted run: Mixdown through the control channel (port 47211).

    python3 tools/alpha/f111.py --exe <path to the release wavcraft> [--keep]

Two short synthetic clips go on two tracks of a session; MIXDOWN renders the whole session
into a new waveform document (checked through STATE: frames, channels, rate), MIXDOWN selected
renders only the picked clip, MIXDOWN normalize=-1 is saved with SAVE and its peak read back from
the WAV (must be -1 dB). Fails on any ERROR line in the app's own log.
"""
import argparse, json, math, os, shutil, struct, subprocess, sys, tempfile, time, wave, socket

PORT, RATE = 47211, 48000


def make_wav(path, rate, channels, secs, hz):
    with wave.open(path, "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(rate)
        n = int(rate * secs)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * hz * i / rate))) * channels for i in range(n)))


class Control:
    def __init__(self, port, token):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("the control channel refused the token")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self):
        a = self.ask("SESSION state")
        head, _, js = a.partition(" {")
        return json.loads("{" + js) if js else {}


import socket




def state(c):
    kv = dict(p.split("=", 1) for p in c.ask("STATE").split()[1:])
    return kv


def wait_idle(c, secs=60):
    for _ in range(int(secs * 10)):
        kv = state(c)
        if kv.get("busy") == "0":
            return kv
        time.sleep(0.1)
    return state(c)


def wav_peak(path):
    """Peak of a 32-bit float WAV (the mixdown is saved as float): walk the chunks to 'data'."""
    raw = open(path, "rb").read()
    i = 12
    while i + 8 <= len(raw):
        cid, size = raw[i:i + 4], struct.unpack("<I", raw[i + 4:i + 8])[0]
        if cid == b"data":
            data = raw[i + 8:i + 8 + size]
            xs = struct.unpack("<%df" % (len(data) // 4), data[:len(data) // 4 * 4])
            return max(abs(x) for x in xs)
        i += 8 + size + (size & 1)
    return -1.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f111.", dir=root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    a, b = (os.path.join(work, n) for n in ("a.wav", "b.wav"))
    make_wav(a, RATE, 2, 1.0, 220)
    make_wav(b, RATE, 1, 1.0, 330)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([args.exe], env=env, stdout=err, stderr=err)
    checks = []

    def check(label, ok, detail=""):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    try:
        c = Control(PORT, open(token).read().strip() if os.path.exists(token) else "")
    except Exception:
        time.sleep(1)
        c = Control(PORT, open(token).read().strip())
    try:
        check("MIXDOWN with no session is refused", c.ask("MIXDOWN").startswith("ERR no session"))
        check("SESSION new", c.ask("SESSION new Demo rate=48000 depth=24") == "OK")
        c.ask("TRACK add Drums stereo")
        c.ask("TRACK add Lead mono")
        check("CLIP add Drums at 0", c.ask(f'CLIP add Drums "{a}" 0') == "OK clips=1")
        check("CLIP add Lead at 0.5", c.ask(f'CLIP add Lead "{b}" 0.5') == "OK clips=2")
        check("MIXDOWN selected with nothing picked is refused", c.ask("MIXDOWN selected").startswith("ERR no clips are selected"))
        check("MIXDOWN", c.ask("MIXDOWN") == "OK")
        kv = wait_idle(c)
        check("the new document is the whole session: 1.5 s, 2 channels, 48 kHz", (kv.get("frames"), kv.get("channels"), kv.get("rate")) == ("72000", "2", "48000"), kv)
        check("the new document is unsaved", kv.get("unsaved") == "1", kv)
        check("CLIP select Drums 1", c.ask("CLIP select Drums 1") == "OK selected=1")
        check("MIXDOWN selected", c.ask("MIXDOWN selected") == "OK")
        kv = wait_idle(c)
        check("only the picked clip: 1.0 s", kv.get("frames") == "48000", kv)
        check("MIXDOWN normalize=-1", c.ask("MIXDOWN normalize=-1") == "OK")
        kv = wait_idle(c)
        check("the normalised mixdown is 1.5 s", kv.get("frames") == "72000", kv)
        out = os.path.join(work, "mix.wav")
        check("SAVE the mixdown", c.ask(f'SAVE {out}') == "OK")
        wait_idle(c)
        pk = wav_peak(out) if os.path.exists(out) else -1.0
        want = 10 ** (-1 / 20)
        check("its peak is -1 dB", abs(pk - want) < 0.002, f"peak {pk:.4f}, wanted {want:.4f}")
        time.sleep(0.5)
        check("the app is still up", proc.poll() is None)
        check("QUIT", c.ask("QUIT discard") == "OK")
        for _ in range(100):
            if proc.poll() is not None:
                break
            time.sleep(0.1)
        check("the app exited", proc.poll() is not None, proc.returncode)
    finally:
        if proc.poll() is None:
            proc.kill()
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    ok = all(x["ok"] for x in checks)
    for x in checks:
        print(("PASS " if x["ok"] else "FAIL ") + x["check"] + ("" if x["ok"] else f"  [{x['detail']}]"))
    print("RESULT", "pass" if ok else "fail", f"({sum(x['ok'] for x in checks)}/{len(checks)})")
    if args.keep or not ok:
        print("work folder kept:", work)
    else:
        shutil.rmtree(work, ignore_errors=True)
    return 0 if ok else 1


sys.exit(main())
