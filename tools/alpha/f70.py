#!/usr/bin/env python3
"""f70 (A278, A281) scripted run: Auto Save preferences, Media and Disk Cache, on the release app.

    python3 tools/alpha/f70.py --exe <path to the wavcraft binary> [--port 47170] [--keep]

Run 1 starts the app in a scratch HOME (so it makes its machine id), then quits. Run 2 starts again,
and while it runs the script makes fake sessions in the scratch session folder, as a crashed run
leaves them: an old dead one, a young dead one, an old one with a journal (recoverable), an old
one from another machine, and the app's own live session (its files aged). CACHE cleanup must
remove only the old dead one; with a shorter age only the young dead one more; the live,
recoverable and foreign sessions stay. PREF set/get of the Auto Save keys: off means no journal is
written after an edit, on means one is; the interval is honoured; the settings file persists them
into run 3 (with the cache size). Fails on any ERROR line in the app's logs.
"""
import argparse, os, shutil, subprocess, sys, tempfile, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from f10 import Control, make_wav, finish, session_dirs, RATE, SECS  # noqa: E402

DEAD_PID = 4000000000
DAY = 86400


def age(path, days):
    t = time.time() - days * DAY
    for e in os.listdir(path):
        os.utime(os.path.join(path, e), (t, t))
    os.utime(path, (t, t))


def fake_session(root, name, machine, journal=None, days=0):
    d = os.path.join(root, name)
    os.makedirs(d)
    open(os.path.join(d, "session.lock"), "w").write(f"pid={DEAD_PID}\nhost=scratch\nmachine={machine}\napp=wavcraft 0.1.0\n")
    open(os.path.join(d, "session.held"), "w").close()
    open(os.path.join(d, "pack-0001.f32"), "wb").write(bytes(8192))
    if journal is not None:
        open(os.path.join(d, "journal.wcj"), "w").write(journal)
    age(d, days)
    return d


def kv(line):
    return dict(x.split("=", 1) for x in line.split()[1:])


def run(exe, port, keep):
    base = os.environ.get("WAVCRAFT_SCRATCH") or tempfile.gettempdir()
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f70.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    wav = os.path.join(work, "synthetic-20s.wav")
    make_wav(wav)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               XDG_DATA_HOME=os.path.join(home, ".local", "share"), APPDATA=os.path.join(home, "AppData", "Roaming"),
               LOCALAPPDATA=os.path.join(home, "AppData", "Local"), WAVCRAFT_CONTROL_PORT=str(port),
               WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + (f": {detail}" if detail and not ok else ""))

    def start(n, *args):
        err = open(os.path.join(work, f"stderr{n}.log"), "w")
        return subprocess.Popen([exe, *args], env=env, stdout=err, stderr=err), err

    def edit(c, n=1):
        for i in range(n):
            c.ask(f"SELECT {1000 * (i + 1)} {1000 * (i + 1) + 500}")
            c.ask("ACTION edit.delete")

    def machine_id():
        for dp, _, fs in os.walk(home):
            if "machine-id" in fs:
                return open(os.path.join(dp, "machine-id")).read().strip()
        return ""

    def settings_text():
        for dp, _, fs in os.walk(home):
            if "settings.txt" in fs:
                return open(os.path.join(dp, "settings.txt")).read()
        return ""

    p, err = start(1)
    try:
        c = Control(port, token)
        for _ in range(100):
            if machine_id():
                break
            time.sleep(0.2)
        mid = machine_id()
        check("run 1: the app made its machine id", len(mid) == 32, mid)
        defaults = kv(c.ask("PREF get"))
        check("defaults: Auto Save on, 2 s, 30 days, default cache", defaults == {"autosave_enabled": "on", "autosave_interval_secs": "2", "autosave_keep_days": "30", "cache_mb": "default"}, defaults)
        c.ask("QUIT")
        p.wait(20)
    except Exception as e:
        check("run 1", False, f"{type(e).__name__}: {e}")
        p.kill()
        err.close()
        return finish(work, logs, checks, keep)
    err.close()

    p, err = start(2)
    try:
        c = Control(port, token)
        for _ in range(100):
            if c.autosave()["scanned"] == "1":
                break
            time.sleep(0.2)
        time.sleep(1.5)  # the start-up sweep (which removes dead sessions with no journal) is over
        # The app's own live session: open a file, age its files.
        c.ask(f"OPEN {wav}")
        st = c.state()
        live = [d for d in session_dirs(sessions)]
        check("run 2: opening the file made one live session", len(live) == 1 and int(st["frames"]) == RATE * SECS, (live, st))
        live_dir = os.path.join(sessions, live[0])
        age(live_dir, 40)
        old = fake_session(sessions, "aaaaaaaaaaaaaaaa", mid, days=40)
        young = fake_session(sessions, "bbbbbbbbbbbbbbbb", mid, days=5)
        recov = fake_session(sessions, "cccccccccccccccc", mid, journal="garbage", days=40)
        foreign = fake_session(sessions, "dddddddddddddddd", "f" * 32, days=40)
        info = kv(c.ask("CACHE info"))
        print("CACHE info:", info)
        check("CACHE info counts the five sessions and what a 30-day clean-up would remove", (info["sessions"], info["removable"], info["recoverable"]) == ("5", "1", "1"), info)
        check("CACHE info shows the default cache budget unchanged", info["cache_mb"] == info["cache_default_mb"], info)
        r = kv(c.ask("CACHE cleanup 30"))
        check("cleanup 30 days removed exactly one session", r["removed"] == "1" and int(r["freed"]) >= 8192, r)
        check("the old dead session is gone", not os.path.exists(old))
        check("the young one, the recoverable one, the foreign one and the live one stay",
              all(os.path.exists(d) for d in (young, recov, foreign, live_dir)) and os.path.exists(os.path.join(recov, "journal.wcj")))
        r = kv(c.ask("CACHE cleanup 1"))
        check("cleanup 1 day removes the young dead session only", r["removed"] == "1" and not os.path.exists(young) and all(os.path.exists(d) for d in (recov, foreign, live_dir)), r)
        check("a second cleanup removes nothing", kv(c.ask("CACHE cleanup 0"))["removed"] == "0")
        check("the live session still works after the clean-ups", int(c.state()["frames"]) == RATE * SECS)

        # Auto Save preferences.
        check("PREF set refuses a bad value", c.ask("PREF set autosave_interval_secs 0").startswith("ERR") and c.ask("PREF set autosave_enabled maybe").startswith("ERR"))
        check("PREF set autosave_enabled off", c.ask("PREF set autosave_enabled off") == "OK autosave_enabled=off")
        edit(c, 2)
        time.sleep(4)
        a = c.autosave()
        check("with Auto Save off an edit offers no journal", a["offered"] == "0" and a["written"] == "0", a)
        check("PREF set autosave_enabled on", c.ask("PREF set autosave_enabled on") == "OK autosave_enabled=on")
        c.ask("PREF set autosave_interval_secs 6")
        edit(c)
        for _ in range(50):
            a = c.autosave()
            if a["written"] == "1" and a["pending"] == "0":
                break
            time.sleep(0.2)
        check("with Auto Save on an edit writes a journal", a["written"] == "1" and os.path.exists(os.path.join(live_dir, "journal.wcj")), a)
        edit(c)
        time.sleep(2.5)
        a = c.autosave()
        check("a 6 s interval holds the next journal back", a["written"] == "1", a)
        for _ in range(60):
            a = c.autosave()
            if a["written"] == "2":
                break
            time.sleep(0.2)
        check("and writes it once the interval has passed", a["written"] == "2" and a["failed"] == "0", a)
        c.ask("PREF set autosave_keep_days 7")
        c.ask("PREF set cache_mb 128")
        info = kv(c.ask("CACHE info"))
        check("the cache size and keep days are reported", info["cache_mb"] == "128" and info["keep_days"] == "7", info)
        text = settings_text()
        check("settings.txt holds the keys", all(k in text for k in ("autosave_enabled = on", "autosave_interval_secs = 6", "autosave_keep_days = 7", "cache_mb = 128")), text)
        c.ask("QUIT discard")
        p.wait(20)
    except Exception as e:
        check("run 2", False, f"{type(e).__name__}: {e}")
        p.kill()
    err.close()

    p, err = start(3)
    try:
        c = Control(port, token)
        got = kv(c.ask("PREF get"))
        check("run 3: the preferences persisted", got == {"autosave_enabled": "on", "autosave_interval_secs": "6", "autosave_keep_days": "7", "cache_mb": "128"}, got)
        c.ask("QUIT")
        p.wait(20)
    except Exception as e:
        check("run 3", False, f"{type(e).__name__}: {e}")
        p.kill()
    err.close()
    return finish(work, logs, checks, keep)


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47170)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    sys.exit(run(a.exe, a.port, a.keep))
