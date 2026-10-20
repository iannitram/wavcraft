#!/usr/bin/env python3
"""Alpha run for f96 (A012): keys that switch workspace, through the control channel.

    python3 tools/alpha/f96.py --exe <path to the release wavcraft>

Saves two workspaces with different layouts, then presses Alt+Shift+1..3 with KEY and checks with
WORKSPACES and PANELS that the active workspace and the layout follow, that a key past the last
workspace is refused and changes nothing, that SHORTCUTS lists the nine commands, and that a
rebound chord switches too.
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


def zones(c):
    s = kv(c.ask("PANELS"))
    return {k: s[k] for k in s if k.startswith(("zone.", "front.", "maximized")) or k in ("history", "selection")}


def ws(c):
    return kv(c.ask("WORKSPACES"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    args = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f96.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    with wave.open(wav, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(22050)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / 22050))) for i in range(22050 * 2)))
    port, token = 47196, os.path.join(work, "control.token")
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
        base = zones(c)
        c.ask("PANEL move history left")
        a = zones(c)
        check("save Edit", c.ask("WORKSPACE save Edit").startswith("OK current=Edit"))
        c.ask("PANEL move history bottom")
        c.ask("PANEL close selection")
        b = zones(c)
        check("save Mix", c.ask("WORKSPACE save Mix").startswith("OK current=Mix"))
        check("order is Default, Edit, Mix", ws(c)["workspaces"] == "Default|Edit|Mix", ws(c))
        r = c.ask("KEY Alt+Shift+2")
        check("Alt+Shift+2 runs workspace.switch.2", r.startswith("OK command=workspace.switch.2"), r)
        check("it made Edit current and applied its layout", ws(c)["current"] == "Edit" and zones(c) == a, (ws(c), zones(c), a))
        r = c.ask("KEY Alt+Shift+3")
        check("Alt+Shift+3 runs workspace.switch.3", r.startswith("OK command=workspace.switch.3"), r)
        check("it made Mix current and applied its layout", ws(c)["current"] == "Mix" and zones(c) == b, (ws(c), zones(c), b))
        r = c.ask("KEY Alt+Shift+1")
        check("Alt+Shift+1 runs workspace.switch.1", r.startswith("OK command=workspace.switch.1"), r)
        check("it made Default current and applied the built-in layout", ws(c)["current"] == "Default" and zones(c) == base, (ws(c), zones(c), base))
        r = c.ask("KEY Alt+Shift+5")
        check("a key past the last workspace is refused", r.startswith("ERR"), r)
        check("and changes nothing", ws(c)["current"] == "Default" and zones(c) == base)
        sc = c.ask("SHORTCUTS")
        check("SHORTCUTS lists all nine commands on Alt+Shift+<n>",
              all(f"workspace.switch.{n}=Alt+Shift+{n}" in sc for n in range(1, 10)), sc[:200])
        r = c.ask("SHORTCUT set workspace.switch.2 Ctrl+Alt+9")
        check("workspace.switch.2 can be rebound", r.startswith("OK"), r)
        r = c.ask("KEY Ctrl+Alt+9")
        check("the new chord switches", r.startswith("OK command=workspace.switch.2") and ws(c)["current"] == "Edit" and zones(c) == a, (r, ws(c)))
        check("the old chord is free", c.ask("KEY Alt+Shift+2").startswith("ERR"))
        c.ask("SHORTCUT reset")

    run(run1)
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
