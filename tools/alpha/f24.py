#!/usr/bin/env python3
"""Alpha run for f24 (A002, A003, A006, A007): docking of the registered panels, through the control channel.

    python3 tools/alpha/f24.py --exe <path to the release wavcraft>

Run 1 (fresh scratch HOME): PANELS reports the default zones; PANEL move puts panels in zones and
tab order; the editor panels refuse to move; PANEL activate changes the front tab; PANEL maximize
toggles; PANEL close hides a panel and moving it again shows it; settings.txt then holds the layout.
Run 2 (same HOME): the layout is back as left, and a closed panel stays closed.
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


def state(c):
    ans = c.ask("PANELS")
    assert ans.startswith("OK "), ans
    return dict(kv.split("=", 1) for kv in ans.split()[1:])


def zones(c):
    s = state(c)
    return {k: s[k] for k in s if k.startswith(("zone.", "front.", "maximized"))}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    args = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f24.", dir=base)
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
        z = zones(c)
        check("default: History and Selection tab together on the right, editor in the centre",
              z["zone.right"] == "history,selection" and z["zone.left"] == "-" and z["zone.bottom"] == "-"
              and z["zone.center"] == "waveform,spectral" and z["front.right"] == "history" and z["maximized"] == "-", z)
        c.ask("PANEL move history left")
        z = zones(c)
        check("PANEL move history left", z["zone.left"] == "history" and z["zone.right"] == "selection", z)
        c.ask("PANEL move selection left")
        z = zones(c)
        check("a second panel joins the left zone as a tab, in front", z["zone.left"] == "history,selection" and z["front.left"] == "selection" and z["zone.right"] == "-", z)
        c.ask("PANEL activate history")
        check("PANEL activate puts a tab in front", zones(c)["front.left"] == "history")
        check("the editor panels refuse to move", c.ask("PANEL move waveform left").startswith("ERR"))
        check("an unknown zone is refused", c.ask("PANEL move history moon").startswith("ERR"))
        check("the centre is refused", c.ask("PANEL move history center").startswith("ERR"))
        check("an unknown panel is refused", c.ask("PANEL close nothere").startswith("ERR"))
        c.ask("PANEL move selection bottom")
        z = zones(c)
        check("PANEL move selection bottom", z["zone.bottom"] == "selection" and z["zone.left"] == "history", z)
        c.ask("PANEL maximize history")
        check("PANEL maximize history", zones(c)["maximized"] == "history")
        c.ask("PANEL maximize history")
        check("maximize again restores", zones(c)["maximized"] == "-")
        c.ask("PANEL maximize spectral")
        check("an editor panel can be maximised too", zones(c)["maximized"] == "spectral")
        c.ask("PANEL maximize spectral")
        c.ask("PANEL close selection")
        s = state(c)
        check("PANEL close hides the panel (registry) and takes it off the tab strip", s["selection"] == "0" and s["zone.bottom"] == "-", s)
        c.ask("PANEL move selection right")
        s = state(c)
        check("moving a closed panel shows it in its new zone", s["selection"] == "1" and s["zone.right"] == "selection", s)
        c.ask("PANEL move selection bottom")
        found = [os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs if f == "settings.txt"]
        text = open(found[0]).read() if found else ""
        check("settings.txt holds the layout and sizes", "panel_layout = left:history" in text and "bottom:selection" in text and "zone_sizes = left:" in text, text.replace("\n", " | ")[-200:])

    def run2(c):
        z = zones(c)
        check("after a restart the layout is back", z["zone.left"] == "history" and z["zone.bottom"] == "selection" and z["zone.right"] == "-" and z["maximized"] == "-", z)
        c.ask("PANEL close history")
        c.ask("PANEL move history right")
        z = zones(c)
        check("and it can be changed again", z["zone.right"] == "history" and z["zone.left"] == "-", z)

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
