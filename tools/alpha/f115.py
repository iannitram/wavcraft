#!/usr/bin/env python3
"""f115 scripted run: mono, stereo and 5.1 track formats through the control channel (port 47215).

    python3 tools/alpha/f115.py --exe <path to the release wavcraft> [--keep]

Synthetic WAVs (mono, stereo, 5.1) are written to a scratch folder; the app starts with HOME, the
log and the session folder inside it. Checked through SESSION state: tracks added as mono, stereo
and 5.1 with their format shown, clips that fit, mono into stereo and stereo into mono converted,
mismatches refused (2 into 5.1, 6 into mono), a format change refused while a track has clips and
accepted once it is empty; then QUIT. Fails on any ERROR line in the app's own log.
"""
import argparse, json, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

PORT, RATE = 47215, 48000


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
    work = tempfile.mkdtemp(prefix="f115.", dir=root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    w1, w2, w6 = (os.path.join(work, n) for n in ("m1.wav", "s2.wav", "x6.wav"))
    make_wav(w1, RATE, 1, 1.0, 330)
    make_wav(w2, RATE, 2, 1.0, 220)
    make_wav(w6, RATE, 6, 1.0, 110)
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

    def tracks():
        return {t["name"]: t for t in c.state()["tracks"]}

    try:
        check("SESSION new", c.ask("SESSION new Fmt rate=48000 depth=24") == "OK")
        check("TRACK add Mon fmt=mono", c.ask("TRACK add Mon fmt=mono") == "OK tracks=1")
        check("TRACK add Ste fmt=stereo", c.ask("TRACK add Ste fmt=stereo") == "OK tracks=2")
        check("TRACK add Sur fmt=5.1", c.ask("TRACK add Sur fmt=5.1") == "OK tracks=3")
        check("an unknown format is refused", c.ask("TRACK add Bad fmt=7.1").startswith("ERR") and len(tracks()) == 3)
        t = tracks()
        check("formats and channel counts are in the state", [(t[n]["format"], t[n]["channels"]) for n in ("Mon", "Ste", "Sur")] == [("Mono", 1), ("Stereo", 2), ("5.1", 6)], t)
        check("mono clip on the mono track", c.ask(f'CLIP add Mon "{w1}" 0') == "OK clips=1")
        check("stereo clip on the stereo track", c.ask(f'CLIP add Ste "{w2}" 0') == "OK clips=2")
        check("5.1 clip on the 5.1 track", c.ask(f'CLIP add Sur "{w6}" 0') == "OK clips=3")
        check("mono clip into stereo is duplicated (2 channels)", c.ask(f'CLIP add Ste "{w1}" 2') == "OK clips=4" and [x["channels"] for x in tracks()["Ste"]["clips"]] == [2, 2])
        check("stereo clip into mono is summed (1 channel)", c.ask(f'CLIP add Mon "{w2}" 2') == "OK clips=5" and [x["channels"] for x in tracks()["Mon"]["clips"]] == [1, 1])
        r = c.ask(f'CLIP add Sur "{w2}" 5')
        check("stereo clip on 5.1 is refused with a message", r.startswith("ERR") and "5.1" in r and c.state()["tracks"][2]["clips"].__len__() == 1, r)
        r = c.ask(f'CLIP add Mon "{w6}" 5')
        check("5.1 clip on mono is refused with a message", r.startswith("ERR") and "Mono" in r, r)
        r = c.ask("TRACK format Mon stereo")
        check("a format change with clips is refused", r.startswith("ERR") and "clip" in r and tracks()["Mon"]["format"] == "Mono", r)
        check("TRACK remove, add again, format change on the empty track", c.ask("TRACK add Empty fmt=mono") == "OK tracks=4" and c.ask("TRACK format Empty 5.1") == "OK" and tracks()["Empty"]["format"] == "5.1")
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
