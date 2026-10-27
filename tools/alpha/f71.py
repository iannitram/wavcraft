#!/usr/bin/env python3
"""f71 alpha (custom frame rate 2-1000 and Compact Disc mm:ss:ff at 75 fps): through the control channel,
on a synthetic 44.1 kHz mono file (125 s) in a scratch HOME: PREF set custom_fps (bounds refused),
ACTION view.time_display.custom / cd75, typed times select the sample frames an independent Fraction
calculation gives and read back as the same text, snap to frames lands on the custom / CD grid, the rate
is persisted (the settings file names it, and a second run of the app starts on it); no ERROR in the log.

    python3 tools/alpha/f71.py --exe <path to the release wavcraft>
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave, math, struct
from fractions import Fraction
from math import ceil, floor

RATE = 44100
PORT = 47171
SECONDS = 125

class Control:
    def __init__(self, port, token_file):
        for _ in range(120):
            if os.path.exists(token_file):
                break
            time.sleep(0.25)
        for _ in range(60):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask("TOKEN " + open(token_file).read().strip()) != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self, wait=True):
        for _ in range(300):
            head = self.ask("STATE").partition(" undo=")[0]
            d = dict(kv.split("=", 1) for kv in head.split()[1:])
            if not wait or d.get("busy") == "0":
                return d
            time.sleep(0.2)
        return d



def first_sample(n, fps):
    return ceil(Fraction(n) / fps * RATE)


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(1), w.setsampwidth(2), w.setframerate(RATE)
        chunk = b"".join(struct.pack("<h", int(8000 * math.sin(i * 0.05))) for i in range(RATE))
        for _ in range(SECONDS):
            w.writeframes(chunk)



def run_app(exe, env, wav, work, token, checks, check, body):
    proc = subprocess.Popen([exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        c = Control(PORT, token)
        body(c)
        check("QUIT", c.ask("QUIT") == "OK")
        proc.wait(20)
    except Exception as e:
        check("control channel", False, f"{type(e).__name__}: {e}")
        proc.kill()
        proc.wait()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    exe = args.exe
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f71.", dir=scratch_root)
    home, logs = os.path.join(work, "home"), os.path.join(work, "logs")
    os.makedirs(home)
    wav, token = os.path.join(work, "tone.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    env = dict(os.environ, HOME=home, USERPROFILE=home, XDG_CONFIG_HOME=os.path.join(home, ".config"), WAVCRAFT_LOG_DIR=logs,
               WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))

    def first(run):
        st = run.state()
        check("the file opens", st.get("frames") == str(SECONDS * RATE), st)

    def one(c):
        first(c)
        check("default custom rate is 48", c.state().get("custom_fps") == "48")
        for bad in ["1", "0", "1001", "abc", "24.5", "-3"]:
            ans = c.ask(f"PREF set custom_fps {bad}")
            check(f"PREF custom_fps {bad} refused", ans.startswith("ERR"), ans)
        check("a refused rate changes nothing", c.state().get("custom_fps") == "48")
        for n, a, na, b, nb in [(2, "00:00:01:01", 3, "00:00:03:00", 6), (30, "00:00:01:00", 30, "00:01:00:15", 1815),
                                (100, "00:00:01:99", 199, "00:00:02:50", 250), (1000, "00:00:01:999", 1999, "00:01:00:000", 60000)]:
            ans = c.ask(f"PREF set custom_fps {n}")
            check(f"PREF set custom_fps {n}", ans == "OK", ans)
            ans = c.ask("ACTION view.time_display.custom")
            c.ask(f"SELECT_TIME {a} {b}")
            st = c.state()
            want = f"{first_sample(na, Fraction(n))}-{first_sample(nb, Fraction(n))}"
            check(f"custom {n} fps: {a} {b} selects {want} and reads back", ans.startswith("OK") and st.get("time_display") == "custom"
                  and st.get("selection") == want and st.get("selection_text") == f"{a}-{b}", f"{ans} | {st.get('selection')} {st.get('selection_text')}")
        bad = c.ask("SELECT_TIME 00:00:01:1000 00:00:02:000")
        check("custom 1000 fps: frame 1000 does not exist", bad.startswith("ERR"), bad)
        c.ask("PREF set custom_fps 25")
        bad = c.ask("SELECT_TIME 00:00:01:25 00:00:02:00")
        check("custom 25 fps: frame 25 does not exist", bad.startswith("ERR"), bad)
        c.ask("PREF set custom_fps 100")
        for frame, want in [(0, 0), (239, 0), (240, 480), (481, 480), (2000, 1920)]:
            ans = c.ask(f"SNAP_FRAME {frame}")
            check(f"custom 100 fps (441 samples a frame): SNAP_FRAME {frame}", ans == f"OK frame={round(frame / 441) * 441}", ans)
        # Compact Disc
        ans = c.ask("ACTION view.time_display.cd75")
        check("cd75 action", ans.startswith("OK") and c.state().get("time_display") == "cd75", ans)
        for a, na, b, nb in [("00:01:00", 75, "01:00:00", 4500), ("00:59:74", 4499, "01:30:00", 6750), ("00:00:74", 74, "02:04:30", 9330)]:
            c.ask(f"SELECT_TIME {a} {b}")
            st = c.state()
            want = f"{first_sample(na, Fraction(75))}-{first_sample(nb, Fraction(75))}"
            check(f"cd75: {a} {b} selects {want} and reads back", st.get("selection") == want and st.get("selection_text") == f"{a}-{b}", f"{st.get('selection')} {st.get('selection_text')}")
        for bad_t in ["00:00:75 00:01:00", "00:60:00 01:00:00", "01:00:00:00 02:00:00:00"]:
            bad = c.ask("SELECT_TIME " + bad_t)
            check(f"cd75 refuses {bad_t}", bad.startswith("ERR"), bad)
        for frame, want in [(293, 0), (294, 588), (1000, 1176)]:
            ans = c.ask(f"SNAP_FRAME {frame}")
            check(f"cd75: SNAP_FRAME {frame} -> {want}", ans == f"OK frame={want}", ans)
        # Persistence: leave on custom 120.
        c.ask("PREF set custom_fps 120")
        c.ask("ACTION view.time_display.custom")
        check("custom 120 chosen", c.state().get("time_display") == "custom")

    run_app(exe, env, wav, work, token, checks, check, one)
    settings = ""
    for d, _, fs in os.walk(work):
        for f in fs:
            if f == "settings.txt":
                settings += open(os.path.join(d, f)).read()
    check("the settings file names custom and 120", "time_display = custom" in settings and "custom_fps = 120" in settings, settings[-200:])
    os.remove(token)

    def two(c):
        first(c)
        st = c.state()
        check("a second run starts on custom 120 fps", st.get("time_display") == "custom" and st.get("custom_fps") == "120", st)
        c.ask("SELECT_TIME 00:00:01:119 00:00:02:000")
        st = c.state()
        check("and reads 120 fps times", st.get("selection") == f"{first_sample(239, Fraction(120))}-{first_sample(240, Fraction(120))}", st.get("selection"))

    run_app(exe, env, wav, work, token, checks, check, two)
    logtext = ""
    for d, _, fs in os.walk(logs):
        for f in fs:
            if f.startswith("wavcraft") and f.endswith(".log"):
                logtext += open(os.path.join(d, f), errors="replace").read()
    errs = [l for l in logtext.splitlines() if "ERROR" in l]
    check("no ERROR line in the app's log", not errs and logtext, errs[:3] or f"{len(logtext.splitlines())} log lines")
    ok = True
    for label, good, detail in checks:
        print(("PASS " if good else "FAIL ") + label + ("" if good else f"  [{detail}]"))
        ok = ok and good
    print("RESULT", "pass" if ok else "FAIL")
    if not ok:
        print("scratch kept:", work)
    elif not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


main()
