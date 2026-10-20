#!/usr/bin/env python3
"""Alpha check for f79 (Extract Channels to Mono Files, Export Audio of Range Markers): drives the release app through the control channel.

    python3 tools/alpha/f79.py --exe <path to wavcraft>

A synthetic 2 s stereo 16-bit WAV is opened in a scratch HOME. `ACTION edit.extract_channels`
must write tone_L.wav and tone_R.wav beside it, sample-identical to the source channels. Three
range markers (two overlapping) and a point are added; `EXPORT_RANGES <folder> pattern=...` must
write one WAV per range, sample-identical to the source ranges, with sanitised names, no temp
file left, and nothing replaced when run again. QUIT; the app's log must hold no ERROR line.
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave, math

RATE = 48000
PORT = 47179


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<hh", int(8000 * math.sin(i * 0.05)), int(6000 * math.sin(i * 0.031 + 1))) for i in range(2 * RATE)))


def read_wav(path):
    with wave.open(path, "rb") as w:
        return w.getnchannels(), w.getframerate(), w.readframes(w.getnframes())


def channel(raw, ch, n):
    return b"".join(raw[(i * n + ch) * 2:(i * n + ch) * 2 + 2] for i in range(len(raw) // (2 * n)))


class Control:
    def __init__(self, port, token):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n"); self.f.flush()
        return self.f.readline().strip()

    def state(self):
        while True:
            head = self.ask("STATE").split(" undo=")[0]
            d = dict(kv.split("=", 1) for kv in head.split()[1:])
            if d.get("busy") == "0":
                return d
            time.sleep(0.2)

    def markers(self):
        ans = self.ask("MARKERS")
        body = ans.split(" ", 2)[2] if ans.count(" ") >= 2 else ""
        out = []
        for x in body.split(";"):
            if x:
                i, a, b, n = x.split(",", 3)
                out.append((int(a), int(b), n))
        return out


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch"); os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f79.", dir=scratch_root)
    checks = []
    def check(label, ok, detail=""):
        checks.append(ok); print(("PASS " if ok else "FAIL ") + label + ("" if ok else f": {detail}"))
    proc = None
    try:
        home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
        os.makedirs(home)
        src_dir = os.path.join(work, "src"); os.makedirs(src_dir)
        wav = os.path.join(src_dir, "tone.wav"); make_wav(wav)
        _, _, src = read_wav(wav)
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
                   WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
        err = open(os.path.join(work, "stderr.log"), "w")
        proc = subprocess.Popen([exe, wav], env=env, stdout=err, stderr=err)
        for _ in range(120):
            if os.path.exists(token): break
            time.sleep(0.5)
        c = Control(PORT, open(token).read().strip())
        st = c.state()
        while int(st["frames"]) != 2 * RATE:
            time.sleep(0.2); st = c.state()

        # --- Extract Channels ---
        ans = c.ask("ACTION edit.extract_channels")
        check("extract accepted", ans.startswith("OK"), ans)
        c.state()
        files = sorted(os.listdir(src_dir))
        check("tone_L.wav and tone_R.wav beside the file, the source untouched", files == ["tone.wav", "tone_L.wav", "tone_R.wav"], files)
        for name, ch in (("tone_L.wav", 0), ("tone_R.wav", 1)):
            try:
                n, rate, raw = read_wav(os.path.join(src_dir, name))
                check(f"{name} is mono, same rate, sample-identical to channel {ch}", n == 1 and rate == RATE and raw == channel(src, ch, 2), (n, rate, len(raw)))
            except Exception as e:
                check(f"{name} readable", False, e)
        check("source file unchanged", read_wav(wav)[2] == src)
        ans = c.ask("ACTION edit.extract_channels")
        c.state()
        check("a second extract keeps the first files", sorted(os.listdir(src_dir)) == ["tone.wav", "tone_L.wav", "tone_L_2.wav", "tone_R.wav", "tone_R_2.wav"], sorted(os.listdir(src_dir)))

        # --- Export range markers ---
        ranges = [(10000, 60000), (40000, 90000), (91000, 95500)]
        for a, b in ranges:
            c.ask(f"SELECT {a} {b}"); c.ask("ACTION marker.add")
        c.ask("SELECT 70000 70000"); c.ask("ACTION marker.add")
        ms = c.markers()
        check("three ranges and a point added", len(ms) == 4 and sum(1 for a, b, _ in ms if a != b) == 3, ms)
        out = os.path.join(work, "out dir")
        ans = c.ask(f"EXPORT_RANGES {out} pattern={{index}}-{{name}}")
        check("export accepted", ans == "OK", ans)
        c.state()
        got = sorted(os.listdir(out)) if os.path.isdir(out) else []
        names = [n for a, b, n in ms if a != b]
        want = sorted(f"{i + 1:03d}-{''.join(ch if ch.isalnum() or ch in ' -_.()' else '_' for ch in n)}.wav" for i, n in enumerate(names))
        check("one file per range marker, named from the marker (sanitised), no temp files", got == want, (got, want))
        for i, (a, b) in enumerate(ranges):
            f = os.path.join(out, [g for g in got if g.startswith(f"{i + 1:03d}-")][0]) if got else ""
            try:
                n, rate, raw = read_wav(f)
                check(f"range {a}-{b} is sample-identical to the source", n == 2 and rate == RATE and raw == src[a * 4:b * 4], (n, rate, len(raw)))
            except Exception as e:
                check(f"range {a}-{b} readable", False, e)
        c.ask(f"EXPORT_RANGES {out} pattern={{index}}-{{name}}"); c.state()
        check("running it again replaces nothing", len(os.listdir(out)) == 6, sorted(os.listdir(out)))
        ans = c.ask("EXPORT_RANGES")
        check("EXPORT_RANGES without a folder is refused", ans.startswith("ERR"), ans)
        check("QUIT", c.ask("QUIT discard") == "OK")
        proc.wait(30)
        log_text = ""
        for d, _, fs in os.walk(logs):
            for f in fs:
                log_text += open(os.path.join(d, f), errors="replace").read()
        errors = [l for l in log_text.splitlines() if "ERROR" in l]
        check("no ERROR line in the app's log", not errors, errors[:3])
    finally:
        if proc and proc.poll() is None:
            proc.kill()
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if checks and all(checks) else 1)


main()
