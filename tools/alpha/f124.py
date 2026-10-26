#!/usr/bin/env python3
"""f124 alpha run: Reverb and Delay through the control channel, in a scratch HOME.

    python3 tools/alpha/f124.py --exe <path to the release wavcraft> [--port 47224]

A 4 s synthetic mono file holding one impulse (1.0 at 0.5 s) is opened. Checked via STATE and a
saved copy read back: EFFECT lists Reverb and Delay; Delay (200 ms, feedback 50 %, wet 50 %) on
the whole file puts echoes at +200 ms and +400 ms at 0.5 and 0.25; Reverb (decay 1 s, wet 100 %)
on 0 to 3 s rings after the impulse and nothing is written past 3 s; bad settings answer ERR;
the app's log holds no ERROR line.
"""
import argparse, glob, os, shutil, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

RATE = 48000


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        data = bytearray(4 * RATE * 2)
        data[int(0.5 * RATE) * 2:int(0.5 * RATE) * 2 + 2] = (32767).to_bytes(2, "little", signed=True)
        w.writeframes(bytes(data))


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
    ap.add_argument("--port", type=int, default=47224)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="f124.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    wav, saved, saved2, token = os.path.join(work, "in.wav"), os.path.join(work, "out.wav"), os.path.join(work, "out2.wav"), os.path.join(work, "control.token")
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
        n = 4 * RATE
        st = c.state()
        check("the file opens", st["frames"] == str(n) and st["steps"] == "1", st)
        listing = c.ask("EFFECT")
        check("EFFECT lists Reverb and Delay", "reverb [" in listing and "delay [" in listing, listing)
        bad = [c.ask("EFFECT delay time=99999"), c.ask("EFFECT reverb decay=0"), c.ask("EFFECT delay feedback=120")]
        check("bad settings answer ERR and add no step", all(b.startswith("ERR") for b in bad) and c.state()["steps"] == "1", bad)
        a = c.ask("EFFECT delay time=200ms feedback=50% wet=50%")
        st = c.state()
        check("Delay runs on the whole file as one step", a.endswith("changed=1") and st["undo"] == "Delay" and st["steps"] == "2", f"{a}; {st}")
        c.ask(f"SELECT 0 {3 * RATE}")
        a = c.ask("EFFECT reverb decay=1s wet=100%")
        st = c.state()
        check("Reverb runs on the selection as one step", a.endswith("changed=1") and st["undo"] == "Reverb" and st["steps"] == "3", f"{a}; {st}")
        c.ask(f"SAVE {saved}")
        st = c.state()
        check("save finished", os.path.exists(saved) and st.get("busy") == "0", st)
        # The reverb ran over the delay's output, so check the tail of the reverb, then undo it for the echoes.
        rev_tail = max(abs(sample(saved, f)[0]) for f in range(int(1.0 * RATE), int(1.5 * RATE), 7))
        check("Reverb rings after the impulse", rev_tail > 1e-3, rev_tail)
        check("Reverb writes nothing past the selection's end (Delay's echoes there are untouched)",
              abs(sample(saved, 3 * RATE + 100)[0]) < 1e-4, sample(saved, 3 * RATE + 100))
        c.ask("ACTION edit.undo")
        st = c.state()
        check("undo removes the Reverb step", st["undo"] == "Delay" and st["redo"] == "Reverb", st)
        c.ask(f"SAVE {saved2}")
        st = c.state()
        check("save finished", os.path.exists(saved2) and st.get("busy") == "0", st)
        at = RATE // 2
        def area(f):
            return sum(sample(saved2, f + k)[0] for k in range(-3, 5))
        e0, e1, e2 = area(at), area(at + RATE // 5), area(at + 2 * RATE // 5)
        check("Delay: dry 0.5, echoes at +200 ms 0.5 and +400 ms 0.25",
              abs(e0 - 0.5) < 0.03 and abs(e1 - 0.5) < 0.03 and abs(e2 - 0.25) < 0.03, (e0, e1, e2))
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
    print("f124 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


main()
