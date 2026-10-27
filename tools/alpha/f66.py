#!/usr/bin/env python3
"""f66 alpha (time entry): through the control channel, on a synthetic 44.1 kHz 10 s file in a
scratch HOME, in the decimal and the SMPTE 30 fps format: GOTO <time> moves the cursor (checked in
STATE's cursor and cursor_text), a time past the end is clamped, a time that does not read is
refused, INSERT_SILENCE_FMT <length> inserts a length written in the format (checked in frames),
and ACTION view.gotoTime is accepted. The app's log has no ERROR line.

    python3 tools/alpha/f66.py --exe <path to the release wavcraft>
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave, math, struct

RATE = 44100
PORT = 47166

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
    args = ap.parse_args()
    exe = args.exe
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f66.", dir=scratch_root)
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
    total = 10 * RATE
    try:
        c = Control(PORT, token)
        st = c.state()
        check("the file opens in decimal", st.get("frames") == str(total) and st.get("time_display") == "decimal", st)

        # Decimal.
        ans = c.ask("GOTO 0:02.500")
        st = c.state()
        check("decimal: GOTO 0:02.500 puts the cursor at frame 110250", ans.startswith("OK") and st.get("cursor_frame") == "110250" and st.get("cursor_text") == "0:02.500", f"{ans} {st.get('cursor')} {st.get('cursor_text')}")
        ans = c.ask("GOTO 90")
        check("decimal: a time past the end (90 s) is clamped to the end", ans.startswith("OK") and c.state().get("cursor_frame") == str(total), ans)
        bad = c.ask("GOTO 1:75.0")
        check("decimal: a time that does not read is refused", bad.startswith("ERR"), bad)
        check("the refusal left the cursor alone", c.state().get("cursor_frame") == str(total))
        c.ask("GOTO 0:01.000")
        ans = c.ask("INSERT_SILENCE_FMT 0:00.500")
        st = c.state()
        check("decimal: INSERT_SILENCE_FMT 0:00.500 adds 22050 frames", ans.startswith("OK") and st.get("frames") == str(total + 22050), f"{ans} frames={st.get('frames')}")
        bad = c.ask("INSERT_SILENCE_FMT 0:00")
        check("decimal: a zero length is refused", bad.startswith("ERR"), bad)
        total += 22050

        # SMPTE 30 fps.
        c.ask("ACTION view.time_display.smpte_30")
        ans = c.ask("GOTO 00:00:01:15")
        st = c.state()
        check("SMPTE: GOTO 00:00:01:15 puts the cursor at frame 66150", ans.startswith("OK") and st.get("cursor_frame") == "66150" and st.get("cursor_text") == "00:00:01:15", f"{ans} {st.get('cursor')} {st.get('cursor_text')}")
        bad = c.ask("GOTO 00:00:01:30")
        check("SMPTE: frame 30 is refused", bad.startswith("ERR"), bad)
        ans = c.ask("INSERT_SILENCE_FMT 00:00:00:15")
        st = c.state()
        check("SMPTE: INSERT_SILENCE_FMT 00:00:00:15 adds 22050 frames", ans.startswith("OK") and st.get("frames") == str(total + 22050), f"{ans} frames={st.get('frames')}")

        # Samples.
        c.ask("ACTION view.time_display.samples")
        c.ask("GOTO 1,000")
        check("samples: GOTO 1,000 is frame 1000", c.state().get("cursor_frame") == "1000" and c.state().get("cursor_text") == "1000")
        ans = c.ask("ACTION view.gotoTime")
        check("the Go to Time action is accepted", ans.startswith("OK"), ans)
        check("QUIT", c.ask("QUIT discard") == "OK")
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
