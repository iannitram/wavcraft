#!/usr/bin/env python3
"""f127 alpha run: Stretch and Pitch and the pitch readout, in a scratch HOME.

    python3 tools/alpha/f127.py --exe <path to the release wavcraft> [--port 47227]

A 4 s mono 220 Hz sine is opened. Checked via STATE, PITCHDETECT and a saved copy:
  1. PITCHDETECT reads 220 Hz +/- 1;
  2. STRETCH ratio=1.5 pitch=0 makes the file 1.5x as long, selects it, names the step
     "Stretch and Pitch"; the saved file still reads about 220 Hz;
  3. STRETCH ratio=1 pitch=12 keeps the length and PITCHDETECT then reads about 440 Hz;
  4. a bad request answers ERR and adds no step; Undo returns the earlier length;
  5. the app's log holds no ERROR line.
"""
import argparse, glob, math, os, shutil, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

RATE = 48000
SECS = 4


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(int(16000 * math.sin(2 * math.pi * 220 * i / RATE)).to_bytes(2, "little", signed=True) for i in range(SECS * RATE)))


def hz(ans):
    return float(ans.split("hz=")[1]) if "hz=" in ans and "none" not in ans else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47227)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="f127.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    wav, saved, token = os.path.join(work, "in.wav"), os.path.join(work, "out.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    checks = []

    def check(label, ok, detail):
        checks.append((label, bool(ok), detail))

    proc = subprocess.Popen([args.exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            if os.path.exists(token):
                break
            time.sleep(0.2)
        c = Control(args.port, open(token).read().strip())
        n = SECS * RATE
        st = c.state()
        check("the file opens", st["frames"] == str(n), st)
        a = c.ask("PITCHDETECT")
        check("PITCHDETECT reads 220 Hz +/- 1", hz(a) is not None and abs(hz(a) - 220) <= 1, a)
        a = c.ask("STRETCH ratio=1.5 pitch=0")
        st = c.state()
        frames = int(st["frames"])
        check("STRETCH 1.5 makes the file 1.5x as long", abs(frames / n - 1.5) < 0.01 and st["undo"] == "Stretch and Pitch" and st["selection"] == f"0-{frames}", f"{a}; {st}")
        a = c.ask("PITCHDETECT")
        check("pitch is still 220 Hz after the stretch", hz(a) is not None and abs(hz(a) - 220) <= 3, a)
        steps = int(c.state()["steps"])
        bad = [c.ask("STRETCH ratio=0"), c.ask("STRETCH ratio=2 pitch=99"), c.ask("STRETCH speed=2")]
        check("bad requests answer ERR and add no step", all(b.startswith("ERR") for b in bad) and int(c.state()["steps"]) == steps, bad)
        c.ask("ACTION edit.undo")
        check("Undo returns the original length", c.state()["frames"] == str(n), c.state())
        a = c.ask("STRETCH ratio=1 pitch=12")
        st = c.state()
        check("pitch +12 keeps the length", abs(int(st["frames"]) - n) < n * 0.01, f"{a}; {st}")
        a = c.ask("PITCHDETECT")
        check("pitch +12 reads about 440 Hz", hz(a) is not None and abs(hz(a) - 440) <= 8, a)
        c.ask(f"SAVE {saved}")
        for _ in range(50):
            if os.path.exists(saved) and c.state().get("busy") == "0":
                break
            time.sleep(0.2)
        got = 0
        if os.path.exists(saved):
            with wave.open(saved, "rb") as w:
                got = w.getnframes()
        check("the saved copy has the processed length", abs(got - n) < n * 0.01, f"{got} frames")
        c.ask("QUIT discard")
        proc.wait(timeout=30)
    finally:
        if proc.poll() is None:
            proc.kill()
    errors = [l.strip()[:200] for p in glob.glob(os.path.join(logs, "*")) if os.path.isfile(p) for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR line", not errors, errors[:3])
    ok = all(o for _, o, _ in checks)
    for label, o, detail in checks:
        print(("PASS " if o else "FAIL ") + label + ("" if o else f"  [{detail}]"))
    if ok and not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    else:
        print("kept", work)
    print("f127 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


main()
