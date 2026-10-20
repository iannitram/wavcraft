#!/usr/bin/env python3
"""f106 alpha (metronome): through the control channel, on a synthetic 44.1 kHz file in a scratch
HOME: METRONOME clicks returns the click frames for 120 bpm 4/4 and 90 bpm 3/4 (accent flags too),
METRONOME render runs the player's own mixer over silence and its click onsets must equal them,
METRONOME on/off/countin/volume are kept and shown in STATE and bad values refused; the app's log
has no ERROR line. No sound device is needed (render mixes into a buffer, not the speakers).

    python3 tools/alpha/f106.py --exe <path to the release wavcraft>
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave, math, struct

RATE = 44100
PORT = 47206


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


def fields(ans):
    return dict(kv.split("=", 1) for kv in ans.split()[1:] if "=" in kv)


def ints(text):
    return [int(x) for x in text.split(",") if x != ""]


def make_wav(path, seconds=10):
    with wave.open(path, "wb") as w:
        w.setnchannels(2), w.setsampwidth(2), w.setframerate(RATE)
        frames = b"".join(struct.pack("<hh", int(8000 * math.sin(i * 0.05)), int(8000 * math.sin(i * 0.07))) for i in range(RATE))
        for _ in range(seconds):
            w.writeframes(frames)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f106.", dir=scratch_root)
    home, logs = os.path.join(work, "home"), os.path.join(work, "logs")
    os.makedirs(home)
    wav, token = os.path.join(work, "tone.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    env = dict(os.environ, HOME=home, USERPROFILE=home, XDG_CONFIG_HOME=os.path.join(home, ".config"), WAVCRAFT_LOG_DIR=logs,
               WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))

    proc = subprocess.Popen([args.exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        c = Control(PORT, token)
        st = c.state()
        check("the file opens; metronome off, no count-in", st.get("frames") == str(10 * RATE) and st.get("metronome") == "0" and st.get("countin") == "0", st)
        # 120 bpm 4/4 at 44.1 kHz: a click every 22050 frames, the accent every 88200.
        a = fields(c.ask("METRONOME clicks 0 4"))
        want = [k * 22050 for k in range(8)]
        check("120 bpm 4/4: clicks 0-4 s at 22050 frame steps", ints(a.get("frames", "")) == want and a.get("n") == "8", a)
        check("accent on bar 1 only (beats 0 and 4)", ints(a.get("accents", "")) == [1, 0, 0, 0, 1, 0, 0, 0], a)
        # the mixer itself (over silence): metronome must be on to sound
        r = fields(c.ask("METRONOME render 0 4"))
        check("metronome off: the mixer makes no click", r.get("n") == "0", r)
        check("METRONOME on", c.ask("METRONOME on").startswith("OK metronome=1") and c.state().get("metronome") == "1")
        r = fields(c.ask("METRONOME render 0 4"))
        check("the mixer's click onsets equal the beat frames", ints(r.get("frames", "")) == want, r)
        r = fields(c.ask("METRONOME render 1 3"))
        check("a window [1 s, 3 s) starts on the first beat inside it", ints(r.get("frames", "")) == [44100, 66150, 88200, 110250], r)
        check("PREF tempo 90 / meter 3/4", c.ask("PREF set tempo 90") == "OK tempo=90" and c.ask("PREF set meter 3/4") == "OK meter=3/4")
        a = fields(c.ask("METRONOME clicks 0 4"))
        check("90 bpm 3/4: clicks every 29400 frames, accent on beats 0 and 3",
              ints(a.get("frames", "")) == [0, 29400, 58800, 88200, 117600, 147000] and ints(a.get("accents", "")) == [1, 0, 0, 1, 0, 0], a)
        r = fields(c.ask("METRONOME render 0 4"))
        check("the mixer follows the new tempo and meter", ints(r.get("frames", "")) == [0, 29400, 58800, 88200, 117600, 147000], r)
        check("METRONOME countin 2", c.ask("METRONOME countin 2").startswith("OK") and c.state().get("countin") == "2")
        check("countin 99 refused", c.ask("METRONOME countin 99").startswith("ERR") and c.state().get("countin") == "2")
        check("volume 0.25 kept; 3 refused", c.ask("METRONOME volume 0.25").endswith("volume=0.25") and c.ask("METRONOME volume 3").startswith("ERR"))
        check("a bad range is refused", c.ask("METRONOME clicks 5 1").startswith("ERR") and c.ask("METRONOME bogus").startswith("ERR"))
        check("METRONOME off", c.ask("METRONOME off").startswith("OK metronome=0") and c.state().get("metronome") == "0")
        r = fields(c.ask("METRONOME render 0 4"))
        check("off again: no click in the mix", r.get("n") == "0", r)
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
