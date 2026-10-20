#!/usr/bin/env python3
"""f46 scripted run: the shortcut editor's control commands, the key file and a restart.

    python3 tools/alpha/f46.py --exe <path to the release wavcraft> [--port 47146]

Starts the app with a fresh HOME and a synthetic 5 s stereo WAV. Checks SHORTCUTS lists the
defaults; assigning a chord another command has is refused, naming that command; a free chord is
assigned and `KEY` with it runs the command (and the old chord no longer does); clear; reassign
(takes the chord from its holder). Restarts the app and checks the changes persisted and still
work. SHORTCUT reset restores the defaults and removes the key file. A garbage key file is
ignored with a warning in the log and the app starts with the defaults. Fails on any ERROR line.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, SECS = 48000, 5


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2), w.setsampwidth(2), w.setframerate(RATE)
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
            if fields.get("busy") == "0":
                return fields
            time.sleep(0.2)
        return fields

    def shortcuts(self):
        a = self.ask("SHORTCUTS")
        return dict(kv.split("=", 1) for kv in a.split()[1:]) if a.startswith("OK") else {"error": a}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47146)
    args = ap.parse_args()
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f46.", dir=scratch_root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    wav, token = os.path.join(work, "tone.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    err = open(os.path.join(work, "stderr.log"), "w")
    checks = []
    n = RATE * SECS

    def check(label, ok, detail):
        checks.append((label, bool(ok), detail))
        print(("ok   " if ok else "FAIL ") + label + "  " + str(detail))

    def start():
        if os.path.exists(token):
            os.remove(token)
        proc = subprocess.Popen([args.exe, wav], env=env, stdout=err, stderr=err)
        for _ in range(120):  # the app makes the token file at start
            if os.path.exists(token) and open(token).read().strip():
                break
            time.sleep(0.5)
        c = Control(args.port, open(token).read().strip())
        st = c.state()
        check("the file opened", int(st["frames"]) == n, st)
        return proc, c

    def stop(proc, c):
        a = c.ask("QUIT discard")
        check("QUIT", a == "OK", a)
        try:
            proc.wait(20)
        except subprocess.TimeoutExpired:
            proc.kill()
            check("the app closed", False, "killed after 20 s")

    def key_file():
        for d, _, fs in os.walk(home):
            if "shortcuts.txt" in fs:
                return os.path.join(d, "shortcuts.txt")
        return None

    proc, c = start()
    sc = c.shortcuts()
    check("SHORTCUTS lists the defaults", sc.get("edit.selectAll") == "Ctrl+A" and sc.get("edit.undo") == "Ctrl+Z" and sc.get("edit.redo") == "Ctrl+Shift+Z|Ctrl+Y" and sc.get("file.open") == "Ctrl+O", {k: sc.get(k) for k in ("edit.selectAll", "edit.undo", "edit.redo")})
    check("no key file yet", key_file() is None, key_file())

    a = c.ask("SHORTCUT set edit.selectAll Ctrl+Z")
    sc = c.shortcuts()
    check("a chord another command has is refused, naming it", a.startswith("ERR conflict") and "edit.undo" in a and sc["edit.selectAll"] == "Ctrl+A" and sc["edit.undo"] == "Ctrl+Z", a)
    a = c.ask("SHORTCUT set edit.nothing Ctrl+K")
    check("an unknown command is refused", a.startswith("ERR no command named"), a)
    a = c.ask("SHORTCUT set edit.selectAll Ctrl+Bogus")
    check("a chord that is not a key is refused", a.startswith("ERR unknown key"), a)

    a = c.ask("SHORTCUT set edit.selectAll Ctrl+K")
    sc = c.shortcuts()
    check("a free chord is assigned", a.startswith("OK") and sc["edit.selectAll"] == "Ctrl+K", a)
    a = c.ask("KEY Ctrl+A")
    check("the old chord no longer runs it", a.startswith("ERR no command is bound"), a)
    a = c.ask("KEY Ctrl+K")
    check("KEY with the new chord runs the command (select all)", "command=edit.selectAll" in a and f"selection=0-{n}" in a, a)

    a = c.ask("SHORTCUT clear edit.deselectAll")
    sc = c.shortcuts()
    check("clear removes every chord", a.startswith("OK") and sc["edit.deselectAll"] == "-", a)
    a = c.ask("KEY Ctrl+D")
    check("a cleared chord runs nothing", a.startswith("ERR no command is bound"), a)

    a = c.ask("SHORTCUT set edit.crop Ctrl+C reassign")
    sc = c.shortcuts()
    check("reassign takes the chord from its holder", a.startswith("OK") and "edit.copy" in a and sc["edit.crop"] == "Ctrl+C" and sc["edit.copy"] == "-", a)
    c.ask("SELECT 100 300")
    a = c.ask("KEY Ctrl+C")
    st = c.state()
    check("the reassigned chord crops", "command=edit.crop" in a and int(st["frames"]) == 200, f"{a}; {st}")
    path = key_file()
    check("the key file was saved", path is not None, path)
    text = open(path).read() if path else ""
    check("the key file lists the differences only", "edit.selectAll = Ctrl+K" in text and "edit.deselectAll = none" in text and "edit.undo" not in text, text.splitlines()[2:])
    stop(proc, c)

    proc, c = start()
    sc = c.shortcuts()
    check("after a restart the changes persisted", sc["edit.selectAll"] == "Ctrl+K" and sc["edit.deselectAll"] == "-" and sc["edit.crop"] == "Ctrl+C" and sc["edit.copy"] == "-" and sc["edit.undo"] == "Ctrl+Z", {k: sc.get(k) for k in ("edit.selectAll", "edit.deselectAll", "edit.crop", "edit.copy")})
    a = c.ask("KEY Ctrl+K")
    check("after a restart KEY with the new chord runs the command", "command=edit.selectAll" in a and f"selection=0-{n}" in a, a)
    a = c.ask("KEY Ctrl+A")
    check("after a restart the old chord is still free", a.startswith("ERR no command is bound"), a)
    a = c.ask("SHORTCUT reset")
    sc = c.shortcuts()
    check("reset restores the defaults", a.startswith("OK") and sc["edit.selectAll"] == "Ctrl+A" and sc["edit.copy"] == "Ctrl+C" and sc["edit.crop"] == "Ctrl+T" and sc["edit.deselectAll"] == "Ctrl+Shift+A|Ctrl+D", a)
    check("reset removes the key file", key_file() is None, key_file())
    a = c.ask("KEY Ctrl+A")
    check("the default chord works again", "command=edit.selectAll" in a, a)
    # A broken key file: write garbage where the app looks for it.
    stop(proc, c)
    cfg = [os.path.join(home, "Library", "Application Support", "wavcraft"), os.path.join(home, ".config", "wavcraft"), os.path.join(home, "AppData", "Roaming", "wavcraft")]
    for d in cfg:
        if os.path.isdir(d):
            open(os.path.join(d, "shortcuts.txt"), "w").write("edit.selectAll = Ctrl+Bogus\nthis is not a key file\n")
    check("a broken key file was put where the app reads it", key_file() is not None, key_file())

    proc, c = start()
    sc = c.shortcuts()
    check("a broken key file is ignored: the app started with the defaults", sc.get("edit.selectAll") == "Ctrl+A" and sc.get("edit.undo") == "Ctrl+Z", sc.get("edit.selectAll"))
    stop(proc, c)

    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    lines = [l.strip() for p in log_files for l in open(p, errors="replace")]
    warns = [l[:200] for l in lines if " WARN " in l and "shortcut file" in l]
    check("the broken file left a warning in the log", bool(warns), warns[:1])
    errors = [l[:200] for l in lines if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    shutil.rmtree(work, ignore_errors=True)
    bad = [x for x in checks if not x[1]]
    print("PASS" if not bad else f"FAIL ({len(bad)} of {len(checks)})")
    sys.exit(1 if bad else 0)


try:
    main()
finally:
    # A crashed run must not leave an app listening on the port.
    subprocess.run(["pkill", "-f", "release/wavcraft .*f46\\."])
