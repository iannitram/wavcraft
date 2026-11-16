#!/usr/bin/env python3
"""B4.5 alpha run: the RMS reading from the player's energy, through the control channel.

    python3 tools/alpha/b4-rms.py --exe <release wavcraft binary> [--port 47205] [--keep]

Generates a 10 s 48 kHz stereo 16-bit WAV of a 1 kHz sine of amplitude 0.1 (peak -20 dBFS) in a
scratch folder, starts the app with HOME, the session folder, the log folder and the token file all
inside it and with WAVCRAFT_NO_AUDIO=1 at 48 kHz, opens the tone and plays it. While playing, METERS
reports l and r at -20.0 and l_rms and r_rms at -23.0 (a sine's RMS is 3 dB below its peak); two
seconds after PLAYBACK STOP the RMS readings are back at the -60.0 floor. Quits through the channel;
any ERROR line in the app's log fails the run; the scratch folder is deleted unless --keep.
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


def sine(secs, hz, amplitude):
    return [amplitude * math.sin(2 * math.pi * hz * n / RATE) for n in range(int(secs * RATE))]


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
            words = dict(kv.split("=", 1) for kv in self.ask("STATE").split()[1:] if "=" in kv)
            if words.get("busy") == "0" and int(words.get("frames", "0")) > 0:
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
    ap.add_argument("--port", type=int, default=47205)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="b4-rms.", dir=base)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    token_file = os.path.join(work, "control.token")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + (f"  [{detail}]" if detail else ""), flush=True)

    tone = os.path.join(work, "tone.wav")
    s = sine(10, 1000, 0.1)
    write_wav(tone, s, s)

    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token_file, RUST_BACKTRACE="1",
               WAVCRAFT_NO_AUDIO="1", WAVCRAFT_NO_AUDIO_RATE=str(RATE))
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([args.exe], env=env, stdout=err, stderr=err)
    try:
        for _ in range(80):
            if os.path.exists(token_file):
                break
            time.sleep(0.25)
        c = Control(args.port, open(token_file).read().strip())

        c.open(tone)
        c.ask("SELECT 0 0")
        c.ask("PLAYBACK PLAY")
        time.sleep(1.0)
        m = fields(c.ask("METERS"))
        check("while playing, l and r are -20.0 within 0.1", abs(m["l"] + 20.0) <= 0.1 and abs(m["r"] + 20.0) <= 0.1, m)
        check("while playing, l_rms and r_rms are -23.0 within 0.1", abs(m["l_rms"] + 23.0) <= 0.1 and abs(m["r_rms"] + 23.0) <= 0.1, m)
        c.ask("PLAYBACK STOP")
        time.sleep(2.0)
        m = fields(c.ask("METERS"))
        check("two seconds after STOP, l_rms and r_rms read -60.0", abs(m["l_rms"] + 60.0) <= 0.01 and abs(m["r_rms"] + 60.0) <= 0.01, m)

        c.ask("QUIT")
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
    if errors:
        print("FAIL the app's own log has no ERROR  " + str(errors[:3]), flush=True)
    if errors or not all(ok for _, ok, _ in checks):
        print("--- log tail ---")
        print("\n".join(log_lines[-15:]))
        print(open(os.path.join(work, "stderr.log"), errors="replace").read()[-1500:])
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    passed = sum(ok for _, ok, _ in checks)
    ok = passed == len(checks) and not errors
    print(f"RESULT {'PASS' if ok else 'FAIL'}: {passed}/{len(checks)} checks")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
