#!/usr/bin/env python3
"""f29 alpha run: Capture Noise Print and Noise Reduction, in a scratch HOME.

    python3 tools/alpha/f29.py --exe <path to the release wavcraft> [--port 47129]

A 12 s synthetic stereo file is generated (deterministic): white Gaussian noise (sigma 0.02) the
whole way, plus a 1 kHz tone (amplitude 0.25) from 5 s. Checked, via the control channel and a
saved copy read back:
  1. a too-short selection answers ERR and leaves no print; NOISE_PRINT says none;
  2. ACTION nr.capture_print on 0 to 2 s (noise only) answers OK; NOISE_PRINT reports both channels;
  3. EFFECT noise_reduction reduce_db=12 on the whole file adds one "Noise Reduction" step;
  4. in the saved file the RMS in the noise-only region (0.25 s to 4.75 s) fell by at least 12 dB
     (and not by wildly more), and the tone's amplitude (6 s to 11 s) moved by less than 1 dB;
  5. the app's log holds no ERROR line.
"""
import argparse, glob, math, os, random, shutil, struct, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

RATE = 48000
SECS = 12
SIGMA = 0.02
TONE_FROM = 5 * RATE


def make_wav(path):
    rng = random.Random(29)
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        out = bytearray()
        for i in range(SECS * RATE):
            tone = 0.25 * math.sin(2 * math.pi * 1000 * i / RATE) if i >= TONE_FROM else 0.0
            l = rng.gauss(0, SIGMA) + tone
            r = rng.gauss(0, SIGMA) + tone
            out += struct.pack("<hh", max(-32768, min(32767, round(l * 32768))), max(-32768, min(32767, round(r * 32768))))
        w.writeframes(bytes(out))


def read_channel(path, ch, a, b):
    with wave.open(path, "rb") as w:
        assert w.getsampwidth() in (2, 3, 4) and w.getnchannels() == 2, (w.getsampwidth(), w.getnchannels())
        width = w.getsampwidth()
        w.setpos(a)
        raw = w.readframes(b - a)
    scale = float(1 << (8 * width - 1))
    step = 2 * width
    return [int.from_bytes(raw[i + ch * width:i + (ch + 1) * width], "little", signed=True) / scale for i in range(0, len(raw), step)]


def rms(x):
    return math.sqrt(sum(v * v for v in x) / len(x))


def tone_amp(x, hz=1000.0):
    re = sum(v * math.cos(2 * math.pi * hz * i / RATE) for i, v in enumerate(x))
    im = sum(v * math.sin(2 * math.pi * hz * i / RATE) for i, v in enumerate(x))
    return 2 * math.hypot(re, im) / len(x)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47129)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="f29.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
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
        check("the file opens", st["frames"] == str(n) and st["steps"] == "1", st)
        c.ask("SELECT 100 1000")
        a = c.ask("ACTION nr.capture_print")
        p = c.ask("NOISE_PRINT")
        check("a too-short selection is refused and leaves no print", a.startswith("ERR") and "too short" in a and p == "OK none", f"{a}; {p}")
        a = c.ask("EFFECT noise_reduction reduce_db=12")
        check("Noise Reduction without a print says to capture one", a.startswith("ERR") and "noise print" in a and c.state()["steps"] == "1", a)
        c.ask(f"SELECT 0 {2 * RATE}")
        a = c.ask("ACTION nr.capture_print")
        p = c.ask("NOISE_PRINT")
        check("capturing from 0 to 2 s works", a.endswith("changed=0") and a.startswith("OK") and c.state()["steps"] == "1", a)
        check("the print has both channels at 48 kHz", p.startswith("OK") and "channels=2" in p and f"rate={RATE}" in p, p)
        c.ask("SELECT 0 0")
        t0 = time.time()
        a = c.ask("EFFECT noise_reduction reduce_db=12")
        took = time.time() - t0
        st = c.state()
        check("EFFECT noise_reduction adds one named step", a.endswith("changed=1") and st["undo"] == "Noise Reduction" and st["steps"] == "2", f"{a}; {st}")
        print(f"(noise reduction of {SECS} s stereo took {took:.1f} s)")
        c.ask(f"SAVE {saved}")
        for _ in range(100):
            if c.state().get("busy") == "0" and os.path.exists(saved):
                break
            time.sleep(0.3)
        check("save finished", os.path.exists(saved) and c.state().get("busy") == "0", c.state())
        nz = (RATE // 4, 19 * RATE // 4)
        tn = (6 * RATE, 11 * RATE)
        for ch, name in ((0, "left"), (1, "right")):
            before, after = read_channel(wav, ch, *nz), read_channel(saved, ch, *nz)
            drop = 20 * math.log10(rms(before) / rms(after))
            check(f"{name}: the noise floor fell by at least 12 dB (and not by wildly more)", 12.0 <= drop <= 15.0, f"{drop:.2f} dB")
            tb, ta = tone_amp(read_channel(wav, ch, *tn)), tone_amp(read_channel(saved, ch, *tn))
            change = 20 * math.log10(ta / tb)
            check(f"{name}: the tone moved by less than 1 dB", abs(change) < 1.0, f"{change:.3f} dB (before {tb:.4f}, after {ta:.4f})")
        c.ask("QUIT discard")
        proc.wait(timeout=30)
    finally:
        if proc.poll() is None:
            proc.kill()
    errors = [l.strip()[:200] for p in glob.glob(os.path.join(logs, "*")) if os.path.isfile(p) for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR line", not errors, errors[:3])
    ok = all(o for _, o, _ in checks)
    for label, o, detail in checks:
        print(("PASS " if o else "FAIL ") + label + f"  [{detail}]")
    if ok and not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    else:
        print("kept", work)
    print("f29 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


main()
