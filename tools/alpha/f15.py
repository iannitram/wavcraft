#!/usr/bin/env python3
"""f15 (History panel) scripted run: arrow keys, Delete State, Clear History, Audition-style names.

    python3 tools/alpha/f15.py --exe <path to the wavcraft binary>

A synthetic 5 s stereo WAV is opened in a scratch HOME with its own session and log folders; the
History panel is driven through the control channel (port 47115). Fails on any ERROR in the log.
"""
import argparse, json, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

RATE, SECS, PORT = 48000, 5, 47115


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        for s in range(0, RATE * SECS, 4800):
            n = min(4800, RATE * SECS - s)
            w.writeframes(b"".join(struct.pack("<hh", int(8000 * math.sin(2 * math.pi * 220 * (s + i) / RATE)), int(8000 * math.sin(2 * math.pi * 330 * (s + i) / RATE))) for i in range(n)))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f15.", dir=root)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    wav, token = os.path.join(work, "t.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    env = dict(os.environ, HOME=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs, XDG_CONFIG_HOME=os.path.join(home, ".config"),
               XDG_CACHE_HOME=os.path.join(home, ".cache"), RUST_BACKTRACE="1", WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    checks = []

    def check(name, ok, detail):
        checks.append({"check": name, "ok": bool(ok), "detail": detail})

    proc = subprocess.Popen([args.exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(80):
            if os.path.exists(token):
                break
            time.sleep(0.25)
        c = Control(PORT, open(token).read().strip())
        for _ in range(100):
            st = c.state()
            if int(st.get("frames", 0)) == RATE * SECS:
                break
            time.sleep(0.2)
        n = RATE * SECS
        c.ask(f"SELECT 0 {RATE}")
        c.ask("ACTION edit.delete")
        c.ask("ACTION edit.leftChannel")
        c.ask(f"SELECT {RATE} {2 * RATE}")
        c.ask("ACTION edit.cut")
        st = c.state()
        check("one-channel Cut is labelled Cut Audio", st["undo"] == "Cut Audio", st)
        c.ask("SELECT 0 0")
        c.ask("ACTION edit.paste")
        st = c.state()
        check("one-channel Paste is labelled Paste Audio", st["undo"] == "Paste Audio", st)
        c.ask("SELECT 1000 100000")
        c.ask("ACTION edit.crop")
        st = c.state()
        check("one-channel Crop is labelled Crop Audio", st["undo"] == "Crop Audio" and st["steps"] == "5" and st["cursor"] == "4", st)
        c.ask("ACTION edit.leftChannel")
        c.ask("SELECT 0 1000")
        c.ask("ACTION edit.delete")
        st = c.state()
        check("all-channel Delete is still Delete", st["undo"] == "Delete" and st["steps"] == "6", st)
        a = c.ask("HISTORY_KEY up")
        st = c.state()
        check("Up arrow moves to the step before", a == "OK cursor=4" and st["cursor"] == "4" and st["undo"] == "Crop Audio", f"{a}; {st}")
        a = c.ask("HISTORY_KEY down")
        a2 = c.ask("HISTORY_KEY down")
        st = c.state()
        check("Down arrow moves forward and stops at the last step", a == "OK cursor=5" and a2 == "OK cursor=unchanged" and st["cursor"] == "5", f"{a}; {a2}; {st}")
        a = c.ask("HISTORY_DELETE 0")
        check("Open can't be deleted", a.startswith("ERR"), a)
        a = c.ask("HISTORY_DELETE 99")
        check("a missing step can't be deleted", a.startswith("ERR"), a)
        a = c.ask("HISTORY_DELETE 4")
        st = c.state()
        check("Delete State removes the step and the later ones", a == "OK removed=2" and st["steps"] == "4" and st["cursor"] == "3" and st["undo"] == "Paste Audio", f"{a}; {st}")
        a = c.ask("HISTORY_DELETE 2")
        st = c.state()
        check("deleting the cursor's step goes back one", a == "OK removed=2" and st["steps"] == "2" and st["cursor"] == "1" and int(st["frames"]) == n - RATE, f"{a}; {st}")
        c.ask("SELECT 0 1000")
        c.ask("ACTION edit.silence")
        before = c.state()
        a = c.ask("HISTORY_CLEAR")
        st = c.state()
        check("Clear History keeps only the current state", a == "OK" and st["steps"] == "1" and st["cursor"] == "0" and st["frames"] == before["frames"] and st["redo"] == "-" and st["undo"] == "-", f"{a}; {st}")
        check("Quit", c.ask("QUIT discard") == "OK", "")
        proc.wait(15)
    except Exception as e:
        check("control channel", False, f"{type(e).__name__}: {e}")
        proc.kill()
    logf = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in logf for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR", bool(logf) and not errors, errors[:3] or f"{len(logf)} log file(s)")
    ok = all(c["ok"] for c in checks)
    print(json.dumps({"status": "pass" if ok else "fail", "checks": checks}, indent=1))
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


main()
