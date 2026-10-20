#!/usr/bin/env python3
"""f28 alpha run: Compressor, Hard Limiter and DeEsser through the control channel, in a scratch HOME.

    python3 tools/alpha/f28.py --exe <path to the release wavcraft> [--port 47128]

A 4 s synthetic stereo 16-bit file (both channels alike):
  0-1 s  1 kHz at 0.5 (-6 dBFS)                     -> EFFECT compressor
  1-2 s  220 Hz at 0.6 + 3.1 kHz at 0.35 (peak .95) -> EFFECT hard_limiter
  2-3 s  200 Hz at 0.3 + 8 kHz at 0.3               -> EFFECT deesser (multiband, then broadband on a copy)
  3-4 s  silence, must stay silent
Checked on a saved copy read back (measured): the compressor's steady-state reduction (expected
10.5 dB for -20 dB / 4:1 / hard knee), the limiter's sample peak against its ceiling, the deesser's
8 kHz and 200 Hz levels; the undo step names; bad settings answer ERR; the dialog opens on the
effect's settings; no ERROR line in the app's log.
"""
import argparse, glob, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

RATE = 48000


def make_wav(path):
    frames = bytearray()
    for n in range(4 * RATE):
        t = n / RATE
        s = n // RATE
        if s == 0:
            v = 0.5 * math.sin(2 * math.pi * 1000 * t)
        elif s == 1:
            v = 0.6 * math.sin(2 * math.pi * 220 * t) + 0.35 * math.sin(2 * math.pi * 3100 * t)
        elif s == 2:
            v = 0.3 * math.sin(2 * math.pi * 200 * t) + 0.3 * math.sin(2 * math.pi * 8000 * t)
        else:
            v = 0.0
        q = struct.pack("<h", int(round(v * 32767)))
        frames += q + q
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(bytes(frames))


def read_left(path):
    with wave.open(path, "rb") as w:
        raw = w.readframes(w.getnframes())
    return [struct.unpack_from("<h", raw, 4 * i)[0] / 32768.0 for i in range(len(raw) // 4)]


def level(x, freq):
    re = sum(v * math.cos(2 * math.pi * freq * n / RATE) for n, v in enumerate(x))
    im = sum(v * math.sin(2 * math.pi * freq * n / RATE) for n, v in enumerate(x))
    return 2 * math.hypot(re, im) / len(x)


def db(x):
    return 20 * math.log10(max(x, 1e-12))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47128)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="f28.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    wav, saved, saved2, token = (os.path.join(work, n) for n in ("in.wav", "out.wav", "out2.wav", "control.token"))
    make_wav(wav)
    src = read_left(wav)
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

        def save_to(path):
            c.ask(f"SAVE {path}")
            time.sleep(0.5)  # let the save start: busy reads 0 before it does
            last = -1
            for _ in range(150):
                size = os.path.getsize(path) if os.path.exists(path) else -2
                if c.state().get("busy") == "0" and size == last:
                    return
                last = size
                time.sleep(0.4)

        st = c.state()
        check("the file opens", st["frames"] == str(4 * RATE) and st["steps"] == "1", st)
        listing = c.ask("EFFECT")
        check("EFFECT lists the three with their settings", all(k in listing for k in ("compressor [threshold_db=", "hard_limiter [max_amplitude_db=", "deesser [frequency_hz=")), listing)
        steps = lambda: int(c.state()["steps"])
        n0 = steps()
        bad = [c.ask("EFFECT compressor ratio=0.5"), c.ask("EFFECT hard_limiter max_amplitude_db=3"), c.ask("EFFECT deesser mode=sideways")]
        check("bad settings answer ERR and add no step", all(b.startswith("ERR") for b in bad) and steps() == n0, bad)
        o = c.ask("EFFECT_DIALOG hard_limiter max_amplitude_db=-6")
        d = c.ask("EFFECT_DIALOG")
        c.ask("EFFECT_DIALOG CANCEL")
        check("the Hard Limiter dialog opens on its settings", o == "OK" and "open=hard_limiter" in d and "-6" in d, f"{o}; {d}")

        c.ask(f"SELECT 0 {RATE}")
        a = c.ask("EFFECT compressor threshold_db=-20 ratio=4 attack_ms=1 release_ms=50 knee_db=0")
        check("Compressor: step named Compressor", a.endswith("changed=1") and c.state()["undo"] == "Compressor", f"{a}; {c.state()}")
        c.ask(f"SELECT {RATE} {2 * RATE}")
        a = c.ask("EFFECT hard_limiter max_amplitude_db=-6 lookahead_ms=3 release_ms=50")
        check("Hard Limiter: step named Hard Limiter", a.endswith("changed=1") and c.state()["undo"] == "Hard Limiter", f"{a}; {c.state()}")
        c.ask(f"SELECT {2 * RATE} {3 * RATE}")
        a = c.ask("EFFECT deesser frequency_hz=6000 threshold_db=-30 amount_db=12 mode=multiband")
        check("DeEsser: step named DeEsser", a.endswith("changed=1") and c.state()["undo"] == "DeEsser", f"{a}; {c.state()}")
        save_to(saved)
        out = read_left(saved)
        check("saved file has the same length", len(out) == len(src), (len(out), len(src)))

        # Interior only: the edge crossfade (5 ms) blends the original back in at both ends of a range.
        tail0 = slice(RATE // 2, RATE - 480)
        red = db(max(abs(v) for v in src[tail0])) - db(max(abs(v) for v in out[tail0]))
        check("Compressor reduced the 1 kHz tone by about 10.5 dB at steady state (measured)", abs(red - 10.5) < 1.5, f"{red:.2f} dB")
        ceiling = 10 ** (-6 / 20)
        pk = max(abs(v) for v in out[RATE + 480:2 * RATE - 480])
        check("Hard Limiter sample peak is at or under -6 dBFS and the signal is not squashed (measured)", ceiling * 0.9 < pk <= ceiling + 1.5 / 32768, f"peak {pk:.4f}, ceiling {ceiling:.4f}")
        seg_in, seg_out = src[2 * RATE + RATE // 2:3 * RATE - 480], out[2 * RATE + RATE // 2:3 * RATE - 480]
        hi = db(level(seg_in, 8000)) - db(level(seg_out, 8000))
        lo = db(level(seg_in, 200)) - db(level(seg_out, 200))
        check("DeEsser multiband cut the 8 kHz band by over 6 dB (measured)", hi > 6, f"{hi:.1f} dB")
        check("DeEsser multiband left 200 Hz within 1 dB (measured)", abs(lo) < 1, f"{lo:.2f} dB")
        check("the silent last second stays silent", max(abs(v) for v in out[3 * RATE:]) < 2 / 32768, max(abs(v) for v in out[3 * RATE:]))

        # Broadband mode: undo the DeEsser, run it again broadband.
        c.ask(f"HISTORY {steps() - 2}")
        c.ask(f"SELECT {2 * RATE} {3 * RATE}")
        a = c.ask("EFFECT deesser frequency_hz=6000 threshold_db=-30 amount_db=12 mode=broadband")
        save_to(saved2)
        out2 = read_left(saved2)
        seg = out2[2 * RATE + RATE // 2:3 * RATE - 480]
        hi2 = db(level(seg_in, 8000)) - db(level(seg, 8000))
        lo2 = db(level(seg_in, 200)) - db(level(seg, 200))
        check("DeEsser broadband turns both bands down (measured)", a.endswith("changed=1") and hi2 > 6 and lo2 > 6, f"{a}: 8 kHz {hi2:.1f} dB, 200 Hz {lo2:.1f} dB")
        c.ask("QUIT discard")
        proc.wait(timeout=30)
    finally:
        if proc.poll() is None:
            proc.kill()
    errors = [l.strip()[:200] for p in glob.glob(os.path.join(logs, "*")) if os.path.isfile(p) for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR line", not errors, errors[:3])
    ok = all(o for _, o, _ in checks)
    for label, o, detail in checks:
        print(("PASS " if o else "FAIL ") + label + (f"  [{detail}]" if not o or "measured" in label else ""))
    if ok and not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    else:
        print("kept", work)
    print("f28 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


main()
