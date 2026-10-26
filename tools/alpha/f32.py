#!/usr/bin/env python3
"""f32 scripted run: shortcuts through the control channel's `KEY <chord>`.

    python3 tools/alpha/f32.py --exe <path to the release wavcraft> [--port 47132]

Starts the app with a fresh HOME and a synthetic 5 s stereo WAV, presses chords with KEY and
checks the result with STATE: select all, Delete, Undo, Redo (both chords), Crop, Home/End,
a chord with no command, Open refused. Quits, and fails on any ERROR line in the app's log.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, SECS = 48000, 5
PORT = 47132


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        frames = bytearray()
        for k in range(RATE * SECS):
            v = int(12000 * math.sin(2 * math.pi * 440 * k / RATE))
            frames += struct.pack("<hh", v, -v)
        w.writeframes(bytes(frames))


class Control:
    def __init__(self, port, token):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        r = self.ask(f"TOKEN {token}")
        if r != "OK":
            raise RuntimeError(f"token refused: {r!r}")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self):
        for _ in range(300):
            head, _, labels = self.ask("STATE").partition(" undo=")
            fields = dict(kv.split("=", 1) for kv in head.split()[1:])
            undo, _, redo = labels.partition(" redo=")
            fields.update(undo=undo, redo=redo)
            if fields.get("busy") == "0":
                return fields
            time.sleep(0.2)
        return fields


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=PORT)
    args = ap.parse_args()
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f32.", dir=scratch_root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    wav, token = os.path.join(work, "tone.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    env = dict(
        os.environ,
        HOME=home,
        USERPROFILE=home,
        WAVCRAFT_LOG_DIR=logs,
        WAVCRAFT_SESSION_DIR=sessions,
        XDG_CONFIG_HOME=os.path.join(home, ".config"),
        XDG_CACHE_HOME=os.path.join(home, ".cache"),
        WAVCRAFT_CONTROL_PORT=str(args.port),
        WAVCRAFT_CONTROL_TOKEN_FILE=token,
    )
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([args.exe, wav], env=env, stdout=err, stderr=err)
    checks = []

    def check(label, ok, detail):
        checks.append((label, bool(ok), detail))
        print(("ok   " if ok else "FAIL ") + label + "  " + str(detail))

    try:
        for _ in range(120):  # the app makes the token file at start
            if os.path.exists(token) and open(token).read().strip():
                break
            time.sleep(0.5)
        c = Control(args.port, open(token).read().strip())
        n = RATE * SECS
        st = c.state()
        check("the file opened", int(st["frames"]) == n, st)

        a = c.ask("KEY Ctrl+A")
        check("Ctrl+A selects all", f"selection=0-{n}" in a and "command=edit.selectAll" in a, a)
        a = c.ask("KEY Cmd+Shift+A")
        check("Cmd+Shift+A deselects (the Mac spelling is the same chord)", "command=edit.deselectAll" in a, a)
        c.ask("SELECT 1000 2000")
        a = c.ask("KEY Delete")
        st = c.state()
        check("Delete removes the selection", "changed=1" in a and int(st["frames"]) == n - 1000 and st["undo"] == "Delete" and st["steps"] == "2", f"{a}; {st}")
        a = c.ask("KEY Ctrl+Z")
        st = c.state()
        check("Ctrl+Z undoes it", int(st["frames"]) == n and st["redo"] == "Delete", f"{a}; {st}")
        a = c.ask("KEY Ctrl+Shift+Z")
        st = c.state()
        check("Ctrl+Shift+Z redoes it", int(st["frames"]) == n - 1000 and st["undo"] == "Delete", f"{a}; {st}")
        c.ask("KEY Ctrl+Z")
        a = c.ask("KEY Ctrl+Y")
        st = c.state()
        check("Ctrl+Y redoes it too", int(st["frames"]) == n - 1000, f"{a}; {st}")
        c.ask("SELECT 100 300")
        a = c.ask("KEY Ctrl+T")
        st = c.state()
        check("Ctrl+T crops to the selection", int(st["frames"]) == 200 and st["undo"].startswith("Crop"), f"{a}; {st}")
        steps = st["steps"]
        c.ask("KEY End")
        st = c.state()
        check("End moves the cursor to the end", st["cursor"] == "200" or st["selection"] == "200-200", st)
        c.ask("KEY Home")
        st = c.state()
        check("Home moves the cursor to the start", st["selection"] == "0-0", st)
        check("History holds Open, Delete and Crop", steps == "3", steps)
        a = c.ask("KEY Ctrl+Shift+Alt+F12")
        check("a chord with no command is refused", a.startswith("ERR no command is bound"), a)
        a = c.ask("KEY Ctrl+Bogus")
        check("a chord that is not a key is refused", a.startswith("ERR unknown key"), a)
        a = c.ask("KEY Ctrl+O")
        check("Open is refused (it would wait on a dialog)", a.startswith("ERR file.open"), a)
        a = c.ask("QUIT discard")
        check("QUIT", a == "OK", a)
        try:
            proc.wait(20)
        except subprocess.TimeoutExpired:
            proc.kill()
            check("the app closed", False, "killed after 20 s")
        log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
        errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
        check("the app's own log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
        bad = [x for x in checks if not x[1]]
        print("RESULT", "PASS" if not bad else f"FAIL ({len(bad)} of {len(checks)})")
        sys.exit(1 if bad else 0)
    finally:
        if proc.poll() is None:
            proc.kill()
        shutil.rmtree(work, ignore_errors=True)


main()
