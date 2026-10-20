#!/usr/bin/env python3
"""f26 alpha run: level meters and loudness measurement through the control channel.

    python3 tools/alpha/f26.py --exe <release wavcraft binary> [--port 47126] [--keep]

Generates known audio (16-bit stereo WAV, standard library only) in a scratch folder, starts the app
with HOME, the session folder, the log folder and the token file all inside it, then checks:
- LOUDNESS on a 1 kHz stereo sine at -20 dBFS peak: integrated -20 LUFS +-0.1 (BS.1770: both channels of
  a sine read its peak level in dBFS), sample peak -20, true peak within 0.3, no loudness range;
- LOUDNESS over a selection, over a range too short for a 400 ms block (-inf);
- LOUDNESS on two levels 10 dB apart: range about 10 LU, integrated the power mean of the two;
- a sine at a quarter of the sample rate, 45 degrees in: true peak well above sample peak;
- METERS: a pushed peak shows at once, is held, the bar falls, the clip light latches until RESET.
Playback itself needs an output device, so the meters are fed through `METERS PUSH` (the same tap the
audio callback writes; the callback side is covered by a unit test in player.rs).
Quits through the channel; any ERROR line in the app's log fails the run; the scratch folder is deleted.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE = 48000


def write_wav(path, left, right):
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        out = bytearray()
        for a, b in zip(left, right):
            out += struct.pack("<hh", max(-32768, min(32767, round(a * 32767))), max(-32768, min(32767, round(b * 32767))))
        w.writeframes(bytes(out))


def sine(secs, hz, peak_db, phase=0.0):
    a = 10 ** (peak_db / 20)
    return [a * math.sin(2 * math.pi * hz * n / RATE + phase) for n in range(int(secs * RATE))]


class Control:
    def __init__(self, port, token):
        for _ in range(80):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=120)
                break
            except OSError:
                time.sleep(0.25)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def open(self, path):
        assert self.ask(f"OPEN {path}") == "OK"
        for _ in range(240):
            if "busy=0" in self.ask("STATE") and "frames=0 " not in self.ask("STATE"):
                return
            time.sleep(0.25)
        raise RuntimeError("open timed out")


def fields(answer):
    assert answer.startswith("OK"), answer
    out = {}
    for kv in answer.split()[1:]:
        k, _, v = kv.partition("=")
        out[k] = float("-inf") if v == "-inf" else (float(v) if v.replace(".", "", 1).replace("-", "", 1).isdigit() else v)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47126)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f26.", dir=base)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    token_file = os.path.join(work, "control.token")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + (f"  [{detail}]" if detail else ""), flush=True)

    tone = os.path.join(work, "tone.wav")
    s = sine(20, 1000, -20)
    write_wav(tone, s, s)
    steps = os.path.join(work, "steps.wav")
    s = sine(20, 1000, -30) + sine(20, 1000, -20)
    write_wav(steps, s, s)
    quarter = os.path.join(work, "quarter.wav")
    s = [0.9 * math.sin(math.pi * n / 2 + math.pi / 4) for n in range(RATE * 5)]
    write_wav(quarter, s, s)

    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token_file, RUST_BACKTRACE="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([args.exe], env=env, stdout=err, stderr=err)
    try:
        for _ in range(80):
            if os.path.exists(token_file):
                break
            time.sleep(0.25)
        c = Control(args.port, open(token_file).read().strip())

        c.open(tone)
        m = fields(c.ask("LOUDNESS"))
        check("tone: scope is the whole file", m["scope"] == "file" and m["frames"] == 20 * RATE, m)
        check("tone: integrated -20 LUFS +-0.1", abs(m["integrated"] + 20) < 0.1, m["integrated"])
        check("tone: sample peak -20 dBFS +-0.05", abs(m["sample_peak"] + 20) < 0.05, m["sample_peak"])
        check("tone: true peak within 0.3 dB of -20", abs(m["true_peak"] + 20) < 0.3, m["true_peak"])
        check("tone: no loudness range", m["range"] < 0.2, m["range"])
        c.ask(f"SELECT {RATE} {3 * RATE}")
        m = fields(c.ask("LOUDNESS"))
        check("tone: a selection of 2 s reads -20 LUFS +-0.3", m["scope"] == "selection" and m["frames"] == 2 * RATE and abs(m["integrated"] + 20) < 0.3, m)
        m = fields(c.ask("LOUDNESS 1000 5000"))
        check("tone: a range under 400 ms has no loudness (-inf) but a peak", m["integrated"] == float("-inf") and m["sample_peak"] > -21, m)

        c.open(steps)
        m = fields(c.ask("LOUDNESS"))
        want = 10 * math.log10((10 ** -3 + 10 ** -2) / 2)
        check("steps: loudness range about 10 LU", abs(m["range"] - 10) < 0.5, m["range"])
        check("steps: integrated is the power mean of the two levels", abs(m["integrated"] - want) < 0.15, f"{m['integrated']} vs {want:.2f}")

        c.open(quarter)
        m = fields(c.ask("LOUDNESS"))
        check("fs/4 sine: true peak 2.5+ dB above the sample peak", m["true_peak"] > m["sample_peak"] + 2.5, f"tp {m['true_peak']} sp {m['sample_peak']}")
        check("fs/4 sine: true peak near the sine's peak (-0.92 dBTP; this meter reads up to 0.6 dB high here)", -0.1 < m["true_peak"] - 20 * math.log10(0.9) < 0.6, m["true_peak"])

        m = fields(c.ask("METERS"))
        check("meters: quiet at the start", m["l"] < -100 and m["r"] < -100 and m["l_clip"] == 0, m)
        m = fields(c.ask("METERS PUSH 0.5 0.25"))
        check("meters: a pushed peak shows at once", abs(m["l"] + 6.02) < 0.3 and abs(m["r"] + 12.04) < 0.3, m)
        check("meters: and is held", abs(m["l_hold"] + 6.02) < 0.01 and abs(m["r_hold"] + 12.04) < 0.01 and m["l_clip"] == 0, m)
        m = fields(c.ask("METERS PUSH 1.0 0"))
        check("meters: full scale lights the clip light", m["l_clip"] == 1 and m["r_clip"] == 0 and abs(m["l_hold"]) < 0.01, m)
        time.sleep(0.5)
        m = fields(c.ask("METERS"))
        check("meters: the hold stays while the bar falls (0.5 s later)", abs(m["l_hold"]) < 0.01 and m["l"] < -3, m)
        t0, m = time.time(), {}
        while time.time() - t0 < 12:
            m = fields(c.ask("METERS"))
            if m["l"] < -50 and m["l_hold"] < -3:
                break
            time.sleep(0.5)
        check("meters: bar fell and the hold released after 1.5 s", m["l"] < -50 and m["l_hold"] < -3, m)
        check("meters: the clip light stays lit", m["l_clip"] == 1, m)
        m = fields(c.ask("METERS RESET"))
        check("meters: clicking (RESET) puts the light out", m["l_clip"] == 0, m)

        check("quit", c.ask("QUIT") == "OK")
        proc.wait(20)
    except Exception as e:
        check("script ran to the end", False, f"{type(e).__name__}: {e}")
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()
        err.close()
    log_lines = []
    if os.path.isdir(logs):
        for name in os.listdir(logs):
            log_lines += open(os.path.join(logs, name), errors="replace").read().splitlines()
    errors = [l[:200] for l in log_lines if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_lines) and not errors, errors[:3] or f"{len(log_lines)} lines")
    if errors or not all(ok for _, ok, _ in checks):
        print("--- log tail ---")
        print("\n".join(log_lines[-15:]))
        print(open(os.path.join(work, "stderr.log"), errors="replace").read()[-1500:])
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    failed = [l for l, ok, _ in checks if not ok]
    print(f"RESULT {'PASS' if not failed else 'FAIL'}: {len(checks) - len(failed)}/{len(checks)} checks")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
