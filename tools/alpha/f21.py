#!/usr/bin/env python3
"""f21 alpha run: Generate Tones and Generate Noise (A303, A304, A308), in a scratch HOME.

    python3 tools/alpha/f21.py --exe <path to the release wavcraft> [--port 47121]

A 1 s synthetic stereo file of silence is opened. Checked, via STATE and a saved copy read back:
  1. GENERATE tone freq=440 dur=2 amp=-6 at the cursor: 3 s long, one step "Generate Tones";
  2. GENERATE noise color=pink dur=1 amp=-12 over a selection of 0.5 s: length 3.5 s, one step;
  3. bad requests answer ERR and add no step;
  4. the dialog: opens on given values, PREVIEW answers its length, Cancel adds no step and ends
     the preview, OK inserts a 1000 Hz tone at the cursor;
  5. after SAVE the frames, the measured frequency (rising zero crossings) and the measured peak
     and RMS level of each generated part are as asked, and the untouched silence is still silent;
  6. the app's log holds no ERROR line.
"""
import argparse, glob, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

RATE = 48000


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"\0\0\0\0" * RATE)


def read_channel(path, ch):
    """(frames, channels, samples of channel `ch` as -1..1) of a PCM WAV."""
    with wave.open(path, "rb") as w:
        n, k, width = w.getnframes(), w.getnchannels(), w.getsampwidth()
        raw = w.readframes(n)
    scale = float(1 << (8 * width - 1))
    out = []
    for i in range(n):
        o = (i * k + ch) * width
        out.append(int.from_bytes(raw[o:o + width], "little", signed=True) / scale)
    return n, k, out


def measure(x):
    """(rising zero crossings, peak, rms) of a list of samples."""
    rises = sum(1 for a, b in zip(x, x[1:]) if a < 0 <= b)
    peak = max(abs(v) for v in x)
    rms = math.sqrt(sum(v * v for v in x) / len(x))
    return rises, peak, rms


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47121)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="f21.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
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
        check("the file opens", st["frames"] == str(RATE) and st["steps"] == "1", st)
        listing = c.ask("GENERATE")
        check("GENERATE lists both generators", "tone [" in listing and "noise [" in listing, listing)

        c.ask("SELECT 0 0")
        a = c.ask("GENERATE tone freq=440 dur=2 amp=-6 wave=sine")
        st = c.state()
        check("tone at the cursor: 3 s, one step", a.endswith("changed=1") and st["frames"] == str(3 * RATE) and st["undo"] == "Generate Tones" and st["steps"] == "2", f"{a}; {st}")

        c.ask(f"SELECT {2 * RATE} {2 * RATE + RATE // 2}")
        a = c.ask("GENERATE noise color=pink dur=1 amp=-12 seed=3")
        st = c.state()
        check("noise over a 0.5 s selection: 3.5 s, one step", a.endswith("changed=1") and st["frames"] == str(7 * RATE // 2) and st["undo"] == "Generate Noise" and st["steps"] == "3", f"{a}; {st}")

        n = steps()
        bad = [c.ask("GENERATE tone freq=5"), c.ask("GENERATE tone wave=blob"), c.ask("GENERATE noise color=blue"), c.ask("GENERATE nonesuch"), c.ask("GENERATE tone seed=x")]
        check("bad requests answer ERR and add no step", all(b.startswith("ERR") for b in bad) and steps() == n, bad)

        c.ask(f"SELECT {7 * RATE // 2} {7 * RATE // 2}")
        o = c.ask("GENERATE_DIALOG tone freq=1000 dur=0.25 amp=-6")
        d = c.ask("GENERATE_DIALOG")
        check("the dialog opens on the given values", o == "OK" and "open=tone" in d and "1000" in d, f"{o}; {d}")
        p = c.ask("GENERATE_DIALOG PREVIEW")
        d = c.ask("GENERATE_DIALOG")
        check("Preview answers its length and is running", p == f"OK preview={RATE // 4}" and "previewing=1" in d, f"{p}; {d}")
        c.ask("GENERATE_DIALOG CANCEL")
        check("Cancel adds no step, closes the dialog and ends the preview", steps() == n and c.ask("GENERATE_DIALOG") == "OK open=none", c.state())
        c.ask("GENERATE_DIALOG tone freq=1000 dur=0.25 amp=-6")
        c.ask("GENERATE_DIALOG PREVIEW")
        a = c.ask("GENERATE_DIALOG OK")
        st = c.state()
        check("OK inserts the dialog's tone at the cursor", a.endswith("changed=1") and st["frames"] == str(7 * RATE // 2 + RATE // 4) and steps() == n + 1, f"{a}; {st}")

        c.ask(f"SAVE {saved}")
        st = c.state()
        check("save finished", os.path.exists(saved) and st.get("busy") == "0", st)
        frames, chans, left = read_channel(saved, 0)
        _, _, right = read_channel(saved, 1)
        check("saved file: frames and channels", frames == 7 * RATE // 2 + RATE // 4 and chans == 2, (frames, chans))
        rises, peak, rms = measure(left[RATE // 10:2 * RATE - RATE // 10])
        secs = (2 * RATE - RATE // 5) / RATE
        check("tone: measured 440 Hz", abs(rises / secs - 440) < 2, f"{rises / secs:.2f} Hz (measured, zero crossings)")
        check("tone: measured peak -6 dBFS, RMS -9 dBFS", abs(20 * math.log10(peak) + 6) < 0.1 and abs(20 * math.log10(rms) + 9.01) < 0.1, f"peak {20 * math.log10(peak):.2f} dB, rms {20 * math.log10(rms):.2f} dB")
        check("tone: right channel carries it too", max(abs(v) for v in right[RATE // 2:RATE]) > 0.49, max(abs(v) for v in right[RATE // 2:RATE]))
        _, npeak, nrms = measure(left[2 * RATE:3 * RATE])
        check("noise: measured peak at most -12 dBFS and audible", npeak <= 10 ** (-12 / 20) + 1e-3 and nrms > 0.01, f"peak {npeak:.4f}, rms {nrms:.4f}")
        check("noise differs between channels", left[2 * RATE:2 * RATE + 100] != right[2 * RATE:2 * RATE + 100], "")
        check("untouched silence after the noise is silent", max(abs(v) for v in left[3 * RATE + 100:7 * RATE // 2 - 100]) == 0, "")
        t0 = 7 * RATE // 2
        rises, peak, _ = measure(left[t0 + 100:t0 + RATE // 4])
        secs = (RATE // 4 - 100) / RATE
        check("dialog tone: measured 1000 Hz at -6 dBFS", abs(rises / secs - 1000) < 12 and abs(20 * math.log10(peak) + 6) < 0.2, f"{rises / secs:.1f} Hz, {20 * math.log10(peak):.2f} dB")
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
    print("f21 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


main()
