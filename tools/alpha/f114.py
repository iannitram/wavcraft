#!/usr/bin/env python3
"""f114 scripted run: playing the multitrack session, silent (WAVCRAFT_NO_AUDIO=1), port 47214.

    python3 tools/alpha/f114.py --exe <path to the release wavcraft> [--keep]

Two synthetic WAVs go on two tracks; the app starts silent (no audio device is opened) with HOME,
the log and the session folder in a scratch folder. Checked through SESSION state: SPLAY moves the
position forward in real time, SSTOP holds it, a second play from an offset starts there, and after
a warm-up no underruns are reported; then QUIT. Fails on any ERROR line in the app's own log.
"""
import argparse, json, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

PORT, RATE = 47214, 48000


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
    work = tempfile.mkdtemp(prefix="f114.", dir=root)
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
               WAVCRAFT_NO_AUDIO="1", WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
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
        check("SESSION new Play rate=48000", c.ask("SESSION new Play rate=48000 depth=24") == "OK")
        c.ask("TRACK add One stereo")
        c.ask("TRACK add Two stereo")
        check("CLIP add on track one", c.ask(f'CLIP add One "{a}" 0') == "OK clips=1")
        check("CLIP add on track two", c.ask(f'CLIP add Two "{b}" 0.5') == "OK clips=2")
        s = c.state()
        check("not playing at first", s.get("playing") is False and s.get("position") == 0, s)
        check("SPLAY with no argument starts", c.ask("SPLAY") == "OK")
        time.sleep(1.5)  # warm-up
        s1 = c.state()
        u1 = s1.get("underruns")
        time.sleep(0.8)
        s2 = c.state()
        check("playing, and the position advanced", s1["playing"] and s2["position"] > s1["position"] + 0.5, (s1["position"], s2["position"]))
        check("the position follows the clock (about 2.3 s after 2.3 s)", 1.8 < s2["position"] < 3.0 or not s2["playing"], s2["position"])
        check("no underruns after the warm-up", s2.get("underruns") == u1 == 0, (u1, s2.get("underruns")))
        check("SSTOP", c.ask("SSTOP") == "OK")
        time.sleep(0.2)
        h = c.state()["position"]
        time.sleep(0.5)
        check("stopped: the position holds", c.state()["position"] == h and c.state()["playing"] is False, h)
        check("SPLAY 1.25 starts from there", c.ask("SPLAY 1.25") == "OK")
        time.sleep(0.3)
        p = c.state()["position"]
        check("the position is just past 1.25 s", 1.25 <= p < 2.0, p)
        check("a negative SPLAY position is refused", c.ask("SPLAY -1").startswith("ERR"))
        time.sleep(2.5)  # the content ends at 2.0 s: the transport stops itself
        check("it stops at the end of the content", c.state()["playing"] is False)
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
