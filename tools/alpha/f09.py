#!/usr/bin/env python3
"""f09 scripted run: snapping (A123, A127) and channel enable (A132) through the control channel.

    python3 tools/alpha/f09.py --exe <path to the release wavcraft>

Starts the app with HOME and its logs in a scratch folder, opens a synthetic 4 s stereo WAV (left
flips from +0.5 to -0.5 at frame 100000, right is a constant 0.25), and checks through STATE and
the SNAP verb: the flags, snapping to the ruler / selection edges / zero crossings, zero crossings
following the enabled channels, and an edit acting only on an enabled channel (read back from a
saved copy). Then QUIT; any ERROR line in the app's log fails the run.
"""
import argparse, os, shutil, struct, subprocess, sys, tempfile, time, wave

RATE, FRAMES, FLIP, PORT = 48000, 4 * 48000, 100000, 47109


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
    work = tempfile.mkdtemp(prefix="f09.", dir=base)
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
        check("file open, snapping starts on, zero crossings off, both channels enabled",
              st["frames"] == str(FRAMES) and (st["snap"], st["snapzero"], st["enabled"]) == ("1", "0", "11"), st)
        snap = lambda f, fpp: c.ask(f"SNAP {f} {fpp}")
        check("snaps to the ruler tick (0.5 s) within 8 px", snap(24300, 100) == "OK frame=24000", snap(24300, 100))
        check("farther than 8 px stays", snap(23000, 100) == "OK frame=23000", snap(23000, 100))
        c.ask("SELECT 50000 60000")
        check("snaps to a selection edge", snap(50300, 100) == "OK frame=50000", snap(50300, 100))
        c.ask("SELECT 0 0")
        check("zero crossings off: 100005 stays", snap(FLIP + 5, 2) == f"OK frame={FLIP + 5}", snap(FLIP + 5, 2))
        a = c.ask("ACTION view.snap.zero.toggle")
        st = c.state()
        check("snap.zero.toggle turns it on (STATE)", a.startswith("OK") and st["snapzero"] == "1", f"{a}; {st['snapzero']}")
        check("zero crossings on: 100005 snaps to the sign change", snap(FLIP + 5, 2) == f"OK frame={FLIP}", snap(FLIP + 5, 2))
        a = c.ask("ACTION channel.toggle.0")
        st = c.state()
        check("channel.toggle.0 disables the left channel (STATE)", a.startswith("OK") and st["enabled"] == "01", f"{a}; {st['enabled']}")
        check("with only the right channel enabled there is no zero crossing", snap(FLIP + 5, 2) == f"OK frame={FLIP + 5}", snap(FLIP + 5, 2))
        a = c.ask("ACTION channel.toggle.1")
        check("the last enabled channel cannot be disabled", c.state()["enabled"] == "01", a)
        c.ask("SELECT 0 1000")
        c.ask("ACTION edit.silence")
        saved = os.path.join(work, "saved.wav")
        c.ask(f"SAVE {saved}")
        c.state()
        left, right = read_wav(saved)
        check("the unselected part of the right channel is untouched", abs(right[1500] - 0.25) < 1e-3, right[1500])
        check("Silence acted on the enabled (right) channel only", max(abs(v) for v in right[300:700]) < 1e-4 and min(left[:1000]) > 0.4,
              (left[300], right[300], right[1500]))
        c.ask("ACTION channel.toggle.0")
        check("toggling again enables both", c.state()["enabled"] == "11", c.state()["enabled"])
        a = c.ask("ACTION view.snap.toggle")
        st = c.state()
        check("view.snap.toggle turns snapping off: nothing snaps", st["snap"] == "0" and snap(24300, 100) == "OK frame=24300", f"{a}; {st['snap']}")
        c.ask("ACTION channel.toggle.9")
        check("a channel the file does not have changes nothing", c.state()["enabled"] == "11", c.state()["enabled"])
        check("QUIT", c.ask("QUIT") == "OK", "")
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
