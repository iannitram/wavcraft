#!/usr/bin/env python3
"""Alpha run for f85 (A001, A011, A029): built-in workspaces and the Editor panel.

    python3 tools/alpha/f85.py --exe <path to the release wavcraft>

Run 1 (fresh scratch HOME): the default layout (editor centre, History right, Selection below);
WORKSPACE switch through Default, Editing, Recording and Repair, checking PANELS for each (all four
differ in arrangement and shown panels); a change then WORKSPACE reset gives the built-in state
back; none can be saved over or deleted; the Editor can't be closed (PANEL close, ACTION window.hide)
but can be maximised and restored. Run 2 (same HOME): the built-in in use is remembered.
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
    work = tempfile.mkdtemp(prefix="f85.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    with wave.open(wav, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(22050)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / 22050))) for i in range(22050 * 2)))
    port, token = 47185, os.path.join(work, "control.token")
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

    def panels(c):
        s = kv(c.ask("PANELS"))
        return {k: v for k, v in s.items() if k != "maximized" and not k.startswith("front.")}

    EXPECT = {
        "Default": {"editor": "1", "waveform": "1", "spectral": "1", "history": "1", "selection": "1", "zone.left": "-",
                    "zone.right": "history", "zone.bottom": "selection", "zone.center": "editor,waveform,spectral"},
        "Editing": {"editor": "1", "waveform": "1", "spectral": "1", "history": "1", "selection": "1", "zone.left": "history",
                    "zone.right": "selection", "zone.bottom": "-", "zone.center": "editor,waveform,spectral"},
        "Recording": {"editor": "1", "waveform": "1", "spectral": "0", "history": "0", "selection": "1", "zone.left": "-",
                      "zone.right": "-", "zone.bottom": "selection", "zone.center": "editor,waveform"},
        "Repair": {"editor": "1", "waveform": "0", "spectral": "1", "history": "1", "selection": "1", "zone.left": "-",
                   "zone.right": "selection", "zone.bottom": "history", "zone.center": "editor,spectral"},
    }

    def run1(c):
        w = ws(c)
        check("a fresh run: Default is in use and the four built-ins are listed", w == {"current": "Default", "modified": "0", "workspaces": "Default|Editing|Recording|Repair"}, w)
        check("the default layout: editor centre, History right, Selection below", panels(c) == EXPECT["Default"], panels(c))
        seen = []
        for name in ["Editing", "Recording", "Repair", "Default", "repair"]:
            a = c.ask("WORKSPACE switch " + name)
            canon = name.capitalize()
            check("WORKSPACE switch " + name, a.startswith("OK current=" + canon + " modified=0"), a)
            check(canon + " has its own arrangement and shown panels", panels(c) == EXPECT[canon], panels(c))
            seen.append(tuple(sorted(panels(c).items())))
        check("the four built-ins are all different", len(set(seen[:4])) == 4)
        c.ask("WORKSPACE switch Editing")
        for name in EXPECT:
            check(name + " can't be saved over", c.ask("WORKSPACE save " + name.lower()).startswith("ERR"))
            check(name + " can't be deleted", c.ask("WORKSPACE delete " + name).startswith("ERR"))
        check("the list is unchanged after those refusals", ws(c)["workspaces"] == "Default|Editing|Recording|Repair", ws(c))
        c.ask("PANEL move history bottom")
        c.ask("PANEL close selection")
        check("a change shows the built-in as modified", ws(c)["modified"] == "1" and panels(c) != EXPECT["Editing"])
        a = c.ask("WORKSPACE reset")
        check("WORKSPACE reset gives the built-in state back", a.startswith("OK current=Editing modified=0") and panels(c) == EXPECT["Editing"], (a, panels(c)))
        c.ask("WORKSPACE switch Repair")
        c.ask("PANEL close history")
        c.ask("WORKSPACE reset Repair")
        check("WORKSPACE reset <name> too", panels(c) == EXPECT["Repair"], panels(c))
        files = sorted(f for d, _, fs in os.walk(home) for f in fs if f.endswith(".workspace"))
        check("built-in workspaces write no files", files == [], files)
        # The Editor panel.
        check("the Editor is a panel, shown", kv(c.ask("PANELS"))["editor"] == "1")
        check("PANEL close editor is refused", c.ask("PANEL close editor").startswith("ERR"))
        check("ACTION window.hide.editor is refused", c.ask("ACTION window.hide.editor").startswith("ERR"))
        check("ACTION window.toggle.editor is refused", c.ask("ACTION window.toggle.editor").startswith("ERR"))
        check("the Editor is still shown", kv(c.ask("PANELS"))["editor"] == "1" and "editor" in kv(c.ask("PANELS"))["zone.center"])
        check("the Editor can't be moved", c.ask("PANEL move editor left").startswith("ERR"))
        c.ask("PANEL maximize editor")
        check("PANEL maximize editor", kv(c.ask("PANELS"))["maximized"] == "editor")
        c.ask("PANEL maximize editor")
        check("maximize again restores", kv(c.ask("PANELS"))["maximized"] == "-")
        c.ask("WORKSPACE switch Recording")
        check("the last switch is Recording", ws(c)["current"] == "Recording" and ws(c)["modified"] == "0", ws(c))

    def run2(c):
        w = ws(c)
        check("after a restart Recording is still in use", w["current"] == "Recording" and w["modified"] == "0", w)
        check("and its layout is back", panels(c) == EXPECT["Recording"], panels(c))
        c.ask("PANEL close selection")
        check("a change after the restart shows modified", ws(c)["modified"] == "1")
        c.ask("WORKSPACE reset")
        check("reset after a restart brings the built-in back", panels(c) == EXPECT["Recording"] and ws(c)["modified"] == "0")

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
