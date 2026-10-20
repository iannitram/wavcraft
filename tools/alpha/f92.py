#!/usr/bin/env python3
"""f92 scripted run: the spectral display's three frequency scales and scrolling the band.

    python3 tools/alpha/f92.py --exe <path to the release wavcraft>

Starts the app on a synthetic stereo file with HOME, the session and log folders in a fresh
scratch folder. Drives `SPECTRAL scale lin|log_partial|log_full`, `SPECTRAL scroll <hz>` and
`SPECTRAL view`, checks `STATE` and the refusals of bad input, checks the settings file, restarts
to see the scale come back, and quits. Fails on any ERROR line in the app's log or a panic on
stderr.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, SECONDS, PORT = 48000, 3, 47192


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        frames = bytearray()
        for i in range(RATE * SECONDS):
            l = int(0.5 * 32767 * math.sin(2 * math.pi * 1000 * i / RATE))
            r = int(0.5 * 32767 * math.sin(2 * math.pi * 5000 * i / RATE))
            frames += struct.pack("<hh", l, r)
        w.writeframes(bytes(frames))


class Control:
    def __init__(self, port, token_file):
        for _ in range(120):
            if os.path.exists(token_file):
                try:
                    self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
                    break
                except OSError:
                    pass
            time.sleep(0.5)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {open(token_file).read().strip()}") != "OK":
            raise RuntimeError("the control channel refused the token")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self, wait=True):
        t0 = time.time()
        while True:
            head = self.ask("STATE").partition(" undo=")[0]
            st = dict(kv.split("=", 1) for kv in head.split()[1:])
            if not wait or (st.get("busy") == "0" and st.get("frames", "0") != "0") or time.time() - t0 > 60:
                return st
            time.sleep(0.2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f92.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    wav, token = os.path.join(work, "tones.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    def settings_text():
        for d, _, fs in os.walk(home):
            if "settings.txt" in fs:
                return open(os.path.join(d, "settings.txt")).read()
        return ""

    def run(body):
        err = open(os.path.join(work, "stderr.log"), "a")
        proc = subprocess.Popen([exe, wav], env=env, stdout=err, stderr=err)
        try:
            body(Control(PORT, token))
            proc.wait(20)
        except Exception as e:
            check("control channel", False, f"{type(e).__name__}: {e}")
            proc.kill()
        err.close()
        check("the app exits cleanly", proc.returncode == 0, f"exit {proc.returncode}")

    def run1(c):
        st = c.state()
        check("opens the file", st["frames"] == str(RATE * SECONDS), st)
        time.sleep(1.0)
        check("defaults: whole range, full log", st["spectral"] == "40-24000:log", st["spectral"])
        a = c.ask("SPECTRAL scale log_partial")
        check("partial log scale keeps the band", a == "OK spectral=40-24000:log_partial", a)
        a = c.ask("SPECTRAL reset")
        check("reset on the partial scale starts at 0 Hz", a == "OK spectral=0-24000:log_partial", a)
        time.sleep(0.8)
        a = c.ask("SPECTRAL view 200 3000 log_partial")
        check("view with the partial scale", a == "OK spectral=200-3000:log_partial", a)
        time.sleep(0.8)
        st = c.state()
        check("STATE reports the partial scale", st["spectral"] == "200-3000:log_partial", st["spectral"])
        a = c.ask("SPECTRAL scroll 1000")
        lo, hi = a.split("spectral=")[1].split(":")[0].split("-") if a.startswith("OK") else ("0", "0")
        check("scroll moves the low edge to 1000 Hz", a.startswith("OK") and abs(float(lo) - 1000) < 1, a)
        check("scroll keeps the band's height on the scale (the top rose)", float(hi) > 3000, a)
        time.sleep(0.8)
        a = c.ask("SPECTRAL scroll 9999999")
        check("scrolling past the top stops at Nyquist", a.startswith("OK") and a.split("-")[1].split(":")[0] == "24000", a)
        a = c.ask("SPECTRAL scroll 0")
        check("scrolling to 0 stops at the bottom", a.startswith("OK spectral=0-"), a)
        a = c.ask("SPECTRAL scale lin")
        check("linear scale", a.endswith(":lin"), a)
        a = c.ask("SPECTRAL scale log_full")
        check("full log scale (the low end rises off 0 Hz)", a.endswith(":log") and not a.startswith("OK spectral=0-"), a)
        a = c.ask("SPECTRAL view 100 5000 log_partial")
        check("view with the partial scale again", a == "OK spectral=100-5000:log_partial", a)
        for bad in ("SPECTRAL scale", "SPECTRAL scale sideways", "SPECTRAL scroll", "SPECTRAL scroll x", "SPECTRAL scroll nan", "SPECTRAL view 5 1 lin", "SPECTRAL view 1 2 sideways"):
            a = c.ask(bad)
            check(f"refuses {bad!r}", a.startswith("ERR SPECTRAL"), a)
        check("the band survived the refusals", c.state()["spectral"] == "100-5000:log_partial", c.state()["spectral"])
        time.sleep(0.8)
        text = settings_text()
        check("the settings file holds the scale", "spectral_view = 100 5000 log_partial" in text, text.replace("\n", " | "))
        check("QUIT", c.ask("QUIT discard") == "OK", "")

    def run2(c):
        st = c.state()
        check("after a restart the scale and band are back", st["spectral"] == "100-5000:log_partial", st["spectral"])
        a = c.ask("SPECTRAL reset")
        check("reset keeps the partial scale", a == "OK spectral=0-24000:log_partial", a)
        check("QUIT again", c.ask("QUIT discard") == "OK", "")

    run(run1)
    run(run2)
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f.endswith(".log")]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    stderr = open(os.path.join(work, "stderr.log"), errors="replace").read()
    bad = [l for l in stderr.splitlines() if "panicked" in l or "fatal" in l.lower()]
    check("nothing on stderr looks like a panic", not bad, bad[:3] or f"{len(stderr.splitlines())} lines")
    ok = all(c["ok"] for c in checks)
    for c in checks:
        print(("PASS " if c["ok"] else "FAIL ") + c["check"] + (f"  [{c['detail']}]" if not c["ok"] else ""))
    print("f92 scripted run:", "PASS" if ok else "FAIL", f"({sum(c['ok'] for c in checks)}/{len(checks)})")
    shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


main()
