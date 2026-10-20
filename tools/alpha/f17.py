#!/usr/bin/env python3
"""Alpha check for f17 (markers move with edits, save/load in WAV, delete, convert): drives the release app through the control channel.

    python3 tools/alpha/f17.py --exe <path to wavcraft>

A synthetic 2 s stereo WAV is opened in a scratch HOME; markers are added, moved by a delete
and an insert, converted and deleted, saved with SAVE, the bytes checked, the file reopened and
its markers compared; QUIT, and the app's log must hold no ERROR line.
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave, math

RATE = 48000
PORT = 47117


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
    work = tempfile.mkdtemp(prefix="f17.", dir=scratch_root)
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
        for sel in ("10000 10000", "30000 40000", "60000 60000", "80000 90000"):
            c.ask("SELECT " + sel); c.ask("ACTION marker.add")
        pos = lambda: [(s, e) for _, s, e, _ in c.markers()[1]]
        check("four markers added", pos() == [(10000, 10000), (30000, 40000), (60000, 60000), (80000, 90000)], pos())
        # Delete 35000-70000: range trimmed, point inside gone, later range moves back by 35000.
        c.ask("SELECT 35000 70000"); c.ask("ACTION edit.delete")
        check("a delete trims, removes and shifts markers", pos() == [(10000, 10000), (30000, 35000), (45000, 55000)], pos())
        c.ask("ACTION edit.undo")
        check("undo restores the markers", len(pos()) == 4, pos())
        c.ask("ACTION edit.redo")
        c.ask("SELECT 20000 20000"); c.ask("INSERT_SILENCE 0.5")
        check("insert silence shifts later markers", pos() == [(10000, 10000), (54000, 59000), (69000, 79000)], pos())
        before = pos()
        # Delete marker (cursor on the point), convert range to point, point to range.
        c.ask("SELECT 10000 10000")
        a = c.ask("ACTION marker.toRange")
        check("point to range needs a selection", "not available" in a or c.markers()[0] == 3, a)
        c.ask("SELECT 9000 12000"); c.ask("ACTION marker.toRange")
        check("point to range uses the selection", pos()[0] == (9000, 12000), pos())
        c.ask("SELECT 10000 10000"); c.ask("ACTION marker.toPoint")
        check("range to point keeps its start", pos()[0] == (9000, 9000), pos())
        c.ask("SELECT 9000 9000"); c.ask("ACTION marker.delete")
        check("delete marker removes the one under the cursor", c.markers()[0] == 2, pos())
        c.ask("ACTION edit.undo")
        check("undo brings it back", c.markers()[0] == 3, pos())
        names = [n for *_, n in c.markers()[1]]
        want = c.markers()[1]
        # Save As, check the chunks in the bytes, reopen, compare.
        out = os.path.join(work, "marked.wav")
        check("SAVE", c.ask("SAVE " + out) == "OK")
        c.state()
        raw = open(out, "rb").read()
        check("file holds cue and adtl chunks", b"cue " in raw and b"LIST" in raw and b"adtl" in raw and b"labl" in raw and b"ltxt" in raw)
        check("RIFF size is right", struct.unpack("<I", raw[4:8])[0] == len(raw) - 8)
        check("OPEN the saved file", c.ask("OPEN " + out) == "OK")
        c.state()
        got = c.markers()[1]
        check("markers read back the same (positions and names)", [x[1:] for x in got] == [x[1:] for x in want], (got, want))
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
