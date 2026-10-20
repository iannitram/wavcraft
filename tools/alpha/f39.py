#!/usr/bin/env python3
"""Alpha check for f39 (Markers panel verbs: MARKER rename / move / select, with undo): drives the release app through the control channel.

    python3 tools/alpha/f39.py --exe <path to wavcraft>

A synthetic 2 s stereo WAV is opened in a scratch HOME; four markers are added, then renamed,
moved and picked through the MARKER verbs, each rename/move checked as exactly one undo step;
QUIT, and the app's log must hold no ERROR line.
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave, math

RATE = 48000
PORT = 47139


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

    def full_state(self):
        d = dict(kv.split("=", 1) for kv in self.ask("STATE").split(" undo=")[0].split()[1:])
        return d

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
    work = tempfile.mkdtemp(prefix="f39.", dir=scratch_root)
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
        for sel in ("10000 10000", "30000 40000", "60000 60000", "80000 90000"):
            c.ask("SELECT " + sel); c.ask("ACTION marker.add")
        pos = lambda: [(s, e) for _, s, e, _ in c.markers()[1]]
        names = lambda: [n for *_, n in c.markers()[1]]
        steps = lambda: int(c.full_state()["steps"])
        check("four markers listed", pos() == [(10000, 10000), (30000, 40000), (60000, 60000), (80000, 90000)], pos())
        check("nothing picked yet", c.full_state()["msel"] == "-", c.full_state())
        # Rename: one step, undoable; the name may hold spaces.
        n = steps()
        check("rename", c.ask("MARKER rename 1 Big Chorus") == "OK")
        check("the name changed, nothing moved", names()[1] == "Big Chorus" and len(pos()) == 4, names())
        check("rename is one history step", steps() == n + 1 and "undo=Rename Marker" in c.ask("STATE"), c.ask("STATE"))
        c.ask("ACTION edit.undo")
        check("undo restores the name", names()[1] != "Big Chorus" and steps() == n + 1, names())
        c.ask("ACTION edit.redo")
        check("rename refuses an empty or unchanged name", c.ask("MARKER rename 1 Big Chorus").startswith("ERR") and c.ask("MARKER rename 9 x").startswith("ERR"))
        # Move: keeps a range's length, reorders the list, one step.
        n = steps()
        check("move a range", c.ask("MARKER move 1 70000") == "OK")
        check("moved with its length, list re-ordered", pos() == [(10000, 10000), (60000, 60000), (70000, 80000), (80000, 90000)], pos())
        check("the marker kept its name through the move", names()[2] == "Big Chorus", names())
        check("move is one history step", steps() == n + 1 and "undo=Move Marker" in c.ask("STATE"))
        c.ask("ACTION edit.undo")
        check("undo restores the position", pos()[1] == (30000, 40000), pos())
        c.ask("ACTION edit.redo")
        c.ask("ACTION edit.undo")
        check("move refuses a place another marker holds", c.ask("MARKER move 0 60000").startswith("ERR"))
        check("move held inside the file", c.ask("MARKER move 3 9999999") == "OK" and pos()[-1] == (2 * RATE - 10000, 2 * RATE), pos())
        c.ask("ACTION edit.undo")
        # Select: picks, sets the selection to the span; add extends.
        check("select a range marker", c.ask("MARKER select 1").startswith("OK selection=30000-40000"), c.ask("STATE"))
        check("selection and pick reported", c.full_state()["selection"] == "30000-40000" and c.full_state()["msel"] == "1")
        check("select a point sets the cursor", c.ask("MARKER select 2").startswith("OK selection=60000-60000") and c.full_state()["msel"] == "2")
        a = c.ask("MARKER select 0 add")
        check("add extends the pick and the span", a.startswith("OK selection=10000-60000") and c.full_state()["msel"] == "0,2", (a, c.full_state()["msel"]))
        a = c.ask("MARKER select 0 add")
        check("add again toggles it off", c.full_state()["msel"] == "2", c.full_state()["msel"])
        check("select out of range is refused", c.ask("MARKER select 9").startswith("ERR"))
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
