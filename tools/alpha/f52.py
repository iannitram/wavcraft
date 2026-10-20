#!/usr/bin/env python3
"""f52 scripted run: the spectral display's frequency axis and the status bar's fields.

    python3 tools/alpha/f52.py --exe <path to the release wavcraft>

Starts the app twice on a synthetic stereo file (1 kHz left, 5 kHz right) with HOME, the session
and log folders in a fresh scratch folder. Run 1 sets the band and scale (`SPECTRAL view`), shows
and hides status bar fields (`STATUSBAR`), checks `STATE`, the refusals of bad input and the
settings file, and quits. Run 2 starts again and checks that both settings came back, resets the
band and quits. Fails on any ERROR line in the app's log or a panic on stderr.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, SECONDS, PORT = 48000, 3, 47152


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        frames = bytearray()
        for i in range(RATE * SECONDS):
            l = int(0.5 * 32767 * math.sin(2 * math.pi * 1000 * i / RATE))
            r = int(0.5 * 32767 * math.sin(2 * math.pi * 5000 * i / RATE))
            frames += struct.pack("<hh", l, r)
        w.writeframes(bytes(frames))


class Control:
    def __init__(self, port, token_file):
        for _ in range(120):
            if os.path.exists(token_file):
                try:
                    self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
                    break
                except OSError:
                    pass
            time.sleep(0.5)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {open(token_file).read().strip()}") != "OK":
            raise RuntimeError("the control channel refused the token")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self, wait=True):
        t0 = time.time()
        while True:
            head = self.ask("STATE").partition(" undo=")[0]
            st = dict(kv.split("=", 1) for kv in head.split()[1:])
            if not wait or (st.get("busy") == "0" and st.get("frames", "0") != "0") or time.time() - t0 > 60:
                return st
            time.sleep(0.2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f52.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    wav, token = os.path.join(work, "tones.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    def settings_text():
        for d, _, fs in os.walk(home):
            if "settings.txt" in fs:
                return open(os.path.join(d, "settings.txt")).read()
        return ""

    def run(body):
        err = open(os.path.join(work, "stderr.log"), "a")
        proc = subprocess.Popen([exe, wav], env=env, stdout=err, stderr=err)
        try:
            body(Control(PORT, token))
            proc.wait(20)
        except Exception as e:
            check("control channel", False, f"{type(e).__name__}: {e}")
            proc.kill()
        err.close()
        check("the app exits cleanly", proc.returncode == 0, f"exit {proc.returncode}")

    def run1(c):
        st = c.state()
        check("opens the file", st["frames"] == str(RATE * SECONDS), st)
        check("defaults: whole range, log scale", st["spectral"] == "40-24000:log", st["spectral"])
        shown = st["statusbar"].split(",")
        check("defaults: status bar shows rate and duration, not size", "sample_rate" in shown and "duration" in shown and "file_size" not in shown, st["statusbar"])
        a = c.ask("SPECTRAL view 900 1100 lin")
        st = c.state()
        check("SPECTRAL view sets the band and the linear scale", a == "OK spectral=900-1100:lin" and st["spectral"] == "900-1100:lin", f"{a}; {st['spectral']}")
        a = c.ask("SPECTRAL view 500 20000")
        check("a band without a scale keeps the scale", a == "OK spectral=500-20000:lin", a)
        a = c.ask("SPECTRAL view 200 4000 log")
        check("log scale", a == "OK spectral=200-4000:log", a)
        a = c.ask("SPECTRAL view 0 30000 lin")
        check("a band past Nyquist is cut to it", a == "OK spectral=0-24000:lin", a)
        for bad in ("SPECTRAL view 1000 500", "SPECTRAL view x y", "SPECTRAL view 1 2 sideways", "SPECTRAL"):
            answer = c.ask(bad)
            check(f"refuses {bad!r}", answer.startswith("ERR"), answer)
        time.sleep(1.5)  # the renderer draws the band
        a = c.ask("SPECTRAL view 900 1100 lin")
        check("the band can be changed again after drawing", a == "OK spectral=900-1100:lin", a)
        a = c.ask("STATUSBAR show file_size")
        check("STATUSBAR show", a.startswith("OK statusbar=") and "file_size" in a, a)
        a = c.ask("STATUSBAR hide channels")
        st = c.state()
        check("STATUSBAR hide", "channels" not in a.split("=", 1)[1].split(",") and "channels" not in st["statusbar"].split(","), a)
        answer = c.ask("STATUSBAR show nonsense")
        check("STATUSBAR refuses an unknown field", answer.startswith("ERR"), answer)
        answer = c.ask("STATUSBAR hide")
        check("STATUSBAR refuses a missing field", answer.startswith("ERR"), answer)
        time.sleep(1.0)
        text = settings_text()
        check("the settings file holds the band", "spectral_view = 900 1100 lin" in text, text.replace("\n", " | "))
        fields = [l.split("=", 1)[1].strip() for l in text.splitlines() if l.startswith("status_fields")]
        check("the settings file holds the fields", fields and "file_size" in fields[0].split(",") and "channels" not in fields[0].split(","), fields)
        check("QUIT", c.ask("QUIT discard") == "OK", "")

    def run2(c):
        st = c.state()
        check("after a restart the band and scale are back", st["spectral"] == "900-1100:lin", st["spectral"])
        shown = st["statusbar"].split(",")
        check("after a restart the fields are back", "file_size" in shown and "channels" not in shown and "duration" in shown, st["statusbar"])
        a = c.ask("STATUSBAR show channels")
        check("a hidden field can be shown again", "channels" in a, a)
        a = c.ask("SPECTRAL reset")
        check("SPECTRAL reset shows the whole range on the current scale", a == "OK spectral=0-24000:lin", a)
        c.ask("SPECTRAL view 40 24000 log")
        time.sleep(1.0)
        check("QUIT", c.ask("QUIT discard") == "OK", "")

    run(run1)
    run(run2)
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f.endswith(".log")]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    stderr = open(os.path.join(work, "stderr.log"), errors="replace").read()
    bad = [l for l in stderr.splitlines() if "panicked" in l or "fatal" in l.lower()]
    check("nothing on stderr looks like a panic", not bad, bad[:3] or f"{len(stderr.splitlines())} lines")
    ok = all(c["ok"] for c in checks)
    for c in checks:
        print(("PASS " if c["ok"] else "FAIL ") + c["check"] + (f"  [{c['detail']}]" if not c["ok"] else ""))
    print("f52 scripted run:", "PASS" if ok else "FAIL", f"({sum(c['ok'] for c in checks)}/{len(checks)})")
    shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


main()
