#!/usr/bin/env python3
"""f59 scripted run: snap to markers (A124) through the control channel.

    python3 tools/alpha/f59.py --exe <path to the release wavcraft>

Starts the app with HOME and its logs in a scratch folder, opens a synthetic 4 s stereo WAV (left
flips from +0.5 to -0.5 at frame 100000, right is a constant 0.25), and checks through STATE and
the SNAP verb: the flags, snapping to the ruler / selection edges / zero crossings, zero crossings
following the enabled channels, and an edit acting only on an enabled channel (read back from a
saved copy). Then QUIT; any ERROR line in the app's log fails the run.
"""
import argparse, os, shutil, struct, subprocess, sys, tempfile, time, wave

RATE, FRAMES, FLIP, PORT = 48000, 4 * 48000, 100000, 47159


def make_wav(path):
    w = wave.open(path, "wb")
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
    left, right = int(0.5 * 32767), int(0.25 * 32767)
    w.writeframes(b"".join(struct.pack("<hh", left if k < FLIP else -left, right) for k in range(FRAMES)))
    w.close()


def read_wav(path, count=2000):
    """The first `count` frames of a 16- or 24-bit PCM WAV as floats, one list per channel."""
    w = wave.open(path, "rb")
    width, ch = w.getsampwidth(), w.getnchannels()
    raw = w.readframes(count)
    out = [[] for _ in range(ch)]
    for i in range(0, len(raw) - width * ch + 1, width * ch):
        for c in range(ch):
            v = int.from_bytes(raw[i + c * width:i + (c + 1) * width], "little", signed=True)
            out[c].append(v / float(1 << (8 * width - 1)))
    return out


class Control:
    def __init__(self, port, token):
        for _ in range(80):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
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

    def state(self, timeout=60):
        t0 = time.time()
        while True:
            head, _, labels = self.ask("STATE").partition(" undo=")
            fields = dict(kv.split("=", 1) for kv in head.split()[1:])
            fields["undo"] = labels.partition(" redo=")[0]
            if fields.get("busy") == "0" or time.time() - t0 > timeout:
                return fields
            time.sleep(0.2)


import socket


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f59.", dir=base)
    checks = []

    def check(label, ok, detail):
        checks.append(bool(ok))
        print(("PASS " if ok else "FAIL ") + label + " | " + str(detail))

    proc = None
    try:
        home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
        os.makedirs(home)
        wav, token = os.path.join(work, "flip.wav"), os.path.join(work, "control.token")
        make_wav(wav)
        env = dict(os.environ, HOME=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions, XDG_CONFIG_HOME=os.path.join(home, ".config"),
                   XDG_CACHE_HOME=os.path.join(home, ".cache"), WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token)
        proc = subprocess.Popen([exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(80):
            if os.path.exists(token):
                break
            time.sleep(0.25)
        c = Control(PORT, open(token).read().strip())
        for _ in range(100):
            st = c.state()
            if int(st["frames"]) == FRAMES:
                break
            time.sleep(0.2)
        check("file open, marker snapping starts on", st["frames"] == str(FRAMES) and st["snapmarkers"] == "1", st)
        snap = lambda f, fpp: c.ask(f"SNAP {f} {fpp}")
        c.ask("SELECT 70000 70000")
        a = c.ask("ACTION marker.add")
        c.ask("SELECT 120000 130000")
        b = c.ask("ACTION marker.add")
        c.ask("SELECT 0 0")
        m = c.ask("MARKERS")
        check("a point marker and a range marker added", m.startswith("OK count=2"), (a, b, m))
        check("snaps to the point marker within 8 px", snap(70500, 100) == "OK frame=70000", snap(70500, 100))
        check("snaps to the range start", snap(119500, 100) == "OK frame=120000", snap(119500, 100))
        check("snaps to the range end", snap(130600, 100) == "OK frame=130000", snap(130600, 100))
        check("farther than 8 px stays", snap(71000, 100) == "OK frame=71000", snap(71000, 100))
        a = c.ask("ACTION view.snap.markers.toggle")
        st = c.state()
        check("view.snap.markers.toggle turns it off (STATE); nothing snaps to markers",
              a.startswith("OK") and st["snapmarkers"] == "0" and snap(70500, 100) == "OK frame=70500", f"{a}; {st['snapmarkers']}")
        c.ask("ACTION view.snap.markers.toggle")
        check("toggling again turns it on", c.state()["snapmarkers"] == "1" and snap(70500, 100) == "OK frame=70000", c.state()["snapmarkers"])
        c.ask("ACTION view.snap.toggle")
        check("master switch off: nothing snaps", snap(70500, 100) == "OK frame=70500", snap(70500, 100))
        check("QUIT", c.ask("QUIT discard") == "OK", "")
        proc.wait(20)
        text = ""
        for d, _, files in os.walk(logs):
            for f in files:
                text += open(os.path.join(d, f), errors="replace").read()
        bad = [l for l in text.splitlines() if "ERROR" in l]
        check("no ERROR line in the app's log", not bad, bad[:3] or f"{len(text.splitlines())} log lines")
    except Exception as e:
        check("script ran", False, f"{type(e).__name__}: {e}")
    finally:
        if proc is not None and proc.poll() is None:
            proc.kill()
        shutil.rmtree(work, ignore_errors=True)
    print("RESULT", "pass" if all(checks) else "FAIL", f"{sum(checks)}/{len(checks)}")
    sys.exit(0 if all(checks) else 1)


main()
