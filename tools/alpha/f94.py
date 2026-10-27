#!/usr/bin/env python3
"""f94 scripted run: Appearance colours and named presets.

    python3 tools/alpha/f94.py --exe <wavcraft> [--port 47194] [--keep]

Run 1: defaults, PREF set color.<item> #rrggbb (bad values refused), APPEARANCE preset save/list/load/
delete, the file holds the colours and the preset. Run 2 (restart): colours and preset persisted and a
loaded preset restores its colours. Each run ends with QUIT discard and no ERROR line in the app's log.
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave, struct, math

COLORS = {"waveform": "#112233", "selection": "#223344", "cursor": "#334455", "playhead": "#445566", "background": "#556677"}


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
    ap.add_argument("--port", type=int, default=47194)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f94.", dir=root)
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

    def wait_exit(proc):
        try:
            proc.wait(timeout=30)
            return True
        except subprocess.TimeoutExpired:
            return False

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

    def first(c):
        for k in COLORS:
            check(f"default color.{k}", c.get(f"color.{k}") == "default", c.get(f"color.{k}"))
        check("default spec_palette", c.get("spec_palette") == "ember")
        for k, v in COLORS.items():
            check(f"PREF set color.{k}", c.ask(f"PREF set color.{k} {v}") == "OK" and c.get(f"color.{k}") == v, c.get(f"color.{k}"))
        for k, v in [("color.cursor", "red"), ("color.cursor", "#12345"), ("color.bogus", "#000000"), ("spec_palette", "rainbow"), ("ui_contrast", "9")]:
            before = c.get(k)
            check(f"bad {k} = {v!r} refused", c.ask(f"PREF set {k} {v}").startswith("ERR") and c.get(k) == before, c.get(k))
        check("set spec_palette", c.ask("PREF set spec_palette glacier") == "OK")
        check("set ui_contrast", c.ask("PREF set ui_contrast 1.2") == "OK" and c.get("ui_contrast") == "1.2")
        check("preset save", c.ask("APPEARANCE preset save Midnight Test") == "OK")
        check("preset list", c.ask("APPEARANCE preset list") == "OK Midnight Test", c.ask("APPEARANCE preset list"))
        check("save a bad name refused", c.ask("APPEARANCE preset save a;b").startswith("ERR"))
        check("reset colour", c.ask("PREF set color.waveform default") == "OK" and c.get("color.waveform") == "default")
        check("preset load restores it", c.ask("APPEARANCE preset load Midnight Test") == "OK" and c.get("color.waveform") == "#112233")
        check("load unknown refused", c.ask("APPEARANCE preset load Nope").startswith("ERR"))
        check("preset save second", c.ask("APPEARANCE preset save Scratch") == "OK")
        check("preset delete", c.ask("APPEARANCE preset delete Scratch") == "OK" and c.ask("APPEARANCE preset list") == "OK Midnight Test")
        text = open(settings_files()[0]).read() if settings_files() else ""
        check("a settings file was written inside the scratch HOME", len(settings_files()) == 1, settings_files())
        check("it holds the colours", all(f"color.{k} = {v}" in text for k, v in COLORS.items()), text)
        check("it holds the preset", "appearance_preset = Midnight Test;" in text, text)

    def second(c):
        for k, v in COLORS.items():
            check(f"color.{k} persisted across a restart", c.get(f"color.{k}") == v, c.get(f"color.{k}"))
        check("spec_palette persisted", c.get("spec_palette") == "glacier")
        check("preset persisted", c.ask("APPEARANCE preset list") == "OK Midnight Test")
        check("change then load restores after restart", c.ask("PREF set color.cursor #ffffff") == "OK" and c.ask("APPEARANCE preset load Midnight Test") == "OK" and c.get("color.cursor") == "#334455")

    try:
        run([], first)
        run([], second)
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
