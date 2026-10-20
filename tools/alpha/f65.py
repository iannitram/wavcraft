#!/usr/bin/env python3
"""f65 scripted run (roadmap A061, A063-A065): the spectral display's options through the control
channel, then a restart to check they were kept in the settings file.

    python3 tools/alpha/f65.py --exe <path to the release wavcraft> [--port 47165]

HOME is a fresh scratch folder (deleted at the end), with the log folder, session folder and
token file inside it. The test file is a synthetic 6 s stereo WAV written here.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE = 16000


def make_wav(path, seconds=6):
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        frames = bytearray()
        for i in range(RATE * seconds):
            t = i / RATE
            a = int(9000 * math.sin(2 * math.pi * 1015 * t))
            b = int(5000 * math.sin(2 * math.pi * (300 + 200 * t) * t))
            frames += struct.pack("<hh", a, b)
        w.writeframes(bytes(frames))


class Control:
    def __init__(self, port, token, tries=80):
        for _ in range(tries):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
                break
            except OSError:
                time.sleep(0.25)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("the control channel refused the token")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self, wait=True, timeout=60):
        t0 = time.time()
        while True:
            head = self.ask("STATE").partition(" undo=")[0]
            fields = dict(kv.split("=", 1) for kv in head.split()[1:])
            if not wait or (fields.get("busy") == "0" and int(fields.get("frames", 0)) > 0) or time.time() - t0 > timeout:
                return fields
            time.sleep(0.2)


def start(exe, env, wav, port, token):
    err = open(os.path.join(env["HOME"], "stderr.log"), "a")
    proc = subprocess.Popen([exe, wav], env=env, stdout=err, stderr=err)
    for _ in range(80):
        if os.path.exists(token):
            break
        time.sleep(0.25)
    return proc, Control(port, open(token).read().strip())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47165)
    args = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f65.", dir=root)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))

    try:
        home = os.path.join(work, "home")
        os.makedirs(home)
        logs = os.path.join(work, "logs")
        wav = os.path.join(work, "tones.wav")
        make_wav(wav)
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"),
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), APPDATA=os.path.join(home, "AppData", "Roaming"),
                   WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
        proc, c = start(args.exe, env, wav, args.port, token)
        st = c.state()
        check("opens the file", int(st.get("frames", 0)) == 6 * RATE, st)
        check("defaults: shown, 2048, hann, 100 dB, split 50",
              (st.get("spectral"), st.get("fft"), st.get("window"), st.get("range"), st.get("split")) == ("1", "2048", "hann", "100", "50"), st)

        def spectral(cmd):
            ans = c.ask(f"SPECTRAL {cmd}")
            return ans, c.state(wait=False)

        a, st = spectral("hide")
        check("SPECTRAL hide", a.startswith("OK") and st["spectral"] == "0", (a, st.get("spectral")))
        a, st = spectral("show")
        check("SPECTRAL show", a.startswith("OK") and st["spectral"] == "1", (a, st.get("spectral")))
        a, st = spectral("toggle")
        check("SPECTRAL toggle hides, again shows", st["spectral"] == "0" and spectral("toggle")[1]["spectral"] == "1")
        a, st = spectral("fft 4096")
        check("SPECTRAL fft 4096", st["fft"] == "4096", a)
        a, st = spectral("fft up")
        check("SPECTRAL fft up -> 8192", st["fft"] == "8192", a)
        a, st = spectral("fft down")
        a, st = spectral("fft down")
        check("SPECTRAL fft down twice -> 2048", st["fft"] == "2048", a)
        a, st = spectral("fft 4096")
        a, st = spectral("window blackman-harris")
        check("SPECTRAL window blackman-harris", st["window"] == "blackman-harris", a)
        a, st = spectral("range 140")
        check("SPECTRAL range 140", st["range"] == "140", a)
        a, st = spectral("split 30")
        check("SPECTRAL split 30", st["split"] == "30", a)
        for bad in ("fft 1000", "window kaiser", "range 5", "range 500", "range nan", "split 99", "fly"):
            ans = c.ask(f"SPECTRAL {bad}")
            check(f"SPECTRAL {bad} is refused", ans.startswith("ERR"), ans)
        st = c.state(wait=False)
        check("refused commands changed nothing", (st["fft"], st["window"], st["range"], st["split"]) == ("4096", "blackman-harris", "140", "30"), st)
        for w in ("hann", "hamming", "blackman", "rectangular"):
            c.ask(f"SPECTRAL window {w}")
            time.sleep(0.4)
        c.ask("SPECTRAL window blackman-harris")
        time.sleep(1.0)  # let the renderer redraw with the last settings
        check("the app is still running after redraws with every window", proc.poll() is None)
        c.ask("SPECTRAL hide")
        check("QUIT", c.ask("QUIT") == "OK")
        proc.wait(20)

        settings = [os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs if f == "settings.txt"]
        text = open(settings[0]).read() if settings else ""
        want = ["spectral_show = 0", "spectral_fft = 4096", "spectral_window = blackman-harris", "spectral_range_db = 140", "spectral_split = 30"]
        check("settings.txt holds the options", all(w in text for w in want), text)

        proc, c = start(args.exe, env, wav, args.port, token)
        st = c.state()
        check("after a restart the options are back",
              (st.get("spectral"), st.get("fft"), st.get("window"), st.get("range"), st.get("split")) == ("0", "4096", "blackman-harris", "140", "30"), st)
        c.ask("SPECTRAL show")
        time.sleep(1.0)
        check("QUIT again", c.ask("QUIT") == "OK")
        proc.wait(20)

        bad = []
        for d, _, fs in os.walk(work):
            for f in fs:
                if f.endswith(".log"):
                    bad += [l for l in open(os.path.join(d, f), errors="replace") if "ERROR" in l or "panicked" in l]
        check("no ERROR line in the app's log or stderr", not bad, bad[:5])
    finally:
        shutil.rmtree(work, ignore_errors=True)
    failed = [c for c in checks if not c[1]]
    for label, ok, detail in checks:
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"))
    print(f"{len(checks) - len(failed)}/{len(checks)} checks passed")
    sys.exit(1 if failed else 0)


main()
