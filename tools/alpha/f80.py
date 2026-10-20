#!/usr/bin/env python3
"""Scripted run for f80 (notices, settings export/import, preference reset).

    python3 tools/alpha/f80.py --exe <wavcraft binary>

Starts the app with HOME in a fresh scratch folder, then through the control channel:
  1. writes a settings file and a shortcuts file in the config folder, exports them (SETTINGS export);
  2. changes the settings, imports the bundle: the files are back, the changed ones kept as .old;
  3. a damaged bundle (cut short, unknown key, path traversal name) is refused and changes nothing;
  4. PREF reset puts the preference files aside as .old and PREF show reports the default folder;
  5. ABOUT opens Help > About and reports the notices' size;
  6. QUIT; the app's log must hold no ERROR line.
Prints one line per check and exits 1 on any failure.
"""
import argparse, os, platform, shutil, socket, subprocess, sys, tempfile, time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import golden  # noqa: E402  (Control, free_port)

PORT = 47180
checks = []


def check(label, ok, detail=""):
    checks.append(bool(ok))
    print(("PASS " if ok else "FAIL ") + label + (f": {detail}" if detail and not ok else ""))


def config_dir(home):
    if platform.system() == "Darwin":
        return os.path.join(home, "Library", "Application Support", "wavcraft")
    if platform.system() == "Windows":
        return os.path.join(home, "AppData", "Roaming", "wavcraft")
    return os.path.join(home, ".config", "wavcraft")


def read(p):
    with open(p, encoding="utf-8") as f:
        return f.read()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    scratch = tempfile.mkdtemp(prefix="f80.", dir=base)
    home = os.path.join(scratch, "home")
    logs = os.path.join(scratch, "logs")
    os.makedirs(home)
    os.makedirs(logs)
    cfg = config_dir(home)
    os.makedirs(cfg)
    token_file = os.path.join(scratch, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, XDG_CONFIG_HOME=os.path.join(home, ".config"), APPDATA=os.path.join(home, "AppData", "Roaming"),
               WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=os.path.join(scratch, "sessions"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token_file)
    env.pop("WAVCRAFT_SESSION_DIR")  # let the settings file's session folder count in PREF show
    settings, shortcuts = os.path.join(cfg, "settings.txt"), os.path.join(cfg, "shortcuts.txt")
    folder_a = os.path.join(scratch, "folder-a")
    folder_b = os.path.join(scratch, "folder-b")
    open(settings, "w").write(f"session_folder = {folder_a}\n")
    open(shortcuts, "w").write("edit.undo = Ctrl+U\n")
    proc = subprocess.Popen([a.exe], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        # The token file is made by the app on start.
        for _ in range(100):
            if os.path.exists(token_file) and os.path.getsize(token_file) >= 64:
                break
            time.sleep(0.2)
        c = golden.Control(PORT, read(token_file).strip())
        bundle = os.path.join(scratch, "export", "settings.wavset")
        r = c.ask(f"SETTINGS export {bundle}")
        check("export answers OK", r.startswith("OK"), r)
        text = read(bundle) if os.path.exists(bundle) else ""
        check("the bundle holds both files", text.startswith("WAVCRAFT-SETTINGS 1\n") and "file settings.txt" in text and "file shortcuts.txt" in text and folder_a in text, text[:200])

        open(settings, "w").write(f"session_folder = {folder_b}\n")
        os.remove(shortcuts)
        r = c.ask(f"SETTINGS import {bundle}")
        check("import answers OK", r.startswith("OK"), r)
        check("settings.txt is back", read(settings) == f"session_folder = {folder_a}\n", read(settings))
        check("shortcuts.txt is back", os.path.exists(shortcuts) and read(shortcuts) == "edit.undo = Ctrl+U\n")
        check("the replaced settings are kept as .old", os.path.exists(settings + ".old") and read(settings + ".old") == f"session_folder = {folder_b}\n")
        r = c.ask("PREF show")
        check("the imported session folder is in use", r.startswith("OK") and f"session_folder={folder_a}" in r, r)

        good = read(bundle)
        for label, bad in [("cut short", good[:-9]), ("unknown key", "WAVCRAFT-SETTINGS 1\nfile settings.txt 9\nbogus = 1\nend\n"),
                           ("path traversal", "WAVCRAFT-SETTINGS 1\nfile ../evil.txt 1\nx\nend\n"), ("not a bundle", "hello\n")]:
            p = os.path.join(scratch, "bad.wavset")
            open(p, "w").write(bad)
            before = read(settings)
            r = c.ask(f"SETTINGS import {p}")
            check(f"a damaged bundle ({label}) is refused", r.startswith("ERR"), r)
            check(f"  and changes nothing ({label})", read(settings) == before and not os.path.exists(os.path.join(cfg, "..", "evil.txt")))

        r = c.ask("PREF reset")
        check("PREF reset answers OK", r.startswith("OK"), r)
        check("settings.txt and shortcuts.txt are put aside", not os.path.exists(settings) and not os.path.exists(shortcuts)
              and read(settings + ".old") == f"session_folder = {folder_a}\n" and read(shortcuts + ".old") == "edit.undo = Ctrl+U\n")
        r = c.ask("PREF show")
        check("the session folder is the default again", r.startswith("OK") and "source=Default" in r and "saved_session_folder= " in r + " ", r)
        r = c.ask("ABOUT")
        n = int(r.split("notices_bytes=")[1]) if "notices_bytes=" in r else 0
        check("About opens and holds the notices", r.startswith("OK") and n > 100_000, r)
        check("QUIT answers OK", c.ask("QUIT").startswith("OK"))
        try:
            proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            check("the app exits after QUIT", False)
    finally:
        if proc.poll() is None:
            proc.kill()
        err = proc.stderr.read().decode(errors="replace") if proc.stderr else ""
    log_text = ""
    for d, _, files in os.walk(logs):
        for f in files:
            log_text += read(os.path.join(d, f)) if not f.endswith(".gz") else ""
    errors = [l for l in (log_text + "\n" + err).splitlines() if "ERROR" in l or "panicked" in l]
    check("no ERROR line in the app's log", not errors, "; ".join(errors[:3]))
    if not a.keep and all(checks):
        shutil.rmtree(scratch, ignore_errors=True)
    else:
        print("scratch kept at", scratch, "(log dir:", logs + ")")
        if not all(checks):
            shutil.rmtree(scratch, ignore_errors=True) if False else None
    print("RESULT", "PASS" if all(checks) else "FAIL", f"{sum(checks)}/{len(checks)}")
    sys.exit(0 if all(checks) else 1)


if __name__ == "__main__":
    main()
