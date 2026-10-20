#!/usr/bin/env python3
"""f121 scripted run: the CD layout through the control channel (port 47221).

    python3 tools/alpha/f121.py --exe <path to the release wavcraft> [--keep]

Two short synthetic WAVs (one 44.1 kHz stereo, one 22.05 kHz mono) are written to a scratch
folder; the app starts with HOME, the log and the session folder inside it. Checked: CD add x2,
CD pause, CD meta (and a refused ISRC that changes nothing), CD state, CD export; then the CUE
times and the exported WAV's length and format are read from the files; QUIT. Fails on any
ERROR line in the app's own log.
"""
import argparse, json, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

PORT = 47221


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



import socket


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f121.", dir=root)
    home, logs, sessions, out = (os.path.join(work, d) for d in ("home", "logs", "sessions", "out"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    a, b = (os.path.join(work, n) for n in ("a.wav", "b.wav"))
    make_wav(a, 44100, 2, 3.0, 220)   # 3.0 s = 225 CD frames
    make_wav(b, 22050, 1, 1.0, 330)   # 1.0 s = 75 CD frames after conversion
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([args.exe], env=env, stdout=err, stderr=err)
    checks = []

    def check(label, ok, detail=""):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    for _ in range(60):
        if os.path.exists(token):
            break
        time.sleep(0.5)
    time.sleep(0.5)
    c = Control(PORT, open(token).read().strip())
    try:
        check("CD add a (stereo 44.1 kHz)", c.ask(f'CD add "{a}"') == "OK tracks=1")
        check("CD add b (mono 22.05 kHz)", c.ask(f'CD add "{b}"') == "OK tracks=2")
        check("CD pause 2 0.5", c.ask("CD pause 2 0.5") == "OK")
        check("CD meta 1", c.ask('CD meta 1 title="First Song" performer=Band isrc=us-abc-24-00001') == "OK")
        check("a bad ISRC is refused and changes nothing", c.ask("CD meta 2 title=Changed isrc=nope").startswith("ERR") and "Changed" not in c.ask("CD state"))
        st = c.ask("CD state")
        check("state: 2 tracks, 150+225+38+75 = 488 frames, 00:06:38", "frames=488" in st and "total=00:06:38" in st and "over=false" in st, st)
        r = c.ask(f'CD export "{out}"')
        check("CD export", r.startswith("OK exported"), r)
        cue = open(os.path.join(out, "disc.cue")).read() if os.path.exists(os.path.join(out, "disc.cue")) else ""
        want = ["TITLE \"First Song\"", "PERFORMER \"Band\"", "ISRC USABC2400001", "INDEX 00 00:00:00", "INDEX 01 00:02:00", "INDEX 00 00:05:00", "INDEX 01 00:05:38"]
        check("CUE has the metadata and times (00:02:00, 00:05:00, 00:05:38)", all(x in cue for x in want), cue)
        wp = os.path.join(out, "disc.wav")
        w = wave.open(wp, "rb")
        check("WAV is 44.1 kHz 16-bit stereo", (w.getframerate(), w.getsampwidth(), w.getnchannels()) == (44100, 2, 2), (w.getframerate(), w.getsampwidth(), w.getnchannels()))
        check("WAV length is 488 CD frames", w.getnframes() == 488 * 588, w.getnframes())
        w.close()
        check("no temporary file is left", not [f for f in os.listdir(out) if "wavcraft-tmp" in f], os.listdir(out))
        time.sleep(0.5)
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
