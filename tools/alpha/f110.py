#!/usr/bin/env python3
"""f110 scripted run: session markers and ranges through the control channel (port 47210).

    python3 tools/alpha/f110.py --exe <path to the release wavcraft> [--keep]

A session with one synthetic clip: SMARKER add (point and range), rename, delete, next/prev
navigation, playhead and selection setters, addhere/addsel, refusals, the markers in SESSION state;
then QUIT. Fails on any ERROR line in the app's own log.
"""
import argparse, json, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

PORT, RATE = 47210, 48000


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
    work = tempfile.mkdtemp(prefix="f110.", dir=root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    a = os.path.join(work, "a.wav")
    make_wav(a, RATE, 2, 10.0, 220)
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

    def mk():
        a = c.ask("SMARKER state")
        return json.loads(a[3:]) if a.startswith("OK ") else {}

    try:
        check("SMARKER before a session is refused", c.ask("SMARKER add 1").startswith("ERR no session"))
        check("SESSION new", c.ask("SESSION new Demo rate=48000 depth=24") == "OK")
        c.ask("TRACK add Drums stereo")
        check("a clip so the session has content", c.ask(f'CLIP add Drums "{a}" 0') == "OK clips=1")
        check("VIEW multitrack", c.ask("VIEW multitrack") == "OK view=multitrack")
        check("add a marker at 4 s", c.ask("SMARKER add 4") == "OK index=1 markers=1")
        check("add a range 1 s to 2.5 s (sorts first)", c.ask("SMARKER add 1 2.5") == "OK index=1 markers=2")
        m = mk()["markers"]
        check("the state lists the range then the marker", [(x["name"], x["range"], x["start"], x["end"]) for x in m] == [("Range 2", True, 1.0, 2.5), ("Marker 1", False, 4.0, 4.0)], m)
        check("rename marker 2", c.ask('SMARKER rename 2 "Chorus start"') == "OK" and mk()["markers"][1]["name"] == "Chorus start")
        check("SESSION state carries the markers too", len(c.state()["markers"]) == 2)
        check("next goes to the range at 1 s", c.ask("SMARKER next") == "OK index=1 playhead=1")
        check("next goes to 4 s", c.ask("SMARKER next") == "OK index=2 playhead=4")
        check("no next after the last", c.ask("SMARKER next").startswith("ERR") and mk()["playhead"] == 4.0)
        check("prev goes back to 1 s", c.ask("SMARKER prev") == "OK index=1 playhead=1")
        check("no prev before the first", c.ask("SMARKER prev").startswith("ERR"))
        check("playhead 6 then addhere", c.ask("SMARKER playhead 6") == "OK" and c.ask("SMARKER addhere") == "OK index=3 markers=3")
        check("select 7 to 9 then addsel makes a range", c.ask("SMARKER select 7 9") == "OK" and c.ask("SMARKER addsel") == "OK index=4 markers=4")
        last = mk()["markers"][3]
        check("the range runs 7 s to 9 s", (last["start"], last["end"], last["range"]) == (7.0, 9.0, True), last)
        check("delete marker 1", c.ask("SMARKER delete 1") == "OK markers=3" and [x["name"] for x in mk()["markers"]][0] == "Chorus start")
        check("bad index and bad time are refused", c.ask("SMARKER delete 9").startswith("ERR") and c.ask("SMARKER add -1").startswith("ERR") and c.ask("SMARKER add 4 2").startswith("ERR"))
        check("nothing changed after the refusals", len(mk()["markers"]) == 3)
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
