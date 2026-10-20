#!/usr/bin/env python3
"""Alpha check for f07 (markers): drives the release app through the control channel.

    python3 tools/alpha/f07.py --exe <path to wavcraft>

A synthetic 2 s stereo WAV is opened in a scratch HOME; markers are added (a point at the
cursor, a range from a selection), listed with MARKERS, navigated, undone and redone; QUIT, and
the app's log must hold no ERROR line.
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave, math

RATE = 48000
PORT = 47107


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<hh", int(8000 * math.sin(i * 0.05)), int(8000 * math.sin(i * 0.031))) for i in range(2 * RATE)))


class Control:
    def __init__(self, port, token):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
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

    def state(self):
        while True:
            head = self.ask("STATE").split(" undo=")[0]
            d = dict(kv.split("=", 1) for kv in head.split()[1:])
            if d.get("busy") == "0":
                return d
            time.sleep(0.2)

    def markers(self):
        ans = self.ask("MARKERS")
        head, _, rest = ans.partition(" ")
        count = int(rest.split(" ", 1)[0].split("=")[1]) if head == "OK" else -1
        body = rest.split(" ", 1)[1] if " " in rest else ""
        items = [tuple(x.split(",", 3)) for x in body.split(";") if x]
        return count, [(int(a), int(b), int(c), n) for a, b, c, n in items]


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch"); os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f07.", dir=scratch_root)
    checks = []
    def check(label, ok, detail=""):
        checks.append(ok); print(("PASS " if ok else "FAIL ") + label + ("" if ok else f": {detail}"))
    proc = None
    try:
        home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
        os.makedirs(home)
        wav = os.path.join(work, "tone.wav"); make_wav(wav)
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
                   WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
        err = open(os.path.join(work, "stderr.log"), "w")
        proc = subprocess.Popen([exe, wav], env=env, stdout=err, stderr=err)
        for _ in range(120):
            if os.path.exists(token): break
            time.sleep(0.5)
        c = Control(PORT, open(token).read().strip())
        st = c.state()
        while int(st["frames"]) != 2 * RATE:
            time.sleep(0.2); st = c.state()
        check("starts with no markers", c.markers() == (0, []), c.markers())
        c.ask("SELECT 12000 12000")
        a = c.ask("ACTION marker.add")
        check("Add Marker at the cursor is a point", a.startswith("OK") and c.markers()[1][0][1:3] == (12000, 12000), a)
        c.ask("SELECT 48000 72000")
        c.ask("ACTION marker.add")
        count, items = c.markers()
        check("Add Marker over a selection is a range, listed in order",
              count == 2 and [(s, e) for _, s, e, _ in items] == [(12000, 12000), (48000, 72000)] and items[0][3] != items[1][3], items)
        c.ask("ACTION marker.add")
        check("the same range again adds nothing", c.markers()[0] == 2)
        st = c.state()
        check("a marker is a History step with the audio unchanged", st["steps"] == "3" and st["frames"] == str(2 * RATE), st)
        c.ask("SELECT 0 0")
        a = c.ask("ACTION marker.next"); check("next marker from the start", a.startswith("OK selection=12000-12000"), a)
        a = c.ask("ACTION marker.next"); check("next marker again", a.startswith("OK selection=48000-48000"), a)
        a = c.ask("ACTION marker.next"); check("no marker after the last: the cursor stays", a.startswith("OK selection=48000-48000"), a)
        a = c.ask("ACTION marker.prev"); check("previous marker", a.startswith("OK selection=12000-12000"), a)
        a = c.ask("ACTION marker.prev"); check("no marker before the first: the cursor stays", a.startswith("OK selection=12000-12000"), a)
        c.ask("ACTION edit.undo")
        check("Undo takes the last marker away", c.markers()[0] == 1, c.markers())
        c.ask("ACTION edit.redo")
        check("Redo brings it back", c.markers()[0] == 2, c.markers())
        c.ask("SELECT 100000 2000")
        c.ask("ACTION edit.delete")
        c.ask("ACTION edit.undo")
        check("markers survive an edit and its undo", c.markers()[0] == 2, c.markers())
        check("QUIT", c.ask("QUIT discard") == "OK")
        proc.wait(30)
        log_text = ""
        for d, _, files in os.walk(logs):
            for f in files:
                log_text += open(os.path.join(d, f), errors="replace").read()
        errors = [l for l in log_text.splitlines() if "ERROR" in l]
        check("no ERROR line in the app's log", not errors, errors[:3])
    finally:
        if proc and proc.poll() is None:
            proc.kill()
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if checks and all(checks) else 1)


main()
