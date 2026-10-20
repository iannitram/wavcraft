#!/usr/bin/env python3
"""f40 scripted run: dither and noise shaping on bit-depth reduction, through the control channel.

    python3 tools/alpha/f40.py --exe <path to the release wavcraft> [--port 47140] [--keep]

A synthetic 1 s mono 48 kHz 32-bit float WAV holding a 1 kHz sine at -100 dBFS (0.33 LSB at 16 bits) is
opened in a scratch HOME. Checked on the saved 16-bit files:
  CONVERT bits=16            -> all samples 0 (plain rounding loses the tone; dither is off by default)
  Undo, CONVERT bits=16 dither=tpdf -> the 1 kHz tone is >= 20 dB above the noise bins around it
  Undo, CONVERT bits=16 dither=tpdf shaping=light and =strong -> the error (saved minus source) has less
  energy at 1-5 kHz and more at 18-23 kHz than flat TPDF, strong more than light
  a bad dither setting answers ERR; QUIT; no ERROR line in the app's log.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, SECONDS = 48000, 1


class Control:
    def __init__(self, port, token, tries=120):
        for _ in range(tries):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=120)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("the control channel refused the token")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self, timeout=120):
        t0 = time.time()
        while True:
            head, _, labels = self.ask("STATE").partition(" undo=")
            fields = dict(kv.split("=", 1) for kv in head.split()[1:])
            undo, _, redo = labels.partition(" redo=")
            fields.update(undo=undo, redo=redo)
            if fields.get("busy") == "0" or time.time() - t0 > timeout:
                return fields
            time.sleep(0.1)




def write_source(path):
    n = RATE * SECONDS
    with open(path, "wb") as f:
        data = struct.pack("<%df" % n, *[1e-5 * math.sin(2 * math.pi * 1000 * i / RATE) for i in range(n)])
        f.write(b"RIFF" + struct.pack("<I", 4 + 26 + 12 + 8 + len(data)) + b"WAVE")
        f.write(b"fmt " + struct.pack("<IHHIIHHH", 18, 3, 1, RATE, RATE * 4, 4, 32, 0))
        f.write(b"fact" + struct.pack("<II", 4, n))
        f.write(b"data" + struct.pack("<I", len(data)) + data)


def read16(path):
    with wave.open(path, "rb") as w:
        assert (w.getnchannels(), w.getsampwidth(), w.getframerate()) == (1, 2, RATE)
        raw = w.readframes(w.getnframes())
    return struct.unpack("<%dh" % (len(raw) // 2), raw)


def goertzel_power(x, hz):
    """Power of the DFT of x at hz (a whole-number bin when len(x) = RATE)."""
    w = 2 * math.pi * hz / RATE
    c = 2 * math.cos(w)
    s1 = s2 = 0.0
    for v in x:
        s1, s2 = v + c * s1 - s2, s1
    return s1 * s1 + s2 * s2 - c * s1 * s2


def band_power(x, lo, hi, step=250):
    return sum(goertzel_power(x, f) for f in range(lo, hi, step)) / len(range(lo, hi, step))


def db(r):
    return 10 * math.log10(max(r, 1e-30))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47140)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f40.", dir=scratch_root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    src = os.path.join(work, "source.wav")
    write_source(src)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(a.port), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([a.exe, src], env=env, stdout=err, stderr=err)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + (f"  [{detail}]" if not ok or detail else ""), flush=True)

    n = RATE * SECONDS
    for _ in range(240):
        if os.path.exists(token) and open(token).read().strip():
            break
        time.sleep(0.25)
    c = Control(a.port, open(token).read().strip())
    scale = 32768.0
    ideal = [1e-5 * math.sin(2 * math.pi * 1000 * i / RATE) * scale for i in range(n)]

    def convert_and_save(settings, name):
        ans = c.ask("CONVERT " + settings)
        st = c.state()
        out = os.path.join(work, name)
        ans2 = c.ask(f"SAVE {out}")
        st = c.state()
        c.ask("ACTION edit.undo")
        c.state()
        return ans, ans2, read16(out)

    try:
        st = c.state()
        check("the source opens: 1 s, mono, 48 kHz", (st["frames"], st["channels"], st["rate"]) == (str(n), "1", str(RATE)), st)

        ans, ans2, off = convert_and_save("bits=16", "off.wav")
        check("CONVERT bits=16 (dither off) saves exact rounding: all zeros for a -100 dBFS tone",
              ans == "OK" and ans2 == "OK" and len(off) == n and not any(off), f"{ans} {ans2} nonzero={sum(1 for v in off if v)}")

        ans, ans2, tpdf = convert_and_save("bits=16 dither=tpdf", "tpdf.wav")
        tone = goertzel_power(tpdf, 1000)
        floor = band_power(tpdf, 2000, 5000, 100) * 1.0
        margin = db(tone / floor)
        check("TPDF dither keeps the 1 kHz tone visible above the noise", ans == "OK" and ans2 == "OK" and margin > 20, f"{margin:.1f} dB")

        err_of = lambda q: [v - i for v, i in zip(q, ideal)]
        flat = err_of(tpdf)
        results = {}
        for name in ("light", "strong"):
            ans, ans2, q = convert_and_save(f"bits=16 dither=tpdf shaping={name}", f"{name}.wav")
            e = err_of(q)
            lo, hi = db(band_power(e, 1000, 5000) / band_power(flat, 1000, 5000)), db(band_power(e, 18000, 23000) / band_power(flat, 18000, 23000))
            results[name] = (lo, hi)
            check(f"shaping={name}: less noise at 1-5 kHz and more at 18-23 kHz than flat TPDF",
                  ans == "OK" and ans2 == "OK" and lo < -3 and hi > 2, f"low {lo:+.1f} dB, high {hi:+.1f} dB")
        check("strong shaping moves more noise than light", results["strong"][0] < results["light"][0] and results["strong"][1] > results["light"][1], results)

        bad = c.ask("CONVERT bits=16 dither=gaussian")
        check("CONVERT dither=gaussian answers ERR", bad.startswith("ERR"), bad)
        check("QUIT discard closes the app", c.ask("QUIT discard") == "OK")
        proc.wait(30)
    except Exception as e:
        check("the run finished without an exception", False, f"{type(e).__name__}: {e}")
        proc.kill()
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's own log has no ERROR line", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    ok = all(x[1] for x in checks)
    print("RESULT", "pass" if ok else "fail", f"{sum(x[1] for x in checks)}/{len(checks)} checks")
    if a.keep or not ok:
        print("scratch kept:", work)
    else:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
