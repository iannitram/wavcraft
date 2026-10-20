#!/usr/bin/env python3
"""f20 scripted run: preferences, the Audio Hardware devices and the reset.

    python3 tools/alpha/f20.py --exe <wavcraft> [--port 47120] [--keep]

Three runs of the app in one scratch HOME (so no setting is written outside it):
1. DEVICES lists the output and input names; PREF get shows the defaults (crossfades 2 and 5 ms,
   both devices "System default"); PREF set changes the crossfades and both devices (the first
   listed ones, when the machine has any) and refuses bad values; the settings file holds them.
2. A restart: PREF get returns what was set (they persisted).
3. A restart with --reset-preferences: PREF get is back to the defaults, and the old file is kept
   aside whole.
Each run must end with QUIT discard (run 1 left an edit) and leave no ERROR line in the app's log. The Shift-at-launch path
needs a held key and is not driven here.
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave, struct, math

DEFAULTS = {"crossfade_cut_join_ms": "2", "crossfade_edge_ms": "5", "output_device": "System default", "input_device": "System default"}


class Control:
    def __init__(self, port, token, tries=120):
        for _ in range(tries):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("the control channel refused the token")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def get(self, key):
        a = self.ask(f"PREF get {key}")
        return a[3:].strip() if a.startswith("OK") else a


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
        w.writeframes(b"".join(struct.pack("<hh", int(8000 * math.sin(i / 20)), int(8000 * math.sin(i / 31))) for i in range(48000 * 2)))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47120)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f20.", dir=root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    make_wav(wav)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               APPDATA=os.path.join(home, "AppData", "Roaming"), RUST_BACKTRACE="1",
               WAVCRAFT_CONTROL_PORT=str(a.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"), flush=True)

    def settings_files():
        return sorted(os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs if f.startswith("settings.txt"))

    def run(extra, body):
        err = open(os.path.join(work, "stderr.log"), "a")
        proc = subprocess.Popen([a.exe, *extra, wav], env=env, stdout=err, stderr=err)
        try:
            for _ in range(240):
                if os.path.exists(token) or proc.poll() is not None:
                    break
                time.sleep(0.25)
            c = Control(a.port, open(token).read().strip())
            body(c)
            check("QUIT closes the app", c.ask("QUIT discard") == "OK" and wait_exit(proc))
        finally:
            if proc.poll() is None:
                proc.kill()

    def wait_exit(proc):
        try:
            proc.wait(timeout=30)
            return True
        except subprocess.TimeoutExpired:
            return False

    picked = {}

    def first(c):
        d = c.ask("DEVICES")
        check("DEVICES answers with output= input= playing_on=", d.startswith("OK output=") and " input=" in d and " playing_on=" in d, d)
        out = d.split(" input=")[0][len("OK output="):]
        inp = d.split(" input=")[1].split(" playing_on=")[0]
        picked["out"] = out.split("|")[0] if out else ""
        picked["in"] = inp.split("|")[0] if inp else ""
        print(f"     devices: {len(out.split('|')) if out else 0} output, {len(inp.split('|')) if inp else 0} input", flush=True)
        for k, v in DEFAULTS.items():
            check(f"default {k} = {v}", c.get(k) == v, c.get(k))
        check("PREF set crossfade_cut_join_ms 0", c.ask("PREF set crossfade_cut_join_ms 0") == "OK")
        check("PREF set crossfade_edge_ms 12", c.ask("PREF set crossfade_edge_ms 12") == "OK")
        check("a bad value is refused", c.ask("PREF set crossfade_edge_ms banana").startswith("ERR") and c.get("crossfade_edge_ms") == "12")
        check("an unknown key is refused", c.ask("PREF get nonsense").startswith("ERR"))
        if picked["out"]:
            check("PREF set output_device <first output>", c.ask(f"PREF set output_device {picked['out']}") == "OK" and c.get("output_device") == picked["out"])
            d2 = c.ask("DEVICES")
            check("the player reopened on the chosen output", d2.endswith("playing_on=" + picked["out"]), d2)
        if picked["in"]:
            check("PREF set input_device <first input>", c.ask(f"PREF set input_device {picked['in']}") == "OK" and c.get("input_device") == picked["in"])
        check("an edit still works with the new crossfades", c.ask("SELECT 1000 2000") .startswith("OK") and c.ask("ACTION edit.delete").endswith("changed=1"))
        check("a settings file was written inside the scratch HOME", len(settings_files()) == 1, settings_files())
        text = open(settings_files()[0]).read() if settings_files() else ""
        check("it holds the new values", "crossfade_edge_ms = 12" in text and "crossfade_cut_join_ms = 0" in text, text)

    def second(c):
        check("crossfade_edge_ms persisted across a restart", c.get("crossfade_edge_ms") == "12", c.get("crossfade_edge_ms"))
        check("crossfade_cut_join_ms persisted", c.get("crossfade_cut_join_ms") == "0")
        check("output_device persisted", c.get("output_device") == (picked["out"] or "System default"), c.get("output_device"))
        check("input_device persisted", c.get("input_device") == (picked["in"] or "System default"), c.get("input_device"))

    def third(c):
        for k, v in DEFAULTS.items():
            check(f"after --reset-preferences {k} is back to {v}", c.get(k) == v, c.get(k))
        aside = [f for f in settings_files() if ".reset-" in f]
        check("the old settings file was kept aside whole", len(aside) == 1 and "crossfade_edge_ms = 12" in open(aside[0]).read(), settings_files())

    try:
        run([], first)
        run([], second)
        run(["--reset-preferences"], third)
    except Exception as e:
        check("the run completed", False, f"{type(e).__name__}: {e}")
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    bad = [c for c in checks if not c[1]]
    print(f"\n{len(checks) - len(bad)}/{len(checks)} checks passed")
    if a.keep or bad:
        print(f"scratch kept: {work}")
    else:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
