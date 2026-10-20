"""f60 scripted run: shortcut sets and the visual keyboard's lookup.

    python3 tools/alpha/f60.py --exe <path to the release wavcraft> [--port 47160]

Starts the app with a fresh HOME and a synthetic 5 s stereo WAV. Saves two sets (each with a
different Select All chord), switches between them and checks `KEY` follows the set and
`KEYBOARD <mods>` lists the keys bound for the modifiers; restarts and checks the active set is
the one in use; deletes sets (the built-in default is refused). Fails on any ERROR line in the log.
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

    def sets(self):
        a = self.ask("SHORTCUTSETS")
        f = dict(kv.split("=", 1) for kv in a.split()[1:]) if a.startswith("OK") else {"error": a}
        return f.get("active"), [s for s in f.get("sets", "").split("|") if s]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47160)
    args = ap.parse_args()
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f60.", dir=scratch_root)
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
        for _ in range(120):
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

    def selects_all(c, chord):
        a = c.ask("KEY " + chord)
        c.ask("SELECT 0 0")
        return "command=edit.selectAll" in a and f"selection=0-{n}" in a

    def refused(c, chord):
        return c.ask("KEY " + chord).startswith("ERR no command is bound")

    proc, c = start()
    active, names = c.sets()
    check("at first the default set is active and none is saved", active == "default" and names == [], (active, names))
    a = c.ask("KEYBOARD none")
    check("KEYBOARD none lists Space (play/stop) and not Ctrl keys", "Space=transport.playStop" in a and "Z=edit.undo" not in a, a[:120])
    a = c.ask("KEYBOARD Ctrl")
    check("KEYBOARD Ctrl lists Z=edit.undo and A=edit.selectAll", "Z=edit.undo" in a and "A=edit.selectAll" in a and "mods=Ctrl" in a, a[:120])
    a = c.ask("KEYBOARD Ctrl+Bogus")
    check("KEYBOARD refuses an unknown modifier", a.startswith("ERR"), a)

    c.ask("SHORTCUT set edit.selectAll Ctrl+K")
    a = c.ask("SHORTCUTSET save Alpha")
    active, names = c.sets()
    check("save Alpha makes it the active set", a.startswith("OK") and active == "Alpha" and names == ["Alpha"], (a, active, names))
    c.ask("SHORTCUT reset")
    active, _ = c.sets()
    check("reset goes back to the default set", active == "default" and refused(c, "Ctrl+K") and selects_all(c, "Ctrl+A"), active)
    c.ask("SHORTCUT set edit.selectAll Ctrl+L")
    a = c.ask("SHORTCUTSET save Beta")
    active, names = c.sets()
    check("save Beta: two sets, Beta active", a.startswith("OK") and active == "Beta" and names == ["Alpha", "Beta"], (a, active, names))
    check("under Beta, Ctrl+L selects all and Ctrl+K does not", selects_all(c, "Ctrl+L") and refused(c, "Ctrl+K"), "")

    a = c.ask("SHORTCUTSET switch Alpha")
    active, _ = c.sets()
    check("switch Alpha", a.startswith("OK") and active == "Alpha", a)
    check("under Alpha, Ctrl+K selects all and Ctrl+L does not", selects_all(c, "Ctrl+K") and refused(c, "Ctrl+L"), "")
    a = c.ask("KEYBOARD Ctrl")
    check("KEYBOARD Ctrl follows the set (K yes, A no)", "K=edit.selectAll" in a and "A=edit.selectAll" not in a, a[:160])
    a = c.ask("SHORTCUTSET switch Nope")
    check("switching to an unknown set is refused", a.startswith("ERR"), a)
    a = c.ask("SHORTCUTSET save bad/name")
    check("a bad name is refused", a.startswith("ERR"), a)
    a = c.ask("SHORTCUTSET save default")
    check("saving over the built-in default is refused", a.startswith("ERR"), a)
    a = c.ask("SHORTCUTSET delete default")
    check("deleting the built-in default is refused", a.startswith("ERR"), a)
    stop(proc, c)

    proc, c = start()
    active, names = c.sets()
    check("after a restart Alpha is still the active set", active == "Alpha" and names == ["Alpha", "Beta"], (active, names))
    check("after a restart Ctrl+K selects all and Ctrl+L does not", selects_all(c, "Ctrl+K") and refused(c, "Ctrl+L"), "")
    c.ask("SHORTCUTSET switch Beta")
    stop(proc, c)

    proc, c = start()
    active, _ = c.sets()
    check("after a second restart Beta applies", active == "Beta" and selects_all(c, "Ctrl+L") and refused(c, "Ctrl+K"), active)
    a = c.ask("SHORTCUTSET switch default")
    active, names = c.sets()
    check("switch default: the shipped table", a.startswith("OK") and active == "default" and selects_all(c, "Ctrl+A") and refused(c, "Ctrl+L") and names == ["Alpha", "Beta"], (a, active, names))
    a = c.ask("SHORTCUTSET delete Alpha")
    active, names = c.sets()
    check("delete Alpha", a.startswith("OK") and names == ["Beta"], (a, names))
    c.ask("SHORTCUTSET switch Beta")
    a = c.ask("SHORTCUTSET delete Beta")
    active, names = c.sets()
    check("deleting the active set leaves none and the default active", a.startswith("OK") and names == [] and active == "default", (a, active, names))
    stop(proc, c)

    proc, c = start()
    active, names = c.sets()
    check("after the last restart the default applies", active == "default" and names == [] and selects_all(c, "Ctrl+A"), (active, names))
    stop(proc, c)

    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    lines = [l.strip() for p in log_files for l in open(p, errors="replace")]
    errors = [l[:200] for l in lines if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    shutil.rmtree(work, ignore_errors=True)
    bad = [x for x in checks if not x[1]]
    print("PASS" if not bad else f"FAIL ({len(bad)} of {len(checks)})")
    sys.exit(1 if bad else 0)


try:
    main()
finally:
    subprocess.run(["pkill", "-f", "release/wavcraft .*f60\\."])
