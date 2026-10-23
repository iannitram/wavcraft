#!/usr/bin/env python3
"""f01 alpha run: the effect seam (Invert) through the control channel, in a scratch HOME.

    python3 tools/alpha/f01.py --exe <path to the release wavcraft> [--port 47101]

A 3 s synthetic stereo file (left constant +0.25, right constant -0.5) is opened. Checked, via
STATE and a saved copy read back:
  1. ACTION effect.invert on a selection of 1 s to 2 s: the step is named "Invert", the length
     is unchanged, only that range is flipped;
  2. Left Channel Only, nothing selected, ACTION effect.invert: the whole left channel is
     flipped, the right is not;
  3. an unknown effect answers ERR; Undo names the step; the app's log holds no ERROR line.
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
    ap.add_argument("--port", type=int, default=47101)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="f01.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
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
        st = c.state()
        n = 3 * RATE
        check("the file opens", st["frames"] == str(n) and st["steps"] == "1", st)
        c.ask(f"SELECT {RATE} {2 * RATE}")
        a = c.ask("ACTION effect.invert")
        st = c.state()
        check("Invert on a selection: step named Invert, length kept", a.endswith("changed=1") and st["undo"] == "Invert" and st["frames"] == str(n) and st["steps"] == "2", f"{a}; {st}")
        c.ask("ACTION edit.leftChannel")
        c.ask("SELECT 0 0")
        a = c.ask("ACTION effect.invert")
        st = c.state()
        check("Invert with nothing selected and the left channel only", a.endswith("changed=1") and st["steps"] == "3" and st["undo"] == "Invert", f"{a}; {st}")
        check("an unknown effect is refused", c.ask("ACTION effect.nonesuch").startswith("ERR"), "ERR expected")
        c.ask(f"SAVE {saved}")
        st = c.state()
        check("save finished", os.path.exists(saved) and st.get("busy") == "0", st)
        # left: +.25 -> 1..2 s flipped -> whole flipped; right: -.5, flipped only in 1..2 s.
        want = {RATE // 2: (-0.25, -0.5), RATE * 3 // 2: (0.25, 0.5), RATE * 5 // 2: (-0.25, -0.5)}
        for frame, (l, r) in want.items():
            got = sample(saved, frame)
            check(f"saved samples at {frame / RATE:.1f} s", abs(got[0] - l) < 2e-3 and abs(got[1] - r) < 2e-3, f"got {got}, want {(l, r)}")
        c.ask("ACTION edit.undo")
        st = c.state()
        check("Undo names the effect's step", st["redo"] == "Invert" and st["steps"] == "3", st)
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
    print("f01 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


main()
