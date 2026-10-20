#!/usr/bin/env python3
"""f82 scripted run: context menus, toolbar and Tools panel through the control channel.

    python3 tools/alpha/f82.py --exe <wavcraft> [--port 47182]

A synthetic 10 s stereo WAV is opened in a scratch HOME. Checked: the editor and ruler menus list
their items with the right enabled state, menu items run table actions (Copy, Paste, Delete,
Undo), disabled ones refuse, the toolbar lists its commands and the tools, a tool button changes
the active tool, then QUIT; the app's log must hold no ERROR line.
"""
import argparse, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

RATE = 48000


def make_wav(path, secs=10):
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
        frames = bytearray()
        for i in range(secs * RATE):
            v = int(12000 * math.sin(2 * math.pi * 440 * i / RATE))
            frames += struct.pack("<hh", v, v // 2)
        w.writeframes(bytes(frames))


def items(answer):
    body = answer.split("items=", 1)[1].split(" ")[0]
    return dict((kv.rsplit(":", 1)[0], kv.rsplit(":", 1)[1] == "1") for kv in body.split(","))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47182)
    args = ap.parse_args()
    root = os.environ.get("F82_SCRATCH_ROOT", os.path.expanduser("~/dev/craft/_wt/scratch"))
    os.makedirs(root, exist_ok=True)
    home = tempfile.mkdtemp(prefix="f82.", dir=root)
    logs, sessions = os.path.join(home, "logs"), os.path.join(home, "sessions")
    token = os.path.join(home, "token")
    wav = os.path.join(home, "tone.wav")
    make_wav(wav)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    results, proc = [], None

    def check(label, ok, detail=""):
        results.append(bool(ok))
        print(("PASS " if ok else "FAIL ") + label + (f"  [{detail}]" if not ok or detail else ""))

    try:
        proc = subprocess.Popen([args.exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(60):
            if os.path.exists(token) and os.path.getsize(token) > 0:
                break
            time.sleep(0.25)
        c = Control(args.port, open(token).read().strip())
        c.state()
        n = int(c.state()["frames"])
        check("file opened", n == 10 * RATE, n)

        a = c.ask("CONTEXTMENU editor list"); m = items(a)
        check("editor menu lists the selection commands", all(k in m for k in ["edit.cut", "edit.copy", "edit.paste", "edit.delete", "edit.silence", "view.zoomToSelection"]), a)
        check("with no selection Delete and Paste are off", m["edit.delete"] is False and m["edit.paste"] is False, a)
        a = c.ask("CONTEXTMENU ruler list"); m = items(a)
        check("ruler menu lists zoom items", all(k in m for k in ["view.zoomIn", "view.zoomOut", "view.zoomOutFull"]), a)
        a = c.ask("CONTEXTMENU editor run edit.delete")
        check("disabled item refuses", a.startswith("ERR"), a)

        c.ask(f"SELECT {RATE} {3 * RATE}")
        m = items(c.ask("CONTEXTMENU editor list"))
        check("with a selection Copy and Delete are on", m["edit.copy"] and m["edit.delete"], m)
        a = c.ask("CONTEXTMENU editor run edit.copy")
        check("menu Copy runs", a.startswith("OK") and a.endswith("changed=0"), a)
        a = c.ask("CONTEXTMENU editor run edit.delete")
        st = c.state()
        check("menu Delete shortens by 2 s", a.endswith("changed=1") and int(st["frames"]) == n - 2 * RATE and st["undo"] == "Delete", f"{a} {st}")
        c.ask("SELECT 0 0")
        a = c.ask("CONTEXTMENU editor run edit.paste")
        st = c.state()
        check("menu Paste inserts the copy", int(st["frames"]) == n and st["undo"] == "Paste", f"{a} {st}")
        a = c.ask("CONTEXTMENU ruler run edit.copy")
        check("an item of another menu is refused", a.startswith("ERR"), a)

        a = c.ask("TOOLBAR list"); m = items(a)
        want = ["file.open", "file.save", "edit.undo", "edit.redo", "transport.playStop", "view.zoomIn", "view.zoomOut", "tool.timeSelection", "tool.hand"]
        check("toolbar lists the common commands", all(k in m for k in want), a)
        check("Undo is on and Redo is off", m["edit.undo"] and not m["edit.redo"], a)
        check("Tools panel lists planned tools too", "tool.lasso:0" in a and "tool.hand:1" in a and "active=tool.timeSelection" in a, a)
        a = c.ask("TOOLBAR run edit.undo"); st = c.state()
        check("toolbar Undo runs", a.endswith("changed=1") and st["redo"] == "Paste", f"{a} {st}")
        a = c.ask("TOOLBAR run tool.hand")
        check("tool button makes the Hand active", "active=tool.hand" in c.ask("TOOLBAR list"), a)
        a = c.ask("TOOLBAR run file.open")
        check("Open refuses over the channel (it is a dialog)", a.startswith("ERR"), a)
        a = c.ask("TOOLBAR run view.zoomIn")
        check("toolbar Zoom In runs", a.startswith("OK"), a)
        c.ask("QUIT discard")
        proc.wait(timeout=30)
        proc = None
    finally:
        if proc and proc.poll() is None:
            proc.kill()
        errors = []
        for d, _, fs in os.walk(logs) if os.path.isdir(logs) else []:
            for f in fs:
                errors += [l for l in open(os.path.join(d, f), errors="replace") if "ERROR" in l]
        check("no ERROR line in the app's log", not errors, errors[:3])
        shutil.rmtree(home, ignore_errors=True)
    print("RESULT", "PASS" if all(results) else "FAIL", f"{sum(results)}/{len(results)}")
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
