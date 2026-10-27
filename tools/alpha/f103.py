#!/usr/bin/env python3
"""f103 scripted run: multitrack basics through the control channel (port 47203).

    python3 tools/alpha/f103.py --exe <path to the release wavcraft> [--keep]

Two small synthetic WAVs are written to a scratch folder; the app starts with HOME, the log and
the session folder inside it. Checked through SESSION state: SESSION new, two tracks, two clips
(one by TRACK name, one by position), a rename, mute and volume, the view switch both ways, a
refused clip (wrong rate), and a track removal that takes its clip along; then QUIT. Fails on
any ERROR line in the app's own log.
"""
import argparse, json, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

PORT, RATE = 47203, 48000


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
    work = tempfile.mkdtemp(prefix="f103.", dir=root)
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
        check("no session before SESSION new", c.ask("SESSION state").endswith(" none"), c.ask("SESSION state"))
        check("TRACK add before a session is refused", c.ask("TRACK add").startswith("ERR no session"))
        check("SESSION new Demo rate=48000", c.ask("SESSION new Demo rate=48000 depth=24") == "OK")
        s = c.state()
        check("the session has its name, rate and depth and no tracks", (s.get("name"), s.get("rate"), s.get("bits"), len(s.get("tracks", []))) == ("Demo", RATE, 24, 0), s)
        check("TRACK add Drums", c.ask("TRACK add Drums stereo") == "OK tracks=1")
        c.ask("TRACK add Lead mono")
        c.ask("TRACK add")
        s = c.state()
        names = [t["name"] for t in s["tracks"]]
        check("three tracks exist", len(names) == 3, names)
        check("TRACK rename 3 Bass", c.ask("TRACK rename 3 Bass") == "OK" and "Bass" in [t["name"] for t in c.state()["tracks"]])
        check("CLIP add by track name", c.ask(f'CLIP add Drums "{a}" 1.0') == "OK clips=1")
        check("CLIP add by position", c.ask(f'CLIP add 3 "{b}" 0.5') == "OK clips=2")
        s = c.state()
        clips = {t["name"]: t["clips"] for t in s["tracks"]}
        check("two clips at their positions and lengths", clips["Drums"] == [{"name": "a", "start": 1.0, "length": 2.0, "channels": 2, "gain_db": 0.0, "offline": False, "tempo": None, "beats": None, "key": None, "follow_ratio": None}] and clips["Bass"] == [{"name": "b", "start": 0.5, "length": 1.0, "channels": 1, "gain_db": 0.0, "offline": False, "tempo": None, "beats": None, "key": None, "follow_ratio": None}], clips)
        check("the session ends at 3.0 s", abs(s["end"] - 3.0) < 1e-9, s["end"])
        check("a clip at the wrong rate is refused", c.ask(f'CLIP add Drums "{odd}" 0').startswith("ERR") and c.state()["end"] == s["end"])
        check("TRACK mute and volume", c.ask("TRACK mute Bass 1") == "OK" and c.ask("TRACK volume Bass -6") == "OK")
        bass = [t for t in c.state()["tracks"] if t["name"] == "Bass"][0]
        check("Bass is muted at -6 dB", bass["mute"] is True and bass["volume_db"] == -6.0, bass)
        check("VIEW waveform and back", c.ask("VIEW waveform") == "OK view=waveform" and c.ask("SESSION state").startswith("OK view=waveform") and c.ask("VIEW multitrack") == "OK view=multitrack")
        check("TRACK remove takes its clip along", c.ask("TRACK remove Drums") == "OK tracks=2" and sum(len(t["clips"]) for t in c.state()["tracks"]) == 1)
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
