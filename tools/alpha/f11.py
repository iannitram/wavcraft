#!/usr/bin/env python3
"""f11 scripted run: Convert Sample Type, Interpret Sample Rate and presets, through the control channel.

    python3 tools/alpha/f11.py --exe <path to the release wavcraft> [--port 47111] [--keep]

A synthetic 2 s stereo 44.1 kHz 16-bit WAV (two sines) is opened in a scratch HOME. Checked through
STATE (frames, rate, channels, steps, undo) and, for the saved files, the WAV header:
CONVERT rate=22050 channels=1 bits=16 (one step; save is 22050 Hz mono 16-bit with the right length and
a tone at the right frequency), Undo, INTERPRET 48000 (same frames, new rate, one step), a preset
(by name), saving and deleting a user preset, CONVERT mono to stereo, a bad setting, QUIT. No ERROR in the app's log.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, SECONDS = 44100, 2


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
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
        data = bytearray()
        for i in range(n):
            l = int(12000 * math.sin(2 * math.pi * 440 * i / RATE))
            r = int(12000 * math.sin(2 * math.pi * 440 * i / RATE))
            data += struct.pack("<hh", l, r)
        w.writeframes(bytes(data))


def zero_crossings(path):
    with wave.open(path, "rb") as w:
        raw = w.readframes(w.getnframes())
        ch, rate = w.getnchannels(), w.getframerate()
    first = struct.unpack("<" + "h" * (len(raw) // 2), raw)[::ch]
    crossings = sum(1 for a, b in zip(first, first[1:]) if a < 0 <= b)
    return crossings * rate / len(first)  # cycles per second


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47111)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f11.", dir=scratch_root)
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
    try:
        st = c.state()
        check("the source opens: 2 s, stereo, 44.1 kHz", (st["frames"], st["channels"], st["rate"]) == (str(n), "2", str(RATE)), st)

        ans = c.ask("CONVERT rate=22050 channels=1 bits=16 quality=high")
        st = c.state()
        want = n // 2
        check("CONVERT rate=22050 channels=1 bits=16 is one step with the new frames, rate and channels",
              ans == "OK" and (st["frames"], st["channels"], st["rate"], st["steps"], st["undo"]) == (str(want), "1", "22050", "2", "Convert Sample Type"), f"{ans}; {st}")
        out = os.path.join(work, "converted.wav")
        c.ask(f"SAVE {out}")
        st = c.state()
        with wave.open(out, "rb") as w:
            hdr = (w.getnchannels(), w.getframerate(), w.getsampwidth(), w.getnframes())
        check("the saved file is mono, 22050 Hz, 16-bit, the converted length", hdr == (1, 22050, 2, want), hdr)
        hz = zero_crossings(out)
        check("the tone stays at 440 Hz after the rate change", abs(hz - 440) < 4, f"{hz:.1f} Hz")

        c.ask("ACTION edit.undo")
        st = c.state()
        check("Undo restores frames, rate and channels in one step", (st["frames"], st["channels"], st["rate"]) == (str(n), "2", str(RATE)) and st["redo"] == "Convert Sample Type", st)

        ans = c.ask("INTERPRET 48000")
        st = c.state()
        check("INTERPRET 48000 keeps the frames and changes the rate (one step)",
              ans == "OK" and (st["frames"], st["rate"], st["steps"], st["undo"]) == (str(n), "48000", "2", "Interpret Sample Rate"), f"{ans}; {st}")

        listing = c.ask("PRESET list")
        check("PRESET list shows the built-in presets", listing.startswith("OK ") and "Voice Mono 22.05 kHz 16-bit" in listing, listing[:200])
        ans = c.ask("CONVERT preset=Voice Mono 22.05 kHz 16-bit")
        st = c.state()
        want = (n * 22050 + 24000) // 48000
        check("CONVERT preset=<name> applies it (48 kHz label to 22.05 kHz mono)", ans == "OK" and (st["frames"], st["rate"], st["channels"]) == (str(want), "22050", "1"), f"{ans}; {st}")

        check("PRESET save stores a named setting", c.ask("PRESET save My phone memo | rate=16000 channels=1 bits=16 quality=normal") == "OK")
        check("the saved preset is listed", "My phone memo | rate=16000 channels=1 bits=16 quality=normal" in c.ask("PRESET list"))
        check("PRESET save refuses a built-in name", c.ask("PRESET save Voice Mono 22.05 kHz 16-bit | rate=8000").startswith("ERR"))
        check("PRESET delete removes it", c.ask("PRESET delete My phone memo") == "OK" and "My phone memo" not in c.ask("PRESET list"))

        ans = c.ask("CONVERT channels=2")
        st = c.state()
        check("CONVERT channels=2 copies mono to stereo", ans == "OK" and (st["channels"], st["frames"]) == ("2", str(want)), f"{ans}; {st}")
        bad = c.ask("CONVERT bits=12")
        check("CONVERT bits=12 answers ERR", bad.startswith("ERR"), bad)
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
