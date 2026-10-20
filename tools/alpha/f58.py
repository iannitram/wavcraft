#!/usr/bin/env python3
"""Alpha check for f58 (merge markers, colour, description, AIFF MARK): drives the release app through the control channel.

    python3 tools/alpha/f58.py --exe <path to wavcraft>

A synthetic 2 s WAV is opened in a scratch HOME; four markers are added and given names, colours and
descriptions through the MARKER verbs; two are merged (one undo step, undone and redone); the file is
saved and reopened (the merged range and the description must survive, the colour is not stored);
then a synthetic AIFF with a MARK chunk (one point, one range as a "[name" / "]name" pair) is opened.
QUIT, and the app's log must hold no ERROR line.
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave, math

RATE = 48000
PORT = 47158


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


def make_aiff(path):
    frames = 2 * RATE
    def pstr(t):
        b = t.encode(); out = bytes([len(b)]) + b
        return out + (b"\0" if len(out) & 1 else b"")
    marks = [(1, 5000, "Hook"), (2, 20000, "[Verse"), (3, 50000, "]Verse")]
    mark = struct.pack(">H", len(marks)) + b"".join(struct.pack(">hI", i, p) + pstr(n) for i, p, n in marks)
    comm = struct.pack(">hIh", 1, frames, 16) + struct.pack(">HQ", 0x400E, 48000 << 48)
    ssnd = struct.pack(">II", 0, 0) + b"".join(struct.pack(">h", int(8000 * math.sin(i * 0.05))) for i in range(frames))
    chunk = lambda cid, body: cid + struct.pack(">I", len(body)) + body + (b"\0" if len(body) & 1 else b"")
    body = b"AIFF" + chunk(b"COMM", comm) + chunk(b"MARK", mark) + chunk(b"SSND", ssnd)
    open(path, "wb").write(b"FORM" + struct.pack(">I", len(body)) + body)


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch"); os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f58.", dir=scratch_root)
    checks = []
    def check(label, ok, detail=""):
        checks.append(ok); print(("PASS " if ok else "FAIL ") + label + ("" if ok else f": {detail}"))
    proc = None
    try:
        home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
        os.makedirs(home)
        wav = os.path.join(work, "tone.wav"); make_wav(wav)
        aiff = os.path.join(work, "tone.aiff"); make_aiff(aiff)
        saved = os.path.join(work, "saved.wav")
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
        info = lambda i: c.ask(f"MARKER info {i}")
        for i, n in enumerate(("Intro", "Chorus", "Bridge", "Outro")):
            c.ask(f"MARKER rename {i} {n}")
        # Colour and description: one step each, undoable.
        n = steps()
        check("description", c.ask("MARKER desc 0 Count in, then drums") == "OK" and c.ask("MARKER desc 1 the big chorus") == "OK")
        check("colour", c.ask("MARKER color 0 3") == "OK" and c.ask("MARKER color 3 6") == "OK")
        check("each is one step", steps() == n + 4 and "undo=Marker Colour" in c.ask("STATE"), c.ask("STATE"))
        check("info shows them", info(0) == "OK color=3 desc=Count in, then drums" and info(2) == "OK color=0 desc=", (info(0), info(2)))
        check("colour 0 clears, 7 and an unchanged value are refused", c.ask("MARKER color 3 7").startswith("ERR") and c.ask("MARKER color 3 6").startswith("ERR") and c.ask("MARKER color 3 0") == "OK" and info(3).startswith("OK color=0"))
        c.ask("MARKER color 3 6")
        # Merge: Intro (point) + Chorus (range) -> one range, names joined, one step, undo/redo.
        n = steps()
        check("merge", c.ask("MARKER merge 1 0") == "OK")
        check("one range from the earliest start to the latest end", pos() == [(10000, 40000), (60000, 60000), (80000, 90000)], pos())
        check("names and descriptions joined, first colour kept", names()[0] == "Intro + Chorus" and info(0) == "OK color=3 desc=Count in, then drums / the big chorus", (names(), info(0)))
        check("merge is one history step", steps() == n + 1 and "undo=Merge Markers" in c.ask("STATE"), c.ask("STATE"))
        c.ask("ACTION edit.undo")
        check("undo restores four markers", len(pos()) == 4 and names()[:2] == ["Intro", "Chorus"], names())
        c.ask("ACTION edit.redo")
        check("redo merges again", len(pos()) == 3 and names()[0] == "Intro + Chorus")
        check("merge refuses one marker, a bad index, no markers", c.ask("MARKER merge 1").startswith("ERR") and c.ask("MARKER merge 1 9").startswith("ERR") and c.ask("MARKER merge").startswith("ERR"))
        # Save as WAV and reopen: the merged range, its name and its description come back.
        check("save", c.ask(f"SAVE {saved}") == "OK")
        c.state()
        check("saved file exists", os.path.getsize(saved) > 2 * RATE * 4 if os.path.exists(saved) else False)
        check("open the saved file", c.ask(f"OPEN {saved}") == "OK")
        st = c.state()
        while int(st["frames"]) != 2 * RATE:
            time.sleep(0.2); st = c.state()
        check("markers survive the round trip", pos() == [(10000, 40000), (60000, 60000), (80000, 90000)] and names()[0] == "Intro + Chorus", (pos(), names()))
        check("description survives, colour is not stored", info(0) == "OK color=0 desc=Count in, then drums / the big chorus", info(0))
        # AIFF: a point and a range pair from a file written by this script (not by the app).
        check("open an AIFF with a MARK chunk", c.ask(f"OPEN {aiff}") == "OK")
        st = c.state()
        while int(st["frames"]) != 2 * RATE:
            time.sleep(0.2); st = c.state()
        check("AIFF markers read: a point and a range", c.markers()[1] == [(1, 5000, 5000, "Hook"), (2, 20000, 50000, "Verse")] or [(s, e, n) for _, s, e, n in c.markers()[1]] == [(5000, 5000, "Hook"), (20000, 50000, "Verse")], c.markers())
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
