#!/usr/bin/env python3
"""#935 control-verb run: EFFECT resolves the one-shot effects, in a scratch HOME.

    python3 tools/alpha/i935.py --exe <path to the release wavcraft> [--port 47135]

A 3 s synthetic stereo file (left constant +0.25, right constant -0.5) is opened. Checked:
  1. EFFECT fadeIn.linear on 0 to 1 s adds one "Fade In (Linear)" step, as ACTION
     effect.fadeIn.linear does on 1 s to 2 s; EFFECT reverse adds a "Reverse" step;
  2. EFFECT reverse with a setting, and EFFECT nonesuch, answer ERR and add no step;
  3. EFFECT_DIALOG reverse answers an ERR saying it has no dialog, and no dialog opens;
  4. the saved samples show both fades, and the app's log holds no ERROR line.
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
    ap.add_argument("--port", type=int, default=47135)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="i935.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
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
        steps = lambda: int(c.state()["steps"])
        st = c.state()
        check("the file opens", st["frames"] == str(3 * RATE) and st["steps"] == "1", st)
        c.ask(f"SELECT 0 {RATE}")
        a = c.ask("EFFECT fadeIn.linear")
        st = c.state()
        check("EFFECT fadeIn.linear adds a Fade In (Linear) step", a.endswith("changed=1") and st["undo"] == "Fade In (Linear)" and st["steps"] == "2", f"{a}; {st}")
        c.ask(f"SELECT {RATE} {2 * RATE}")
        b = c.ask("ACTION effect.fadeIn.linear")
        st = c.state()
        check("ACTION effect.fadeIn.linear answers and names its step the same way", b.endswith("changed=1") and st["undo"] == "Fade In (Linear)" and st["steps"] == "3", f"{b}; {st}")
        c.ask(f"SELECT {2 * RATE} {3 * RATE}")
        a = c.ask("EFFECT reverse")
        st = c.state()
        check("EFFECT reverse adds a Reverse step", a.endswith("changed=1") and st["undo"] == "Reverse" and st["steps"] == "4", f"{a}; {st}")
        before = steps()
        bad = [c.ask("EFFECT reverse amount=1"), c.ask("EFFECT nonesuch")]
        check("a setting on a one-shot, or an unknown name, answers ERR and adds no step",
              all(x.startswith("ERR") for x in bad) and "PARAM" in bad[0] and steps() == before, bad)
        d = c.ask("EFFECT_DIALOG reverse")
        check("EFFECT_DIALOG reverse says it has no dialog, and none opens",
              d.startswith("ERR") and "no dialog" in d and c.ask("EFFECT_DIALOG") == "OK open=none", d)
        c.ask(f"SAVE {saved}")
        for _ in range(50):
            if c.state().get("busy") == "0" and os.path.exists(saved):
                break
            time.sleep(0.2)
        check("save finished", os.path.exists(saved), c.state())
        for frame, (l, r) in {RATE // 2: (0.125, -0.25), RATE * 3 // 2: (0.125, -0.25), RATE * 5 // 2: (0.25, -0.5)}.items():
            got = sample(saved, frame)
            check(f"saved samples at {frame / RATE:.2f} s", abs(got[0] - l) < 3e-3 and abs(got[1] - r) < 3e-3, f"got {got}, want {(l, r)}")
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
    print("i935 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
