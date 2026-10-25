#!/usr/bin/env python3
"""f47 alpha (SMPTE formats and snap to frames): through the control channel, on a synthetic
44.1 kHz mono file (125 s) in a scratch HOME: for each of the six SMPTE rates, switch View > Time
Display, type a start and end with SELECT_TIME, and check STATE's frames against an independent
calculation (Fraction arithmetic here); the drop-frame minute boundary 00:00:59;29 to 00:01:00;02;
a dropped number is refused; snap to frames (SNAP_FRAME, the step a ruler click or drag calls)
lands on frame boundaries in SMPTE, does nothing in Decimal, and does nothing when Snap to Frames
is toggled off; the app's log has no ERROR line.

    python3 tools/alpha/f47.py --exe <path to the release wavcraft>
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave, math, struct
from fractions import Fraction
from math import ceil, floor

RATE = 44100
PORT = 47147
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


F2997 = Fraction(30000, 1001)
# (action suffix, rate as a Fraction, start text, start timecode frame number, end text, end frame number)
CASES = [
    ("smpte_24", Fraction(24), "00:00:01:00", 24, "00:00:02:12", 60),
    ("smpte_25", Fraction(25), "00:00:01:00", 25, "00:00:02:12", 62),
    ("smpte_2997ndf", F2997, "00:00:01:00", 30, "00:01:00:00", 1800),
    ("smpte_30", Fraction(30), "00:00:01:00", 30, "00:01:00:15", 1815),
    ("smpte_5994", 2 * F2997, "00:00:01:00", 60, "00:00:02:30", 150),
    # Drop-frame: 00:00:59;29 is timecode frame 1799 and 00:01:00;02 is 1800 (two numbers skipped);
    # 00:10:00;00 is 17982 (the tenth minute keeps its numbers).
    ("smpte_2997df", F2997, "00:00:59;29", 1799, "00:01:00;02", 1800),
    ("smpte_2997df", F2997, "00:01:00;02", 1800, "00:02:00;02", 3600 - 2 + 0),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    exe = args.exe
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f47.", dir=scratch_root)
    home, logs = os.path.join(work, "home"), os.path.join(work, "logs")
    os.makedirs(home)
    wav, token = os.path.join(work, "tone.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    env = dict(os.environ, HOME=home, USERPROFILE=home, XDG_CONFIG_HOME=os.path.join(home, ".config"), WAVCRAFT_LOG_DIR=logs,
               WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))

    proc = subprocess.Popen([exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        c = Control(PORT, token)
        st = c.state()
        check("the file opens", st.get("frames") == str(SECONDS * RATE), st)
        for name, fps, a, na, b, nb in CASES:
            ans = c.ask(f"ACTION view.time_display.{name}")
            c.ask(f"SELECT_TIME {a} {b}")
            st = c.state()
            want = f"{first_sample(na, fps)}-{first_sample(nb, fps)}"
            check(f"{name}: typed {a} {b} selects {want} and reads back the same text",
                  ans.startswith("OK") and st.get("time_display") == name and st.get("selection") == want and st.get("selection_text") == f"{a}-{b}",
                  f"{ans} | {st.get('selection')} {st.get('selection_text')}")
        c.ask("ACTION view.time_display.smpte_2997df")
        bad = c.ask("SELECT_TIME 00:01:00;00 00:02:00;02")
        check("drop-frame: 00:01:00;00 does not exist and is refused", bad.startswith("ERR"), bad)
        c.ask("ACTION view.time_display.smpte_25")
        bad = c.ask("SELECT_TIME 00:00:01:25 00:00:02:00")
        check("25 fps: frame 25 does not exist and is refused", bad.startswith("ERR"), bad)

        # Snap to frames, 25 fps: 1764 samples a frame at 44.1 kHz.
        for frame, want in [(0, 0), (881, 0), (882, 1764), (1764, 1764), (2000, 1764), (2700, 3528)]:
            ans = c.ask(f"SNAP_FRAME {frame}")
            check(f"25 fps: SNAP_FRAME {frame} -> {want}", ans == f"OK frame={want}", ans)
        # 29.97 non-drop: frame boundaries are first samples of frames (1471.47 samples a frame).
        c.ask("ACTION view.time_display.smpte_2997ndf")
        ans = c.ask("SNAP_FRAME 1000000")
        got = int(ans.rpartition("=")[2]) if ans.startswith("OK") else -1
        n = round(Fraction(1000000) * F2997 / RATE)
        check("29.97 ndf: SNAP_FRAME 1000000 lands on the nearest frame start", got == first_sample(n, F2997), f"{ans} want {first_sample(n, F2997)}")
        c.ask("ACTION view.time_display.decimal")
        ans = c.ask("SNAP_FRAME 1000000")
        check("Decimal display: SNAP_FRAME changes nothing", ans == "OK frame=1000000", ans)
        c.ask("ACTION view.time_display.smpte_25")
        check("snap_frames is on by default", c.state().get("snap_frames") == "1")
        c.ask("ACTION view.snap_frames.toggle")
        ans = c.ask("SNAP_FRAME 2000")
        check("Snap to Frames off: SNAP_FRAME changes nothing", c.state().get("snap_frames") == "0" and ans == "OK frame=2000", ans)
        c.ask("ACTION view.snap_frames.toggle")
        check("and back on", c.ask("SNAP_FRAME 2000") == "OK frame=1764")
        check("QUIT", c.ask("QUIT") == "OK")
        proc.wait(20)
    except Exception as e:
        check("control channel", False, f"{type(e).__name__}: {e}")
        proc.kill()
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
