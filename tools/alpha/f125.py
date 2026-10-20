#!/usr/bin/env python3
"""f125 alpha run: Pitch Shifter and Saturator through the control channel, in a scratch HOME.

    python3 tools/alpha/f125.py --exe <path to the release wavcraft> [--port 47225]

A 3 s synthetic mono 440 Hz sine (peak 0.5) is opened. Checked via STATE and a saved copy:
  1. EFFECT lists both effects; EFFECT pitch_shift semitones=12 adds a step, keeps the length,
     and the saved file's strongest frequency near 440 Hz is about 880 Hz;
  2. undo, then EFFECT saturator drive=12 output=-6: the saved peak stays under the -6 dB ceiling
     and the 3rd harmonic (1320 Hz) appears;
  3. bad settings answer ERR and add no step; the app's log has no ERROR line.
"""
import argparse, glob, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

RATE = 44100


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(0.5 * 32767 * math.sin(2 * math.pi * 440 * i / RATE))) for i in range(3 * RATE)))


def read(path):
    with wave.open(path, "rb") as w:
        n = w.getnframes()
        raw = w.readframes(n)
    return n, [v / 32768.0 for v in struct.unpack("<%dh" % n, raw)]


def goertzel(x, f):
    w = 2 * math.pi * f / RATE
    re = sum(s * math.cos(w * i) for i, s in enumerate(x))
    im = sum(s * math.sin(w * i) for i, s in enumerate(x))
    return math.hypot(re, im) / (len(x) / 2)


def peak(x, lo, hi):
    return max(range(lo, hi + 1), key=lambda f: goertzel(x, f))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47225)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="f125.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    wav, token = os.path.join(work, "in.wav"), os.path.join(work, "control.token")
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
        n = 3 * RATE
        steps = lambda: int(c.state()["steps"])
        st = c.state()
        check("the file opens", st["frames"] == str(n) and st["steps"] == "1", st)
        listing = c.ask("EFFECT")
        check("EFFECT lists both", "pitch_shift [" in listing and "saturator [" in listing, listing)
        a = c.ask("EFFECT pitch_shift semitones=12")
        st = c.state()
        check("pitch_shift adds a step and keeps the length", a.endswith("changed=1") and st["undo"] == "Pitch Shifter" and st["frames"] == str(n), f"{a}; {st}")
        c.ask(f"SAVE {work}/pitch.wav")
        c.state()
        got_n, x = read(f"{work}/pitch.wav")
        f = peak(x[RATE // 2:5 * RATE // 2], 800, 960)
        check("saved length is the same", got_n == n, got_n)
        check("measured frequency is about 880 Hz", 878 <= f <= 882, f"{f} Hz")
        c.ask("ACTION edit.undo")
        a = c.ask("EFFECT saturator drive=12 output=-6")
        st = c.state()
        check("saturator adds a step", a.endswith("changed=1") and st["undo"] == "Saturator", f"{a}; {st}")
        c.ask(f"SAVE {work}/sat.wav")
        c.state()
        _, y = read(f"{work}/sat.wav")
        pk = max(abs(s) for s in y)
        ceiling = 10 ** (-6 / 20)
        check("saturator peak is under the ceiling", pk <= ceiling + 1e-3, f"peak {pk:.4f}, ceiling {ceiling:.4f}")
        h3, h3in = goertzel(y[RATE:2 * RATE], 1320), goertzel(x[:0] or read(wav)[1][RATE:2 * RATE], 1320)
        check("saturator adds a 3rd harmonic", h3 > 0.02 and h3in < 0.005, f"in {h3in:.4f}, out {h3:.4f}")
        before = steps()
        bad = [c.ask("EFFECT pitch_shift semitones=99"), c.ask("EFFECT saturator drive=loud"), c.ask("EFFECT saturator volume=1")]
        check("bad settings answer ERR and add no step", all(b.startswith("ERR") for b in bad) and steps() == before, bad)
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
    print("f125 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


main()
