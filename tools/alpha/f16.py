#!/usr/bin/env python3
"""f16 alpha run: the tool model and right-click extend, through the control channel.

    python3 tools/alpha/f16.py --exe <release wavcraft> [--port 47116]

Starts the app twice in a scratch HOME (settings file absent, then `right_click = menu`), on a
synthetic WAV. Checks STATE's tool and right_click fields, `ACTION tool.time_selection`,
`GESTURE right_click <frame>` (extend, shorten, cursor), and that the setting turns it off.
Fails on any ERROR line in the app's log. Deletes the scratch folder.
"""
import argparse, math, os, shutil, socket, subprocess, struct, sys, tempfile, time, wave

RATE, SECS = 48000, 5


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2), w.setsampwidth(2), w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<hh", int(8000 * math.sin(i * 0.05)), int(8000 * math.sin(i * 0.03))) for i in range(RATE * SECS)))


class Control:
    def __init__(self, port, token_file):
        for _ in range(120):
            try:
                if os.path.exists(token_file):
                    self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
                    break
            except OSError:
                pass
            time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        assert self.ask("TOKEN " + open(token_file).read().strip()) == "OK"

    def ask(self, line):
        self.f.write(line + "\n"), self.f.flush()
        return self.f.readline().strip()

    def state(self):
        for _ in range(120):
            a = self.ask("STATE")
            head = a.partition(" undo=")[0]
            d = dict(kv.split("=", 1) for kv in head.split()[1:])
            if d.get("busy") == "0" and d.get("frames") not in (None, "0"):
                return d
            time.sleep(0.5)
        raise RuntimeError("file never opened: " + a)


def run_app(exe, env, wav, port, token, script):
    proc = subprocess.Popen([exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        c = Control(port, token)
        script(c)
        assert c.ask("QUIT") == "OK"
        proc.wait(20)
    finally:
        if proc.poll() is None:
            proc.kill()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47116)
    a = ap.parse_args()
    work = tempfile.mkdtemp(prefix="f16.", dir=os.environ.get("WAVCRAFT_SCRATCH") or None)
    results = []

    def check(label, ok, detail=""):
        results.append(ok)
        print(("PASS " if ok else "FAIL ") + label + (f"  [{detail}]" if detail else ""))

    try:
        home, logs = os.path.join(work, "home"), os.path.join(work, "logs")
        os.makedirs(home)
        wav = os.path.join(work, "tone.wav")
        make_wav(wav)
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_LOG_DIR=logs,
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
                   WAVCRAFT_CONTROL_PORT=str(a.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)

        def first(c):
            s = c.state()
            check("default tool is time_selection", s.get("tool") == "time_selection", s.get("tool"))
            check("right-click default is extend", s.get("right_click") == "extend", s.get("right_click"))
            c.ask("SELECT 1000 2000")
            r = c.ask("GESTURE right_click 3000")
            check("right-click past the end extends it", "selection=1000-3000" in r and "moved=1" in r, r)
            r = c.ask("GESTURE right_click 1500")
            check("right-click inside shortens from the nearer edge", "selection=1500-3000" in r, r)
            c.ask("SELECT 5000 5000")
            r = c.ask("GESTURE right_click 2000")
            check("right-click with only a cursor selects to the click", "selection=2000-5000" in r, r)
            r = c.ask("GESTURE right_click 99999999")
            check("right-click past the file end is clamped", f"selection=2000-{RATE * SECS}" in r, r)
            check("a bad gesture is an error", c.ask("GESTURE right_click x").startswith("ERR"))
            r = c.ask("ACTION tool.time_selection")
            check("ACTION tool.time_selection answers OK", r.startswith("OK"), r)
            check("STATE reports the tool after it", c.state().get("tool") == "time_selection")

        run_app(a.exe, env, wav, a.port, token, first)

        # Second run: the setting file switches right-click to a menu (no extend).
        for d in (os.path.join(home, ".config", "wavcraft"), os.path.join(home, "Library", "Application Support", "wavcraft"), os.path.join(home, "AppData", "Roaming", "wavcraft")):
            os.makedirs(d, exist_ok=True)
            open(os.path.join(d, "settings.txt"), "w").write("right_click = menu\n")
        os.remove(token)

        def second(c):
            s = c.state()
            check("setting read from the settings file", s.get("right_click") == "menu", s.get("right_click"))
            c.ask("SELECT 1000 2000")
            r = c.ask("GESTURE right_click 3000")
            check("with the menu setting right-click does not move the selection", "selection=1000-2000" in r and "moved=0" in r, r)

        run_app(a.exe, env, wav, a.port, token, second)

        bad = []
        for root, _, files in os.walk(logs):
            for f in files:
                bad += [l for l in open(os.path.join(root, f), errors="replace") if "ERROR" in l]
        check("no ERROR line in the app's log", not bad, bad[:3])
    finally:
        shutil.rmtree(work, ignore_errors=True)
    print("RESULT", "PASS" if all(results) and results else "FAIL", f"{sum(results)}/{len(results)}")
    sys.exit(0 if all(results) and results else 1)


main()
