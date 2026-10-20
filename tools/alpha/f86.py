#!/usr/bin/env python3
"""f86 scripted run: UI language. Pseudo-locale on, a menu title through I18N get, restart, persistence."""
import argparse, glob, os, shutil, socket, subprocess, sys, tempfile, time

PORT = 47186
ap = argparse.ArgumentParser()
ap.add_argument("--exe", required=True)
exe = ap.parse_args().exe
base = os.path.expanduser("~/dev/craft/_wt/scratch")
os.makedirs(base, exist_ok=True)
work = tempfile.mkdtemp(prefix="f86.", dir=base)
fails = []


def check(label, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + label + (f" ({detail})" if detail else ""))
    if not ok:
        fails.append(label)


def run(first):
    env = dict(os.environ, HOME=work, XDG_CONFIG_HOME=os.path.join(work, "cfg"), WAVCRAFT_LOG_DIR=os.path.join(work, "log"),
               WAVCRAFT_SESSION_DIR=os.path.join(work, "sess"), WAVCRAFT_CONTROL_PORT=str(PORT),
               WAVCRAFT_CONTROL_TOKEN_FILE=os.path.join(work, "token"))
    proc = subprocess.Popen([exe], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    s = None
    for _ in range(120):
        try:
            s = socket.create_connection(("127.0.0.1", PORT), timeout=30)
            break
        except OSError:
            time.sleep(0.5)
    if s is None:
        proc.kill()
        raise SystemExit("no control channel")
    f = s.makefile("rw", encoding="utf-8")

    def ask(line):
        f.write(line + "\n")
        f.flush()
        return f.readline().strip()
    token = open(os.path.join(work, "token")).read().strip()
    assert ask(f"TOKEN {token}") == "OK"
    return proc, ask


try:
    proc, ask = run(True)
    check("starts in English", ask("PREF get language") == "OK language=en")
    check("menu title in English", ask("I18N get menu.file") == "OK File", ask("I18N get menu.file"))
    check("unknown language refused", ask("PREF set language klingon").startswith("ERR"))
    check("switch to pseudo", ask("PREF set language pseudo") == "OK language=pseudo")
    title = ask("I18N get menu.file")
    check("menu title is pseudo", title.startswith("OK [") and title != "OK File", title)
    check("a status string keeps its blanks", "{0}" in ask("I18N get status.cursor"))
    check("no keys missing in pseudo", ask("I18N missing") == "OK 0", ask("I18N missing"))
    check("unknown key shows up as missing", ask("I18N get no.such.key") == "OK no.such.key" and "no.such.key" in ask("I18N missing"))
    check("QUIT", ask("QUIT") == "OK")
    proc.wait(timeout=30)
    proc, ask = run(False)
    check("pseudo persists after restart", ask("PREF get language") == "OK language=pseudo")
    t2 = ask("I18N get menu.file")
    check("menu title still pseudo", t2 == title, t2)
    check("back to English", ask("PREF set language en") == "OK language=en" and ask("I18N get menu.file") == "OK File")
    ask("QUIT")
    proc.wait(timeout=30)
    errs = [l.strip() for p in glob.glob(os.path.join(work, "log", "**", "*"), recursive=True) if os.path.isfile(p)
            for l in open(p, errors="replace") if "ERROR" in l]
    check("no ERROR lines in the app log", not errs, "; ".join(errs[:3]))
finally:
    shutil.rmtree(work, ignore_errors=True)
sys.exit(1 if fails else 0)
