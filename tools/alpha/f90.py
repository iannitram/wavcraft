#!/usr/bin/env python3
"""f90 alpha (Bars and Beats): through the control channel, on a synthetic 44.1 kHz file in a
scratch HOME: switch to Bars and Beats, set the tempo and meter with PREF, type selections with
SELECT_TIME in bar:beat.tick and check STATE's selection (frames) and selection_text; times that
do not read are refused; the tempo is saved per document in the settings file and a second start
comes up with it; the app's log has no ERROR line.

    python3 tools/alpha/f90.py --exe <path to the release wavcraft>
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave, math, struct

RATE = 44100
PORT = 47190


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


def near(sel, a, b, tol=20):
    """True when the selection text "x-y" is within `tol` frames of a-b (a time reads from the first frame that shows it)."""
    x, _, y = (sel or "").partition("-")
    return x.isdigit() and y.isdigit() and 0 <= a - int(x) <= tol and 0 <= b - int(y) <= tol


def make_wav(path, seconds=10):
    with wave.open(path, "wb") as w:
        w.setnchannels(2), w.setsampwidth(2), w.setframerate(RATE)
        frames = b"".join(struct.pack("<hh", int(8000 * math.sin(i * 0.05)), int(8000 * math.sin(i * 0.07))) for i in range(RATE))
        for _ in range(seconds):
            w.writeframes(frames)


def start(exe, env, wav):
    return subprocess.Popen([exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    exe = args.exe
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f90.", dir=scratch_root)
    home, logs = os.path.join(work, "home"), os.path.join(work, "logs")
    os.makedirs(home)
    wav, token = os.path.join(work, "tone.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    env = dict(os.environ, HOME=home, USERPROFILE=home, XDG_CONFIG_HOME=os.path.join(home, ".config"), WAVCRAFT_LOG_DIR=logs,
               WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))

    proc = start(exe, env, wav)
    try:
        c = Control(PORT, token)
        st = c.state()
        check("the file opens at the default tempo", st.get("frames") == str(10 * RATE) and st.get("tempo") == "120" and st.get("meter") == "4/4", st)
        ans = c.ask("ACTION view.time_display.bars_beats")
        check("ACTION view.time_display.bars_beats", ans.startswith("OK") and c.state().get("time_display") == "bars_beats", ans)
        # 120 bpm, 4/4: a bar is 2 s.
        c.ask("SELECT_TIME 2:1.000 3:2.000")
        st = c.state()
        check("120 bpm 4/4: 2:1.000-3:2.000 is frames 88200-198450 (within a tick)", near(st.get("selection"), 2 * RATE, 4 * RATE + RATE // 2) and st.get("selection_text") == "2:1.000-3:2.000", (st.get("selection"), st.get("selection_text")))
        check("PREF set tempo 90", c.ask("PREF set tempo 90") == "OK tempo=90")
        c.ask("SELECT_TIME 2:1.000 3:1.000")
        st = c.state()
        check("90 bpm: a bar is 2.667 s, 2:1.000-3:1.000 is 117600-235200", near(st.get("selection"), 117600, 235200) and st.get("selection_text") == "2:1.000-3:1.000" and st.get("tempo") == "90", (st.get("selection"), st.get("selection_text")))
        check("PREF set meter 3/4", c.ask("PREF set meter 3/4") == "OK meter=3/4")
        c.ask("SELECT_TIME 2:1.000 3:2.480")
        st = c.state()
        check("90 bpm 3/4: a bar is 2 s, 2:1.000-3:2.480 is 88200-220500", near(st.get("selection"), 88200, 220500) and st.get("selection_text") == "2:1.000-3:2.480", (st.get("selection"), st.get("selection_text")))
        bad = c.ask("SELECT_TIME 1:4.000 2:1.000")
        check("beat 4 does not exist in 3/4: refused", bad.startswith("ERR"), bad)
        check("the refusal left the selection alone", near(c.state().get("selection"), 88200, 220500))
        check("a tempo out of range is refused", c.ask("PREF set tempo 5").startswith("ERR") and c.ask("PREF get tempo") == "OK tempo=90")
        check("a bad meter is refused", c.ask("PREF set meter 4/5").startswith("ERR"))
        other = c.ask("ACTION view.time_display.decimal")
        c.ask("SELECT_TIME 0:01.000 0:02.000")
        check("the other formats are unchanged", other.startswith("OK") and c.state().get("selection_text") == "0:01.000-0:02.000")
        c.ask("ACTION view.time_display.bars_beats")
        check("QUIT", c.ask("QUIT") == "OK")
        proc.wait(20)
    except Exception as e:
        check("control channel", False, f"{type(e).__name__}: {e}")
        proc.kill()
    found = [os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs if f == "settings.txt"]
    text = open(found[0]).read() if found else ""
    check("the settings file holds this document's tempo and the format", "tempo = 90 3/4 " in text and "time_display = bars_beats" in text, text[-300:] or "no settings.txt")
    proc = start(exe, env, wav)
    try:
        c = Control(PORT, token)
        st = c.state()
        check("a second start comes up in Bars and Beats at 90 bpm 3/4", st.get("time_display") == "bars_beats" and st.get("tempo") == "90" and st.get("meter") == "3/4", st)
        c.ask("SELECT_TIME 1:1.000 2:1.000")
        check("and SELECT_TIME reads it there", near(c.state().get("selection"), 0, 2 * RATE), c.state().get("selection"))
        c.ask("QUIT")
        proc.wait(20)
    except Exception as e:
        check("second start", False, f"{type(e).__name__}: {e}")
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
