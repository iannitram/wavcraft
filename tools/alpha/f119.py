#!/usr/bin/env python3
"""f119 scripted run: clip tempo/beat properties through the control channel (port 47219).

    python3 tools/alpha/f119.py --exe <path to the release wavcraft> [--keep]

A synthetic WAV is written to a scratch folder; the app starts with HOME, the log and the session
folder inside it. CLIPPROP sets tempo, beats, key, name and gain; SESSION state shows them; bad
values are refused and change nothing; clearing works; then QUIT. Fails on any ERROR in the log.
"""
import argparse, json, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

PORT, RATE = 47219, 48000


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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f119.", dir=root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    a = os.path.join(work, "loop.wav")
    make_wav(a, RATE, 2, 2.0, 220)
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

    def clip():
        return c.state()["tracks"][0]["clips"][0]

    try:
        check("SESSION new", c.ask("SESSION new Demo rate=48000") == "OK")
        c.ask("TRACK add Drums stereo")
        check("CLIP add", c.ask(f'CLIP add Drums "{a}" 1.0') == "OK clips=1")
        k = clip()
        check("a new clip has no tempo and 0 dB", (k["tempo"], k["beats"], k["key"], k["gain_db"]) == (None, None, None, 0.0), k)
        check("CLIPPROP sets tempo, beats, name, gain", c.ask('CLIPPROP Drums 0 tempo=96 beats=4 name="Drum Loop" gain=-3.5') == "OK")
        k = clip()
        check("SESSION state shows them", (k["name"], k["tempo"], k["beats"], k["gain_db"]) == ("Drum Loop", 96.0, 4.0, -3.5), k)
        check("the follow ratio is session tempo over clip tempo", abs(k["follow_ratio"] - 120 / 96) < 1e-9, k)
        check("key is text", c.ask('CLIPPROP 1 0 key="A minor"') == "OK" and clip()["key"] == "A minor")
        before = clip()
        check("a bad tempo is refused", c.ask("CLIPPROP Drums 0 tempo=abc").startswith("ERR"))
        check("an out-of-range tempo is refused", c.ask("CLIPPROP Drums 0 tempo=5000 name=Zed").startswith("ERR"))
        check("a refused change alters nothing", clip() == before, clip())
        check("a missing clip is refused", c.ask("CLIPPROP Drums 3 tempo=100").startswith("ERR"))
        check("tempo=none clears the tempo only", c.ask("CLIPPROP Drums 0 tempo=none") == "OK" and clip()["tempo"] is None and clip()["beats"] == 4.0)
        time.sleep(1.0)
        check("the app is still up", proc.poll() is None)
        check("QUIT", c.ask("QUIT") == "OK")
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
