#!/usr/bin/env python3
"""f14 alpha run: effect parameters, the generic dialog and Repeat Last Command, in a scratch HOME.

    python3 tools/alpha/f14.py --exe <path to the release wavcraft> [--port 47114]

A 3 s synthetic stereo file (left constant +0.25, right constant -0.5) is opened. Checked, via
STATE and a saved copy read back:
  1. EFFECT lists Gain with its setting; EFFECT gain gain_db=-6.0206 on 1 s to 2 s halves it;
     out-of-range, unknown-key and unknown-effect requests answer ERR and add no step;
  2. the dialog: opened on gain_db=-12.0412, CANCEL adds no step, OK adds a Gain step (2 s to 3 s);
  3. Repeat Last Command (no dialog) on 0 to 0.5 s repeats Gain with the dialog's -12 dB;
     Repeat Last Command with dialog reopens the dialog on those values;
  4. Invert (no settings) then Repeat with dialog runs again at once;
  5. the saved samples match, and the app's log holds no ERROR line.
"""
import argparse, glob, os, shutil, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

RATE = 48000


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        frame = (8192).to_bytes(2, "little", signed=True) + (-16384).to_bytes(2, "little", signed=True)
        w.writeframes(frame * (3 * RATE))


def sample(path, frame):
    """(left, right) of a PCM WAV at `frame`, as -1..1."""
    with wave.open(path, "rb") as w:
        ch, width = w.getnchannels(), w.getsampwidth()
        w.setpos(frame)
        raw = w.readframes(1)
    scale = float(1 << (8 * width - 1))
    return tuple(int.from_bytes(raw[i * width:(i + 1) * width], "little", signed=True) / scale for i in range(ch))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47114)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="f14.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
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
        n = 3 * RATE
        steps = lambda: int(c.state()["steps"])
        st = c.state()
        check("the file opens", st["frames"] == str(n) and st["steps"] == "1", st)
        listing = c.ask("EFFECT")
        check("EFFECT lists Gain and its setting", "gain [gain_db=0" in listing and "invert []" in listing, listing)
        c.ask(f"SELECT {RATE} {2 * RATE}")
        a = c.ask("EFFECT gain gain_db=-6.0206")
        st = c.state()
        check("EFFECT gain on 1 s to 2 s: step named Gain", a.endswith("changed=1") and st["undo"] == "Gain" and st["steps"] == "2", f"{a}; {st}")
        before = steps()
        bad = [c.ask("EFFECT gain gain_db=99"), c.ask("EFFECT gain volume=1"), c.ask("EFFECT gain gain_db=loud"), c.ask("EFFECT nonesuch")]
        check("bad requests answer ERR and add no step", all(b.startswith("ERR") for b in bad) and steps() == before, bad)
        # the dialog
        c.ask(f"SELECT {2 * RATE} {3 * RATE}")
        o = c.ask("EFFECT_DIALOG gain gain_db=-12.0412")
        d = c.ask("EFFECT_DIALOG")
        check("the dialog opens on the given values", o == "OK" and "open=gain" in d and "-12.04" in d, f"{o}; {d}")
        c.ask("EFFECT_DIALOG CANCEL")
        check("Cancel adds no step and closes it", steps() == before and c.ask("EFFECT_DIALOG") == "OK open=none", c.state())
        c.ask("EFFECT_DIALOG gain gain_db=-12.0412")
        a = c.ask("EFFECT_DIALOG OK")
        st = c.state()
        check("OK applies Gain to the selection", a.endswith("changed=1") and st["undo"] == "Gain" and steps() == before + 1, f"{a}; {st}")
        # Repeat Last Command
        c.ask(f"SELECT 0 {RATE // 2}")
        a = c.ask("ACTION edit.repeatLast")
        check("Repeat Last Command repeats Gain with the same settings", a.endswith("changed=1") and steps() == before + 2 and c.state()["undo"] == "Gain", a)
        c.ask(f"SELECT {RATE // 2} {RATE}")
        a = c.ask("ACTION edit.repeatLastDialog")
        d = c.ask("EFFECT_DIALOG")
        check("Repeat with dialog reopens the dialog on the last values", "open=gain" in d and "-12.04" in d and steps() == before + 2, f"{a}; {d}")
        c.ask("EFFECT_DIALOG CANCEL")
        # no-settings effect: Repeat with dialog runs at once
        c.ask(f"SELECT {RATE // 2} {RATE}")
        c.ask("EFFECT invert")
        n0 = steps()
        c.ask(f"SELECT {5 * RATE // 2} {3 * RATE}")
        a = c.ask("ACTION edit.repeatLastDialog")
        check("Invert has no dialog: Repeat with dialog runs it at once", a.endswith("changed=1") and steps() == n0 + 1 and c.ask("EFFECT_DIALOG") == "OK open=none", f"{a}; {c.state()}")
        c.ask(f"SAVE {saved}")
        st = c.state()
        check("save finished", os.path.exists(saved) and st.get("busy") == "0", st)
        want = {
            RATE // 4: (0.0625, -0.125),       # repeat of Gain -12 dB
            RATE * 3 // 4: (-0.25, 0.5),       # Invert
            RATE * 3 // 2: (0.125, -0.25),     # Gain -6 dB
            RATE * 9 // 4: (0.0625, -0.125),   # dialog Gain -12 dB
            RATE * 11 // 4: (-0.0625, 0.125),  # dialog Gain, then Invert by Repeat
        }
        for frame, (l, r) in want.items():
            got = sample(saved, frame)
            check(f"saved samples at {frame / RATE:.2f} s", abs(got[0] - l) < 2e-3 and abs(got[1] - r) < 2e-3, f"got {got}, want {(l, r)}")
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
    print("f14 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


main()
