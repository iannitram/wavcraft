#!/usr/bin/env python3
"""Alpha run for f41 (A009, A010): workspaces, through the control channel.

    python3 tools/alpha/f41.py --exe <path to the release wavcraft>

Run 1 (fresh scratch HOME): Default is built in and current; WORKSPACE save keeps two workspaces;
switch restores each layout; a change shows modified=1 and reset brings the saved layout back;
delete refuses Default and the one in use, and removes another; Default can't be replaced.
Run 2 (same HOME): after a restart the same workspace is current, the list and layout are the same,
and the files are in the config folder.
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
    work = tempfile.mkdtemp(prefix="f41.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    with wave.open(wav, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(22050)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / 22050))) for i in range(22050 * 2)))
    port, token = 47141, os.path.join(work, "control.token")
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
        w = ws(c)
        check("a fresh run: Default is the only workspace and the one in use", w == {"current": "Default", "modified": "0", "workspaces": "Default"}, w)
        base = zones(c)
        c.ask("PANEL move history left")
        check("a change makes the workspace modified", ws(c)["modified"] == "1", ws(c))
        a = zones(c)
        check("WORKSPACE save Edit", c.ask("WORKSPACE save Edit").startswith("OK current=Edit modified=0"), ws(c))
        c.ask("PANEL move history bottom")
        c.ask("PANEL close selection")
        b = zones(c)
        check("WORKSPACE save Mix down (a name with a space)", c.ask("WORKSPACE save Mix down").startswith("OK current=Mix_down modified=0"), ws(c))
        check("both are listed, Default first", ws(c)["workspaces"] == "Default|Edit|Mix_down", ws(c))
        c.ask("WORKSPACE switch Edit")
        check("switch restores the layout and shown panels", zones(c) == a and ws(c)["current"] == "Edit", (zones(c), a))
        c.ask("WORKSPACE switch mix down")
        check("switch back (name case ignored)", zones(c) == b and b["selection"] == "0", zones(c))
        c.ask("WORKSPACE switch Edit")
        c.ask("PANEL move history right")
        c.ask("PANEL maximize history")
        check("after a change Edit shows modified=1", ws(c)["modified"] == "1" and zones(c) != a)
        check("WORKSPACE reset", c.ask("WORKSPACE reset").startswith("OK current=Edit modified=0"), ws(c))
        check("reset brought the saved layout back (maximise is not part of it)", zones(c) == a, (zones(c), a))
        c.ask("WORKSPACE switch Default")
        check("the built-in Default is the registry layout", zones(c) == base, (zones(c), base))
        check("Default can't be replaced", c.ask("WORKSPACE save default").startswith("ERR"))
        check("Default can't be deleted", c.ask("WORKSPACE delete Default").startswith("ERR"))
        c.ask("WORKSPACE switch Edit")
        check("the workspace in use can't be deleted", c.ask("WORKSPACE delete Edit").startswith("ERR") and "Edit" in ws(c)["workspaces"])
        check("an unknown workspace is refused", c.ask("WORKSPACE switch Nope").startswith("ERR") and c.ask("WORKSPACE delete Nope").startswith("ERR"))
        check("a bad name is refused", c.ask("WORKSPACE save a/b").startswith("ERR") and c.ask("WORKSPACE save").startswith("ERR"))
        check("WORKSPACE delete Mix down", c.ask("WORKSPACE delete Mix down").startswith("OK") and ws(c)["workspaces"] == "Default|Edit", ws(c))
        files = sorted(f for d, _, fs in os.walk(home) for f in fs if f.endswith(".workspace"))
        check("one file per saved workspace in the config folder", files == ["edit.workspace"], files)
        run1.a = a

    def run2(c):
        w = ws(c)
        check("after a restart Edit is still in use and listed with Default", w == {"current": "Edit", "modified": "0", "workspaces": "Default|Edit"}, w)
        check("and its layout is back", zones(c)["zone.left"] == "history", zones(c))
        c.ask("PANEL close history")
        check("a change after the restart shows as modified", ws(c)["modified"] == "1")
        c.ask("WORKSPACE reset Edit")
        check("reset after a restart brings it back", ws(c)["modified"] == "0" and zones(c) == run1.a, (zones(c), run1.a))

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
