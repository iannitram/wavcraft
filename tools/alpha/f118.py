#!/usr/bin/env python3
"""f118 scripted run: master track and 5.1 through the control channel (port 47218).

    python3 tools/alpha/f118.py --exe <path to the release wavcraft> [--keep]

Checked through SESSION state: a stereo session has a stereo master and refuses a 5.1 track;
MASTER vol and mute are kept; SESSION new master=5.1 makes a 6-channel master and accepts a 5.1
track; bad master arguments are refused; then QUIT. Fails on any ERROR line in the app's log.
"""
import argparse, json, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

PORT, RATE = 47218, 48000


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
    work = tempfile.mkdtemp(prefix="f118.", dir=root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    a, b, odd = (os.path.join(work, n) for n in ("a.wav", "b.wav", "odd.wav"))
    make_wav(a, RATE, 2, 2.0, 220)
    make_wav(b, RATE, 1, 1.0, 330)
    make_wav(odd, 44100, 1, 0.5, 440)
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
        check("SESSION new Stereo (default master)", c.ask("SESSION new Stereo rate=48000") == "OK")
        m = c.state()["master"]
        check("the default master is stereo, 0 dB, unmuted", (m["format"], m["channels"], m["volume_db"], m["mute"]) == ("Stereo", 2, 0.0, False), m)
        check("a 5.1 track is refused in a stereo session", c.ask("TRACK add Surr 5.1").startswith("ERR"))
        check("MASTER vol=-6 mute=1", c.ask("MASTER vol=-6 mute=1") == "OK")
        m = c.state()["master"]
        check("the master keeps -6 dB and mute", (m["volume_db"], m["mute"]) == (-6.0, True), m)
        check("MASTER mute=0", c.ask("MASTER mute=0") == "OK" and c.state()["master"]["mute"] is False)
        check("the master is not listed as a track", c.state()["tracks"] == [])
        check("a bad master argument is refused", c.ask("MASTER vol=loud").startswith("ERR") and c.ask("SESSION new Bad master=quad").startswith("ERR"))
        check("SESSION new Surround master=5.1", c.ask("SESSION new Surround rate=48000 master=5.1") == "OK")
        m = c.state()["master"]
        check("the 5.1 master has 6 channels", (m["format"], m["channels"]) == ("5.1", 6), m)
        check("a 5.1 track is accepted", c.ask("TRACK add Surr 5.1") == "OK tracks=1")
        t = c.state()["tracks"][0]
        check("the track is 6 channels", t["channels"] == 6, t)
        check("MASTER on the 5.1 session", c.ask("MASTER vol=-3") == "OK" and c.state()["master"]["volume_db"] == -3.0)
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
