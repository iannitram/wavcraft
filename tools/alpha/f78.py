#!/usr/bin/env python3
"""Alpha run for f78 (A048): panel menus and Metadata Display, through the control channel.

    python3 tools/alpha/f78.py --exe <path to the release wavcraft>

Run 1 (fresh scratch HOME): PANELMENU list/run for standard and panel-specific items, errors,
METADATA set/open. Run 2 (same HOME): the closed panel and the metadata fields persist.
The app's log must hold no ERROR line. The scratch folder is deleted at the end.
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave, math, struct


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Control:
    def __init__(self, port, token_file):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        for _ in range(40):
            if os.path.exists(token_file):
                break
            time.sleep(0.25)
        if self.ask("TOKEN " + open(token_file).read().strip()) != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()


def panels(c):
    ans = c.ask("PANELS")
    assert ans.startswith("OK "), ans
    return dict(kv.split("=") for kv in ans.split()[1:])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    args = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f78.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    with wave.open(wav, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(22050)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / 22050))) for i in range(22050 * 2)))
    port, token = 47178, os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               APPDATA=os.path.join(home, "AppData", "Roaming"), LOCALAPPDATA=os.path.join(home, "AppData", "Local"),
               WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))

    def run(body):
        err = open(os.path.join(work, "stderr.log"), "a")
        proc = subprocess.Popen([args.exe, wav], env=env, stdout=err, stderr=err)
        try:
            c = Control(port, token)
            body(c)
            check("QUIT closes the app", c.ask("QUIT discard") == "OK")
            proc.wait(20)
        except Exception as e:
            check("run completed", False, f"{type(e).__name__}: {e}")
            proc.kill()

    def run1(c):
        a = c.ask("PANELMENU history list")
        check("history menu lists standard items then Clear History", a == "OK items=close,maximize,clear_history maximized=none", a)
        a = c.ask("PANELMENU waveform list")
        check("waveform menu has only the standard items", a == "OK items=close,maximize maximized=none", a)
        check("an unknown panel answers ERR", c.ask("PANELMENU nothere list").startswith("ERR"))
        check("an unknown item answers ERR", c.ask("PANELMENU history run nothere").startswith("ERR"))
        check("a metadata item on a non-file panel answers ERR", c.ask("PANELMENU history run metadata_display").startswith("ERR"))
        a = c.ask("PANELMENU history run clear_history")
        check("Clear History with no edits answers ERR (nothing to clear)", a.startswith("ERR"), a)
        c.ask("INSERT_SILENCE 0.5")
        a = c.ask("PANELMENU history run clear_history")
        check("Clear History after an edit opens the confirmation", a.startswith("OK") and "confirm_clear_history=1" in a, a)
        a = c.ask("PANELMENU spectral run maximize")
        check("Maximize marks the panel maximized", a.startswith("OK maximized=spectral"), a)
        check("the menu now reads maximized=spectral", c.ask("PANELMENU spectral list").endswith("maximized=spectral"))
        a = c.ask("PANELMENU spectral run maximize")
        check("Maximize again restores", a.startswith("OK maximized=none"), a)
        a = c.ask("PANELMENU history run close")
        check("Close Panel hides the panel", a.endswith("history=0"), a)
        check("settings.txt lists the closed panel", "hidden_panels = history" in "".join(open(os.path.join(d, f)).read() for d, _, fs in os.walk(home) for f in fs if f == "settings.txt"))
        check("METADATA reports the default fields", c.ask("METADATA") == "OK fields=name,duration,rate,channels dialog=0")
        check("METADATA set chooses fields", c.ask("METADATA set name,size,format") == "OK fields=name,format,size")
        check("METADATA set refuses an unknown field", c.ask("METADATA set bogus").startswith("ERR"))
        check("METADATA open shows the dialog", c.ask("METADATA open") == "OK" and c.ask("METADATA").endswith("dialog=1"))

    def run2(c):
        p = c.ask("PANELS")
        check("closed panel is still closed after a restart", p == "OK waveform=1 spectral=1 history=0", p)
        m = c.ask("METADATA")
        check("metadata fields are kept after a restart", m == "OK fields=name,format,size dialog=0", m)

    run(run1)
    run(run2)
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR line", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    ok = all(c[1] for c in checks)
    for label, good, detail in checks:
        print(("PASS " if good else "FAIL ") + label + ("" if good else f"  [{detail}]"))
    print("RESULT", "pass" if ok else "fail")
    if ok:
        shutil.rmtree(work, ignore_errors=True)
    else:
        print("kept for inspection:", work)
    sys.exit(0 if ok else 1)


main()
