#!/usr/bin/env python3
"""f42 scripted run: context-sensitive channel selection, the Select actions and the Edit
menu's channel group, through the control channel on a synthetic stereo file.

    python3 tools/alpha/f42.py --exe <path to the release wavcraft>
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, SECS = 48000, 4
PORT = 47142


def make_wav(path):
    n = RATE * SECS
    with wave.open(path, "wb") as w:
        w.setnchannels(2), w.setsampwidth(2), w.setframerate(RATE)
        frames = bytearray()
        for i in range(n):
            l = int(12000 * math.sin(2 * math.pi * 220 * i / RATE)) + 3000
            r = int(-12000 * math.sin(2 * math.pi * 330 * i / RATE)) - 3000
            frames += struct.pack("<hh", l, r)
        w.writeframes(bytes(frames))


class Ctl:
    def __init__(self, token):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", PORT), timeout=30)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        assert self.ask(f"TOKEN {token}") == "OK", "token refused"

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self):
        while True:
            head = self.ask("STATE").partition(" undo=")[0]
            d = dict(kv.split("=", 1) for kv in head.split()[1:])
            if d.get("busy") == "0":
                return d
            time.sleep(0.2)


def levels(path, lo, hi):
    with wave.open(path, "rb") as w:
        w.setpos(lo)
        raw = w.readframes(hi - lo)
    vals = struct.unpack("<" + "h" * (len(raw) // 2), raw)
    return max(abs(v) for v in vals[0::2]), max(abs(v) for v in vals[1::2])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="f42.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    wav, saved, token = os.path.join(work, "in.wav"), os.path.join(work, "out.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([exe, wav], env=env, stdout=err, stderr=err)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))

    try:
        for _ in range(100):  # the token file appears when the channel starts
            if os.path.exists(token):
                break
            time.sleep(0.2)
        time.sleep(0.5)
        c = Ctl(open(token).read().strip())
        st = c.state()
        check("file open, stereo", st["channels"] == "2" and int(st["frames"]) == RATE * SECS, st)
        check("starts with both channels", st["selch"] == "LR", st["selch"])

        a = c.ask("GESTURE channel_drag 0.05 1000 5000")
        check("drag in the top strip selects the left channel", a == "OK selection=1000-5000 selch=L", a)
        a = c.ask("GESTURE channel_drag 0.95 1000 5000")
        check("drag in the bottom strip selects the right channel", a.endswith("selch=R"), a)
        a = c.ask("GESTURE channel_drag 0.5 1000 5000")
        check("drag in the middle selects both", a.endswith("selch=LR"), a)
        c.ask("GESTURE channel_drag 0.95 1000 5000")
        a = c.ask("GESTURE channel_drag -0.1 1000 5000")
        check("a drag starting on the ruler keeps the mask", a.endswith("selch=R"), a)
        c.ask("GESTURE channel_drag 0.5 1000 5000")

        # An edit respects the mask: silence the left channel only over 1 s..2 s.
        c.ask(f"GESTURE channel_drag 0.1 {RATE} {2 * RATE}")
        a = c.ask("ACTION edit.silence")
        check("Silence on a left-strip selection", a.endswith("changed=1"), a)
        c.ask(f"GESTURE channel_drag 0.9 {3 * RATE} {int(3.5 * RATE)}")
        c.ask("ACTION edit.silence")
        check("history has the two edits", c.state()["steps"] == "3", c.state())

        # Select actions.
        a = c.ask("ACTION select.all")
        check("select.all", a.startswith(f"OK selection=0-{RATE * SECS}"), a)
        a = c.ask("ACTION select.none")
        check("select.none leaves the cursor", a.startswith("OK selection=0-0"), a)
        c.ask(f"SELECT {RATE} {2 * RATE}")
        a = c.ask("ACTION select.toStart")
        check("select.toStart", a.startswith(f"OK selection=0-{RATE}"), a)
        c.ask(f"SELECT {RATE} {2 * RATE}")
        a = c.ask("ACTION select.toEnd")
        check("select.toEnd", a.startswith(f"OK selection={2 * RATE}-{RATE * SECS}"), a)
        a = c.ask("ACTION select.currentView")
        check("select.currentView is the whole file when fully zoomed out", a.startswith(f"OK selection=0-{RATE * SECS}"), a)
        c.ask("ACTION edit.bothChannels")
        a = c.ask("ACTION edit.rightChannel")
        check("Right Channel Only", c.state()["selch"] == "R", a)
        a = c.ask("ACTION edit.bothChannels")
        check("Both Channels", c.state()["selch"] == "LR", a)

        # The saved file shows the masks: left silent in 1-2 s and untouched in 3-3.5 s, right the reverse.
        a = c.ask(f"SAVE {saved}")
        c.state()
        l1, r1 = levels(saved, RATE + 1000, 2 * RATE - 1000)
        l2, r2 = levels(saved, 3 * RATE + 1000, int(3.5 * RATE) - 1000)
        check("left channel silent in the left-strip range, right intact", l1 == 0 and r1 > 1000, (l1, r1))
        check("right channel silent in the bottom-strip range, left intact", r2 == 0 and l2 > 1000, (l2, r2))
        a = c.ask("QUIT")
        check("QUIT", a == "OK", a)
        proc.wait(30)
    except Exception as e:
        check("script ran to the end", False, f"{type(e).__name__}: {e}")
        proc.kill()
    log_lines = [l for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log" for l in open(os.path.join(d, f), errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR line", not log_lines, log_lines[:3])
    for label, ok, detail in checks:
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"))
    bad = [c for c in checks if not c[1]]
    shutil.rmtree(work, ignore_errors=True)
    print("f42 scripted run:", "FAIL" if bad else "PASS")
    sys.exit(1 if bad else 0)


main()
