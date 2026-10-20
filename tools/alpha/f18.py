#!/usr/bin/env python3
"""f18 alpha (time display): through the control channel, on a synthetic 44.1 kHz file in a
scratch HOME: switch View > Time Display to each format, type a selection with SELECT_TIME in
that format, and check STATE's selection (frames) and selection_text (formatted). Also: a typed
time that does not read is refused; the choice is saved in the settings file and a second start
comes up in it; the app's log has no ERROR line.

    python3 tools/alpha/f18.py --exe <path to the release wavcraft>
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave, math, struct

RATE = 44100
PORT = 47118

CASES = [  # (action name, start text, end text, expected selection text)
    ("decimal", "0:01.000", "0:02.500", "0:01.000-0:02.500"),
    ("compact", "0:01", "0:02.5", "0:01-0:02.5"),
    ("samples", "44100", "110250", "44100-110250"),
    ("smpte30", "00:00:01:00", "00:00:02:15", "00:00:01:00-00:00:02:15"),
]


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
    exe = ap.parse_args().exe
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f18.", dir=scratch_root)
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
        check("the file opens", st.get("frames") == str(10 * RATE) and st.get("time_display") == "decimal", st)
        for name, a, b, text in CASES:
            ans = c.ask(f"ACTION view.time_display.{name}")
            sel = c.ask(f"SELECT_TIME {a} {b}")
            st = c.state()
            check(f"{name}: typed {a} {b} selects frames 44100-110250 and reads back {text}",
                  ans.startswith("OK") and st.get("time_display") == name and st.get("selection") == "44100-110250" and st.get("selection_text") == text,
                  f"{ans} | {sel} | {st.get('selection')} {st.get('selection_text')}")
        bad = c.ask("SELECT_TIME 00:00:01:30 00:00:02:00")
        check("a time that does not read in the format is refused (frame 30 at 30 fps)", bad.startswith("ERR"), bad)
        check("the refusal left the selection alone", c.state().get("selection") == "44100-110250")
        # SELECT_TIME past the end clamps like SELECT.
        c.ask("SELECT_TIME 00:00:09:00 00:01:00:00")
        check("a time past the end is clamped to the file", c.state().get("selection") == f"{9 * RATE}-{10 * RATE}", c.state().get("selection"))
        check("QUIT", c.ask("QUIT") == "OK")
        proc.wait(20)
    except Exception as e:
        check("control channel", False, f"{type(e).__name__}: {e}")
        proc.kill()
    # The choice is in the settings file, and a second start uses it.
    found = [os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs if f == "settings.txt"]
    text = open(found[0]).read() if found else ""
    check("the settings file holds time_display = smpte30", "time_display = smpte30" in text, found or "no settings.txt")
    proc = start(exe, env, wav)
    try:
        c = Control(PORT, token)
        st = c.state()
        check("a second start comes up in SMPTE", st.get("time_display") == "smpte30", st.get("time_display"))
        c.ask("SELECT_TIME 00:00:00:00 00:00:01:00")
        check("and SELECT_TIME reads SMPTE there", c.state().get("selection") == f"0-{RATE}")
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
    elif not ap.parse_args().keep:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


main()
