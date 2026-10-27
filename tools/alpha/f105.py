#!/usr/bin/env python3
"""Alpha check for f105 (Playlist): drives the release app through the control channel.

    python3 tools/alpha/f105.py --exe <path to wavcraft>

A synthetic 2 s stereo WAV is opened in a scratch HOME; three range markers and a point marker are
added; PLAYLIST add / loops / move / remove / sequence / play / state are checked (the sequence of
frame ranges, in order, with loops; the run hands the same ranges to the player); QUIT, and the
app's log must hold no ERROR line.
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave, math

RATE = 48000
PORT = 47205


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
    work = tempfile.mkdtemp(prefix="f105.", dir=scratch_root)
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
        # Markers 0..3 by position: A 10000-12000, B 30000-31000, P (point) 50000, C 60000-63000.
        for sel in ("10000 12000", "30000 31000", "50000 50000", "60000 63000"):
            c.ask("SELECT " + sel); c.ask("ACTION marker.add")
        check("four markers", c.markers()[0] == 4, c.markers())
        seq = lambda: c.ask("PLAYLIST sequence").rstrip()
        check("starts empty", seq() == "OK count=0", seq())
        check("play refuses an empty playlist", c.ask("PLAYLIST play").startswith("ERR"))
        check("add B x2", c.ask("PLAYLIST add 1 loops=2") == "OK items=1")
        check("add A (one loop)", c.ask("PLAYLIST add 0") == "OK items=2")
        check("add C x3", c.ask("PLAYLIST add 3 loops=3") == "OK items=3")
        check("a point marker is refused", c.ask("PLAYLIST add 2").startswith("ERR"))
        check("an unknown marker is refused", c.ask("PLAYLIST add 9").startswith("ERR"))
        B, A, C = "30000-31000", "10000-12000", "60000-63000"
        want = [B, B, A, C, C, C]
        check("sequence: B B A C C C", seq() == f"OK count=6 {';'.join(want)}", seq())
        check("move C to the front", c.ask("PLAYLIST move 2 0") == "OK")
        want = [C, C, C, B, B, A]
        check("sequence after move", seq() == f"OK count=6 {';'.join(want)}", seq())
        check("move out of range refused", c.ask("PLAYLIST move 0 5").startswith("ERR"))
        check("set loops of A to 2", c.ask("PLAYLIST loops 2 2") == "OK")
        want = [C, C, C, B, B, A, A]
        check("sequence after loops", seq() == f"OK count=7 {';'.join(want)}", seq())
        check("state lists items", c.ask("PLAYLIST state").startswith("OK items=3 list=3x3;1x2;0x2 running=0"), c.ask("PLAYLIST state"))
        check("remove the middle item", c.ask("PLAYLIST remove 1") == "OK")
        want = [C, C, C, A, A]
        check("sequence after remove", seq() == f"OK count=5 {';'.join(want)}", seq())
        # Play: the runner hands each range to the player in order; with no output device it walks
        # through at once, with one it plays them (about 0.2 s of audio in all).
        steps0 = c.full_state()["steps"]
        check("play", c.ask("PLAYLIST play") == "OK")
        for _ in range(100):
            if " running=0 " in c.ask("PLAYLIST state") + " ": break
            time.sleep(0.2)
        state = c.ask("PLAYLIST state")
        check("run finished and played every range in order", " running=0 " in state + " " and state.endswith("played=" + ";".join(want)), state)
        check("playing did not touch the document", c.full_state()["steps"] == steps0)
        check("rename follows: the playlist uses marker ids", c.ask("MARKER rename 3 Chorus") == "OK" and seq() == f"OK count=5 {';'.join(want)}")
        check("clear", c.ask("PLAYLIST clear") == "OK" and seq() == "OK count=0")
        check("bad verb is refused", c.ask("PLAYLIST nonsense").startswith("ERR"))
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
