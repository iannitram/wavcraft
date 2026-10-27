#!/usr/bin/env python3
"""Alpha run for f02 (A008): the panel registry and Window menu, through the control channel.

    python3 tools/alpha/f02.py --exe <path to the release wavcraft>

Run 1 (fresh scratch HOME): PANELS reports every panel shown; ACTION window.toggle.history hides
History; window.hide.spectral hides the spectral display; window.show.waveform on a shown panel
changes nothing; an unknown panel id answers ERR; settings.txt then lists the hidden panels; QUIT.
Run 2 (same HOME): the hidden panels are still hidden; window.toggle.history shows it again; QUIT.
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
    return {k: v for k, v in (kv.split("=") for kv in ans.split()[1:]) if k in ("waveform", "spectral", "history")}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    args = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f02.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    with wave.open(wav, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(22050)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / 22050))) for i in range(22050 * 2)))
    port, token = free_port(), os.path.join(work, "control.token")
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
            check("QUIT closes the app", c.ask("QUIT") == "OK")
            proc.wait(20)
        except Exception as e:
            check("run completed", False, f"{type(e).__name__}: {e}")
            proc.kill()

    def run1(c):
        p = panels(c)
        check("PANELS lists waveform, spectral, history, all shown", p == {"waveform": "1", "spectral": "1", "history": "1"}, p)
        a = c.ask("ACTION window.toggle.history")
        check("window.toggle.history hides History", "history=0" in a.split(), a)
        a = c.ask("ACTION window.hide.spectral")
        check("window.hide.spectral hides the spectral display", "spectral=0 history=0" in a, a)
        a = c.ask("ACTION window.show.waveform")
        check("window.show.waveform on a shown panel changes nothing", panels(c) == {"waveform": "1", "spectral": "0", "history": "0"}, a)
        a = c.ask("ACTION window.toggle.nothere")
        check("an unknown panel id answers ERR", a.startswith("ERR"), a)
        found = [os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs if f == "settings.txt"]
        text = open(found[0]).read() if found else ""
        check("settings.txt lists the hidden panels", "hidden_panels = spectral,history" in text, text.replace("\n", " | ")[-120:])

    def run2(c):
        p = panels(c)
        check("hidden panels are still hidden after a restart", p == {"waveform": "1", "spectral": "0", "history": "0"}, p)
        a = c.ask("ACTION window.toggle.history")
        check("window.toggle.history shows History again", "history=1" in a.split(), a)

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
