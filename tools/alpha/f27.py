#!/usr/bin/env python3
"""f27 alpha run: Parametric EQ, High-Pass, Low-Pass, Notch and DC Offset removal, in a scratch HOME.

    python3 tools/alpha/f27.py --exe <path to the release wavcraft> [--port 47127]

A synthetic mono file of five 1 s segments (0.5 amplitude sines) is opened:
  0: 1 kHz   -> EFFECT notch freq=1000 q=10 depth_db=40       (RMS must fall by about 40 dB)
  1: 100 Hz  -> EFFECT high_pass cutoff=1000 slope=24         (RMS must fall by over 40 dB)
  2: 8 kHz   -> EFFECT low_pass cutoff=1000 slope=24          (RMS must fall by over 40 dB)
  3: 1 kHz + 0.25 DC -> EFFECT dc_offset                      (mean must go to about 0)
  4: 1 kHz   -> EFFECT parametric_eq mid1_freq=1000 mid1_gain_db=6 mid1_q=1 (RMS must rise by 6 dB)
Each effect is applied to its segment's selection with `EFFECT <name> key=value`; the file is SAVEd
and RMS/mean are measured on the middle half of each segment. Bad settings must answer ERR. The
app's log must hold no ERROR line.
"""
import argparse, glob, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

RATE = 48000


def make_wav(path):
    frames = []
    for seg, (freq, dc) in enumerate([(1000, 0), (100, 0), (8000, 0), (1000, 0.25), (1000, 0)]):
        amp = 0.5 if dc == 0 else 0.25
        for k in range(RATE):
            v = amp * math.sin(2 * math.pi * freq * k / RATE) + dc
            frames.append(struct.pack("<h", int(max(-1, min(1, v)) * 32767)))
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(frames))


def read_wav(path):
    with wave.open(path, "rb") as w:
        raw = w.readframes(w.getnframes())
    return [s / 32768.0 for s in struct.unpack("<%dh" % (len(raw) // 2), raw)]


def middle(samples, seg):
    return samples[seg * RATE + RATE // 4: seg * RATE + 3 * RATE // 4]


def rms(v):
    return math.sqrt(sum(x * x for x in v) / len(v))


def mean(v):
    return sum(v) / len(v)


def db(a, b):
    return 20 * math.log10(max(a, 1e-9) / max(b, 1e-9))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47127)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="f27.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    wav, saved, token = os.path.join(work, "in.wav"), os.path.join(work, "out.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    original = read_wav(wav)
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
        check("the file opens", st["frames"] == str(5 * RATE) and st["steps"] == "1", st)
        listing = c.ask("EFFECT")
        for name in ("parametric_eq", "high_pass", "low_pass", "notch", "dc_offset"):
            check(f"EFFECT lists {name}", name in listing, listing)
        bad = [c.ask("EFFECT notch depth_db=90"), c.ask("EFFECT high_pass slope=36"), c.ask("EFFECT low_pass cutoff=3")]
        check("bad settings answer ERR and add no step", all(b.startswith("ERR") for b in bad) and c.state()["steps"] == "1", bad)
        jobs = [
            (0, "EFFECT notch freq=1000 q=10 depth_db=40", "Notch Filter"),
            (1, "EFFECT high_pass cutoff=1000 slope=24", "High-Pass Filter"),
            (2, "EFFECT low_pass cutoff=1000 slope=24", "Low-Pass Filter"),
            (3, "EFFECT dc_offset", "Remove DC Offset"),
            (4, "EFFECT parametric_eq mid1_freq=1000 mid1_gain_db=6 mid1_q=1", "Parametric EQ"),
        ]
        for seg, line, label in jobs:
            c.ask(f"SELECT {seg * RATE} {(seg + 1) * RATE}")
            a = c.ask(line)
            st = c.state()
            check(f"{line}: step named {label}", a.endswith("changed=1") and st["undo"] == label, f"{a}; {st}")
        c.ask(f"SAVE {saved}")
        st = c.state()
        check("save finished", os.path.exists(saved) and st.get("busy") == "0", st)
        out = read_wav(saved)
        check("length kept", len(out) == len(original), (len(out), len(original)))
        d = db(rms(middle(out, 0)), rms(middle(original, 0)))
        check("notch: 1 kHz down about 40 dB", -45 < d < -34, f"{d:.1f} dB (measured)")
        d = db(rms(middle(out, 1)), rms(middle(original, 1)))
        check("high-pass 1 kHz 24 dB/oct: 100 Hz down over 40 dB", d < -40, f"{d:.1f} dB (measured)")
        d = db(rms(middle(out, 2)), rms(middle(original, 2)))
        check("low-pass 1 kHz 24 dB/oct: 8 kHz down over 40 dB", d < -40, f"{d:.1f} dB (measured)")
        m0, m1 = mean(middle(original, 3)), mean(middle(out, 3))
        check("DC removed: mean 0.25 -> about 0", abs(m0 - 0.25) < 0.01 and abs(m1) < 0.005, f"before {m0:.4f}, after {m1:.4f} (measured)")
        d = db(rms(middle(out, 4)), rms(middle(original, 4)))
        check("parametric EQ bell +6 dB at 1 kHz", 5.7 < d < 6.3, f"{d:.2f} dB (measured)")
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
    print("f27 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


main()
