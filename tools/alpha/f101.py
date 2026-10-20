#!/usr/bin/env python3
"""Alpha run for f101 (A013): floating panels remember their monitor, through the control channel.

    python3 tools/alpha/f101.py --exe <path to the release wavcraft>

Run 1: MONITORS simulate two monitors; float two panels at places on each; the PANELS report names
the monitor of each; save the workspace "Dual"; drop the second monitor (the panel on it moves onto
the primary, clamped inside it, the other stays); switch workspaces (Dual comes back reconciled);
put both monitors back and switch to Dual so the settings keep the panel on the second monitor.
Run 2 (restart with WAVCRAFT_MONITORS naming only the primary): both panels are inside the primary
monitor on start. Run 3: a tiny monitor shrinks and clamps windows. The log has no ERROR line.
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
    work = tempfile.mkdtemp(prefix="f101.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    with wave.open(wav, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(22050)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / 22050))) for i in range(22050 * 2)))
    port, token = 47201, os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               APPDATA=os.path.join(home, "AppData", "Roaming"), LOCALAPPDATA=os.path.join(home, "AppData", "Local"),
               WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    env.pop("WAVCRAFT_MONITORS", None)
    checks = []
    BOTH = "main:0,0,1920,1080;side:1920,0,1280,1024"
    MAIN = "main:0,0,1920,1080"

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))

    def run(body, monitors=None):
        e = dict(env)
        if monitors:
            e["WAVCRAFT_MONITORS"] = monitors
        err = open(os.path.join(work, "stderr.log"), "a")
        proc = subprocess.Popen([args.exe, wav], env=e, stdout=err, stderr=err)
        try:
            c = Control(port, token)
            body(c)
            check("QUIT closes the app", c.ask("QUIT") == "OK")
            proc.wait(20)
        except Exception as ex:
            check("run completed", False, f"{type(ex).__name__}: {ex}")
            proc.kill()

    def rect(p, id):
        return tuple(float(v) for v in p["float." + id].split(","))

    def inside(r, m):
        return r[0] >= m[0] and r[1] >= m[1] and r[0] + r[2] <= m[0] + m[2] and r[1] + r[3] <= m[1] + m[3]

    def run1(c):
        check("real monitors are not listed (egui gives only a size)", c.ask("MONITORS") == "OK monitors=real")
        check("bad monitor lists are refused", c.ask("MONITORS simulate nonsense").startswith("ERR"))
        check("MONITORS simulate two", c.ask("MONITORS simulate " + BOTH).startswith("OK monitors=simulated:main@"))
        check("float history on the second monitor", c.ask("PANEL float history 2000 100 320 260").startswith("OK "))
        check("float selection on the primary", c.ask("PANEL float selection 100 100 320 260").startswith("OK "))
        time.sleep(1.0)
        p = panels(c)
        check("history is noted on 'side'", p.get("floatmon.history") == "side", p)
        check("selection is noted on 'main'", p.get("floatmon.selection") == "main", p)
        check("history keeps its place", rect(p, "history") == (2000, 100, 320, 260), p.get("float.history"))
        check("WORKSPACE save Dual", c.ask("WORKSPACE save Dual").startswith("OK current=Dual"))
        check("MONITORS simulate the primary only", c.ask("MONITORS simulate " + MAIN).startswith("OK monitors=simulated:main@"))
        p = panels(c)
        check("history moved onto the primary, inside it", p.get("floatmon.history") == "main" and inside(rect(p, "history"), (0, 0, 1920, 1080)), p)
        check("history was clamped to the right edge (1600 = 1920 - 320)", rect(p, "history")[0] == 1600, p.get("float.history"))
        check("selection did not move", rect(p, "selection") == (100, 100, 320, 260), p.get("float.selection"))
        c.ask("WORKSPACE switch Default")
        p = panels(c)
        check("Default floats nothing", p["floating"] == "-", p)
        c.ask("WORKSPACE switch Dual")
        p = panels(c)
        check("switching to Dual (saved with history on 'side') puts it on the primary", p.get("floatmon.history") == "main" and inside(rect(p, "history"), (0, 0, 1920, 1080)), p)
        c.ask("MONITORS simulate " + BOTH)
        c.ask("WORKSPACE switch Default")
        c.ask("WORKSPACE switch Dual")
        p = panels(c)
        check("with both monitors back, Dual restores history on 'side' at its saved place", p.get("floatmon.history") == "side" and rect(p, "history") == (2000, 100, 320, 260), p)

    def run2(c):
        time.sleep(1.0)
        check("MONITORS reports the faked set from the environment", c.ask("MONITORS") .startswith("OK monitors=simulated:main@0,0,1920,1080"))
        p = panels(c)
        check("after a restart with the second monitor gone, both panels still float", p["floating"] == "history,selection", p)
        check("history (saved on 'side') is on the primary monitor", p.get("floatmon.history") == "main" and inside(rect(p, "history"), (0, 0, 1920, 1080)), p)
        check("selection is inside the primary monitor", inside(rect(p, "selection"), (0, 0, 1920, 1080)), p)
        check("history is where the clamp puts it", rect(p, "history") == (1600, 100, 320, 260), p.get("float.history"))

    def run3(c):
        p = panels(c)
        check("a third start keeps the panels on the primary", p.get("floatmon.history") == "main", p)
        c.ask("MONITORS simulate tiny:0,0,300,200")
        p = panels(c)
        check("a monitor smaller than the windows shrinks and clamps them", inside(rect(p, "history"), (0, 0, 300, 200)) and inside(rect(p, "selection"), (0, 0, 300, 200)), p)

    run(run1)
    run(run2, monitors=MAIN)
    run(run3, monitors=MAIN)
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
