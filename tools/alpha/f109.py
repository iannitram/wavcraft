#!/usr/bin/env python3
"""f109 scripted run: per-track automation lanes (volume, pan) through the control channel (port 47209).

    python3 tools/alpha/f109.py --exe <path to the release wavcraft> [--keep]

Starts the app with HOME, the log and the session folder inside a scratch folder. Makes a session
with two tracks, shows a lane, and drives AUTO add / move / delete / thin / state; checks the points,
their order, the clamping of out-of-range values, refused input, and that the app is still up; then
QUIT. Fails on any ERROR line in the app's own log.
"""
import argparse, json, os, shutil, socket, subprocess, sys, tempfile, time

PORT = 47209


class Control:
    def __init__(self, port, token_file):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        token = open(token_file).read().strip() if os.path.exists(token_file) else ""
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("the control channel refused the token")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def auto(self):
        a = self.ask("AUTO state")
        return json.loads(a[3:]) if a.startswith("OK ") else {}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f109.", dir=root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([args.exe], env=env, stdout=err, stderr=err)
    checks = []

    def check(label, ok, detail=""):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    time.sleep(1)
    c = Control(PORT, token)
    try:
        check("AUTO before a session is refused", c.ask("AUTO state").startswith("ERR no session"))
        check("SESSION new", c.ask("SESSION new Demo rate=48000 depth=24") == "OK")
        c.ask("TRACK add Drums stereo")
        c.ask("TRACK add Lead mono")
        check("AUTO show Drums volume", c.ask("AUTO show Drums volume") == "OK" and c.auto()["shown"] == {"Drums": "volume"}, c.auto())
        check("add three points out of order", c.ask("AUTO add Drums volume 2 -6") == "OK index=0 points=1"
              and c.ask("AUTO add Drums volume 1 0") == "OK index=0 points=2"
              and c.ask("AUTO add Drums volume 3 -12") == "OK index=2 points=3")
        check("points come back in time order", c.auto()["lanes"]["Drums"]["volume"] == [[1.0, 0.0], [2.0, -6.0], [3.0, -12.0]], c.auto())
        check("a value above the range is clamped to +12 dB", c.ask("AUTO add Drums volume 4 99") == "OK index=3 points=4" and c.auto()["lanes"]["Drums"]["volume"][3] == [4.0, 12.0])
        check("move point 3 earlier and lower", c.ask("AUTO move Drums volume 3 2.5 -9") == "OK index=2 points=4")
        check("the moved point sits in order", c.auto()["lanes"]["Drums"]["volume"] == [[1.0, 0.0], [2.0, -6.0], [2.5, -9.0], [3.0, -12.0]], c.auto())
        check("thin removes the redundant middle points", c.ask("AUTO thin Drums volume 0.1").startswith("OK removed=2 "), c.auto())
        check("the corners stay", c.auto()["lanes"]["Drums"]["volume"] == [[1.0, 0.0], [3.0, -12.0]], c.auto())
        check("delete point 0", c.ask("AUTO delete Drums volume 0") == "OK points=1")
        check("a pan lane on another track by position", c.ask("AUTO add 2 pan 0 -1") == "OK index=0 points=1" and c.ask("AUTO add 2 pan 4 1") == "OK index=1 points=2")
        check("pan lane is shown too", c.ask("AUTO show Lead pan") == "OK" and c.auto()["shown"]["Lead"] == "pan")
        check("a point that does not exist is refused", c.ask("AUTO delete Drums volume 9").startswith("ERR no point 9"))
        check("mute lanes are refused", c.ask("AUTO add Drums mute 1 1").startswith("ERR automation lanes here"))
        check("a negative time is refused", c.ask("AUTO add Drums volume -1 0").startswith("ERR"))
        check("hide the lane", c.ask("AUTO show Drums off") == "OK" and "Drums" not in c.auto()["shown"])
        check("the points survive hiding", len(c.auto()["lanes"]["Drums"]["volume"]) == 1)
        st = c.ask("SESSION state")
        check("SESSION state names the lanes", '"automation":[{"param":"volume","points":1}]' in st, st[:200])
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
