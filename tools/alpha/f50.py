#!/usr/bin/env python3
"""Alpha run for f50 (A004): floating panels, through the control channel.

    python3 tools/alpha/f41.py --exe <path to the release wavcraft>

Run 1 (fresh scratch HOME): PANEL float history makes it floating (PANELS lists it with a window
rect, the zone it left is empty), the window is really drawn (float_window=os|embedded), editor
panels and unknown ids are refused, maximise is refused while floating, a second panel floats too,
the layout is saved as a workspace. Run 2 (same HOME, restart): both panels are still floating with
the same rect; the workspace switch to Default docks them, switch back floats them again; dock one
back by name and one by its old zone. Run 3: selection floats, history is docked; dock selection left. Run 4: nothing floats.
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


def kv(ans):
    assert ans.startswith("OK "), ans
    return dict(p.split("=", 1) for p in ans.split()[1:])


def panels(c):
    return kv(c.ask("PANELS"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    args = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f50.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    with wave.open(wav, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(22050)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / 22050))) for i in range(22050 * 2)))
    port, token = 47150, os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               APPDATA=os.path.join(home, "AppData", "Roaming"), LOCALAPPDATA=os.path.join(home, "AppData", "Local"),
               WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []
    state = {}

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

    def rect(p, id):
        return p.get("float." + id)

    def run1(c):
        p = panels(c)
        check("fresh: nothing floats", p["floating"] == "-" and "float_window" not in p, p)
        check("PANEL float history", c.ask("PANEL float history").startswith("OK "))
        time.sleep(1.5)
        p = panels(c)
        check("history is listed as floating with a rect", p["floating"] == "history" and rect(p, "history"), p)
        check("it left its zone", "history" not in p["zone.right"].split(",") and p["zone.right"] == "selection", p)
        check("the window was drawn (os or embedded)", p.get("float_window") in ("os", "embedded"), p.get("float_window"))
        state["kind"] = p.get("float_window")
        check("editor panels cannot float", c.ask("PANEL float waveform").startswith("ERR"))
        check("unknown panel refused", c.ask("PANEL float nope").startswith("ERR"))
        check("a floating panel is not maximised", c.ask("PANEL maximize history").startswith("ERR"))
        check("PANEL float selection", c.ask("PANEL float selection").startswith("OK "))
        time.sleep(1.0)
        p = panels(c)
        check("both float, the right zone is empty", p["floating"] == "history,selection" and p["zone.right"] == "-", p)
        check("each has its own place", rect(p, "history") != rect(p, "selection"), p)
        state["history"], state["selection"] = rect(p, "history"), rect(p, "selection")
        check("the layout is a modified workspace", kv(c.ask("WORKSPACES"))["modified"] == "1")
        check("WORKSPACE save Floaty", c.ask("WORKSPACE save Floaty").startswith("OK current=Floaty modified=0"))

    def run2(c):
        time.sleep(1.5)
        p = panels(c)
        check("after a restart both still float", p["floating"] == "history,selection", p)
        check("with the same place and size (history)", rect(p, "history") == state["history"], (rect(p, "history"), state["history"]))
        check("with the same place and size (selection)", rect(p, "selection") == state["selection"], (rect(p, "selection"), state["selection"]))
        check("the window is drawn again", p.get("float_window") == state["kind"], p.get("float_window"))
        check("the workspace is not modified by the restart", kv(c.ask("WORKSPACES"))["modified"] == "0")
        c.ask("WORKSPACE switch Default")
        p = panels(c)
        check("switching to Default docks them (Default floats nothing)", p["floating"] == "-" and p["zone.right"] == "history,selection", p)
        c.ask("WORKSPACE switch Floaty")
        time.sleep(1.0)
        p = panels(c)
        check("switching back floats them again, same rects", p["floating"] == "history,selection" and rect(p, "history") == state["history"], p)
        check("PANEL dock history bottom", c.ask("PANEL dock history bottom").startswith("OK "))
        p = panels(c)
        check("history is docked at the bottom, selection still floats", p["zone.bottom"] == "history" and p["floating"] == "selection", p)
        check("a bad zone is refused", c.ask("PANEL dock selection moon").startswith("ERR"))
        check("PANEL dock selection (back to where it came from)", c.ask("PANEL dock selection").startswith("OK "))
        p = panels(c)
        check("selection is back in the right zone", p["floating"] == "-" and p["zone.right"] == "selection" and "float_window" not in p, p)
        c.ask("PANEL float selection")
        time.sleep(0.5)

    def run3(c):
        time.sleep(1.0)
        p = panels(c)
        check("after the third start selection floats, history is docked", p["floating"] == "selection" and p["zone.bottom"] == "history", p)
        check("PANEL dock selection left", c.ask("PANEL dock selection left").startswith("OK "))

    def run4(c):
        p = panels(c)
        check("after a last restart nothing floats and selection is on the left", p["floating"] == "-" and p["zone.left"] == "selection", p)

    run(run1)
    run(run2)
    run(run3)
    run(run4)
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR line", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    print("floating windows were:", state.get("kind"))
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
