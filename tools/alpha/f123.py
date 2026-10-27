#!/usr/bin/env python3
"""f123 scripted run: audio types through the control channel (port 47223).

    python3 tools/alpha/f123.py --exe <path to the release wavcraft> [--keep]

Synthetic WAVs are written to a scratch folder; the app starts with HOME, the log and the session
folder inside it. Checked through SESSION state: CLIPTYPE assign, multi-assign, clear, filter, errors; then
QUIT. Fails on any ERROR line in the app's own log.
"""
import argparse, json, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

PORT, RATE = 47223, 48000


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
    work = tempfile.mkdtemp(prefix="f123.", dir=root)
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
        check("SESSION new", c.ask("SESSION new Demo rate=48000 depth=24") == "OK")
        c.ask("TRACK add Voice mono")
        c.ask("TRACK add Bed stereo")
        check("three clips added", c.ask(f'CLIP add Voice "{a}" 0') == "OK clips=1" and c.ask(f'CLIP add Voice "{a}" 3') == "OK clips=2" and c.ask(f'CLIP add Bed "{b}" 0') == "OK clips=3")
        def types():
            return {t["name"]: [x.get("type") for x in t["clips"]] for t in c.state()["tracks"]}
        check("no types at first", types() == {"Voice": [None, None], "Bed": [None]}, types())
        check("CLIPTYPE Voice 1 dialogue", c.ask("CLIPTYPE Voice 1 dialogue") == "OK clips=1" and types()["Voice"] == ["dialogue", None], types())
        check("multi-assign: CLIPTYPE Voice 1,2 music", c.ask("CLIPTYPE Voice 1,2 music") == "OK clips=2" and types()["Voice"] == ["music", "music"], types())
        check("CLIPTYPE Bed all sfx", c.ask("CLIPTYPE Bed all sfx") == "OK clips=1" and types()["Bed"] == ["sfx"], types())
        check("CLIPTYPE Voice 2 none clears one", c.ask("CLIPTYPE Voice 2 none") == "OK clips=1" and types()["Voice"] == ["music", None], types())
        check("an unknown type is refused", c.ask("CLIPTYPE Voice 1 foley").startswith("ERR no audio type"))
        check("a missing clip is refused and changes nothing", c.ask("CLIPTYPE Voice 1,9 dialogue").startswith("ERR no clip 9") and types()["Voice"] == ["music", None], types())
        check("filter shows in the state line", c.ask("CLIPTYPE filter sfx") == "OK filter=sfx" and c.state()["filter"] == "sfx")
        check("filter all", c.ask("CLIPTYPE filter all") == "OK filter=all" and c.state()["filter"] == "all")
        check("TRACK remove forgets the types", c.ask("TRACK remove Bed") == "OK tracks=1" and types() == {"Voice": ["music", None]}, types())
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
