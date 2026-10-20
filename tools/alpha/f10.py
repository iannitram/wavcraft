#!/usr/bin/env python3
"""f10 (A239) scripted run: Auto Save and crash recovery, on the release app.

    python3 tools/alpha/f10.py --exe <path to the wavcraft binary> [--port 47110] [--keep]

Run 1 opens a synthetic stereo WAV in a scratch HOME and session folder, makes five edits (Delete,
Silence on one channel, Cut, Paste, Insert Silence) and an Undo, waits for the journal to be
durable, then kills the app with SIGKILL. Run 2 starts with the same HOME and session folder, no
file: it must find the session (and the stale-session sweep that runs at start must leave it),
recover it through the control channel, and show the same length, the same History labels and
lengths at every step, and the same cursor. It then edits on, and a clean QUIT discard removes the
recovered session's folder. Fails on any ERROR line in the app's logs.
"""
import argparse, math, os, shutil, signal, socket, struct, subprocess, sys, tempfile, time, wave

RATE, SECS = 48000, 20


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        for s in range(SECS):
            frames = bytearray()
            for i in range(RATE):
                n = s * RATE + i
                l = int(12000 * math.sin(2 * math.pi * 440 * n / RATE))
                r = int(12000 * math.sin(2 * math.pi * 330 * n / RATE)) + (n % 97)
                frames += struct.pack("<hh", l, r)
            w.writeframes(bytes(frames))


class Control:
    def __init__(self, port, token_file, tries=120):
        for _ in range(tries):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.25)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        for _ in range(40):
            if os.path.exists(token_file):
                break
            time.sleep(0.25)
        if self.ask("TOKEN " + open(token_file).read().strip()) != "OK":
            raise RuntimeError("the control channel refused the token")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self, until_idle=True, timeout=90):
        t0 = time.time()
        while True:
            head, _, labels = self.ask("STATE").partition(" undo=")
            fields = dict(kv.split("=", 1) for kv in head.split()[1:])
            fields["undo"], _, fields["redo"] = labels.partition(" redo=")
            if not until_idle or fields.get("busy") == "0" or time.time() - t0 > timeout:
                return fields
            time.sleep(0.2)

    def autosave(self):
        return dict(kv.split("=", 1) for kv in self.ask("AUTOSAVE").split()[1:])

    def history(self):
        """Every step's (frames, undo label) by clicking through the History panel, then back."""
        st = self.state()
        n, cur = int(st["steps"]), int(st["cursor"])
        out = []
        for i in range(n):
            self.ask(f"HISTORY {i}")
            s = self.state()
            out.append((int(s["frames"]), s["undo"]))
        self.ask(f"HISTORY {cur}")
        self.state()
        return out


def journal_ok(dir_, steps, cursor):
    p = os.path.join(dir_, "journal.wcj")
    if not os.path.exists(p):
        return False
    lines = open(p).read().splitlines()
    return sum(l.startswith("step ") for l in lines) == steps and f"cursor {cursor}" in lines


def session_dirs(root):
    return [d for d in os.listdir(root) if len(d) == 16] if os.path.isdir(root) else []


def run(exe, port, keep):
    base = os.environ.get("WAVCRAFT_SCRATCH") or tempfile.gettempdir()
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f10.", dir=base)
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

    err1 = open(os.path.join(work, "stderr1.log"), "w")
    p1 = subprocess.Popen([exe, wav], env=env, stdout=err1, stderr=err1)
    try:
        c = Control(port, token)
        st = c.state()
        for _ in range(200):
            if int(st["frames"]) == RATE * SECS and st["busy"] == "0":
                break
            time.sleep(0.2)
            st = c.state()
        check("run 1: the file opened at its full length", int(st["frames"]) == RATE * SECS, st)
        time.sleep(3)
        check("an unedited document has no journal (a crash loses nothing)", c.autosave()["written"] == "0" and not any(os.path.exists(os.path.join(sessions, d, "journal.wcj")) for d in session_dirs(sessions)), c.autosave())
        c.ask(f"SELECT {RATE} {2 * RATE}")
        c.ask("ACTION edit.delete")
        c.ask("ACTION edit.leftChannel")
        c.ask("SELECT 0 24000")
        c.ask("ACTION edit.silence")
        c.ask("ACTION edit.leftChannel")
        c.ask("SELECT 100000 130000")
        c.ask("ACTION edit.cut")
        c.ask("SELECT 0 0")
        c.ask("ACTION edit.paste")
        c.ask("INSERT_SILENCE 0.5")
        c.ask("ACTION edit.undo")
        before = c.state()
        hist = c.history()
        print("before the kill:", before, hist)
        check("run 1: Open plus five edits, cursor on the fourth", before["steps"] == "6" and before["cursor"] == "4", before)
        dirs = session_dirs(sessions)
        check("run 1: one session folder", len(dirs) == 1, dirs)
        sdir = os.path.join(sessions, dirs[0])
        ok = False
        for _ in range(100):
            ok = journal_ok(sdir, 6, 4) and not c.autosave()["pending"] == "1"
            if ok:
                break
            time.sleep(0.2)
        a = c.autosave()
        check("run 1: the journal is durable and current (6 steps, cursor 4)", ok and a["failed"] == "0", a)
        mode = os.stat(os.path.join(sdir, "journal.wcj")).st_mode & 0o777
        check("the journal is owner-only", mode & 0o077 == 0 or os.name == "nt", oct(mode))
        check("no half-written journal left beside it", not os.path.exists(os.path.join(sdir, "journal.wcj.tmp")))
    except Exception as e:
        check("run 1", False, f"{type(e).__name__}: {e}")
        p1.kill()
        return finish(work, logs, checks, keep)
    p1.send_signal(signal.SIGKILL)
    p1.wait()
    err1.close()
    check("run 1 was killed (SIGKILL)", p1.returncode == -signal.SIGKILL, p1.returncode)
    check("the session folder stays after the kill", os.path.isdir(sdir))

    err2 = open(os.path.join(work, "stderr2.log"), "w")
    p2 = subprocess.Popen([exe], env=env, stdout=err2, stderr=err2)
    try:
        c = Control(port, token)
        a = c.autosave()
        for _ in range(100):
            if a["scanned"] == "1":
                break
            time.sleep(0.2)
            a = c.autosave()
        check("run 2: the crashed session is offered for recovery", a["recoverable"] == "1", a)
        time.sleep(2)  # the stale-session sweep runs at start-up; it must leave a recoverable session
        check("run 2: the start-up sweep left the session", os.path.isdir(sdir) and os.path.exists(os.path.join(sdir, "journal.wcj")))
        r = c.ask("RECOVER")
        st = c.state()
        print("after recovery:", st)
        check("run 2: RECOVER answers OK", r == "OK", r)
        check("run 2: same length, steps, cursor and labels",
              (st["frames"], st["steps"], st["cursor"], st["undo"], st["redo"]) == (before["frames"], before["steps"], before["cursor"], before["undo"], before["redo"]),
              f"{st} vs {before}")
        check("run 2: the recovered work counts as unsaved", st["unsaved"] == "1", st)
        h2 = c.history()
        check("run 2: every History step has the same length and label", h2 == hist, f"{h2} vs {hist}")
        st = c.state()
        check("run 2: the cursor is back where it was after clicking through", st["cursor"] == before["cursor"] and st["frames"] == before["frames"], st)
        c.ask("ACTION edit.redo")
        st = c.state()
        check("run 2: Redo works on the recovered history", st["cursor"] == "5" and st["undo"] == "Insert Silence", st)
        c.ask("SELECT 0 1000")
        c.ask("ACTION edit.delete")
        st = c.state()
        check("run 2: a new edit works after recovery", st["steps"] == "7" and st["undo"] == "Delete", st)
        for _ in range(100):
            if journal_ok(sdir, 7, 6) and c.autosave()["pending"] == "0":
                break
            time.sleep(0.2)
        check("run 2: the recovered session keeps its journal current", journal_ok(sdir, 7, 6), c.autosave())
        c.ask("QUIT discard")
        p2.wait(20)
    except Exception as e:
        check("run 2", False, f"{type(e).__name__}: {e}")
        p2.kill()
    err2.close()
    check("run 2 quit cleanly and removed the recovered session folder", p2.returncode == 0 and not os.path.exists(sdir), f"exit {p2.returncode}, folder exists: {os.path.exists(sdir)}")
    return finish(work, logs, checks, keep)


def finish(work, logs, checks, keep):
    bad = []
    for name in sorted(os.listdir(logs)) if os.path.isdir(logs) else []:
        for line in open(os.path.join(logs, name), errors="replace"):
            if "ERROR" in line or "panicked" in line:
                bad.append(f"{name}: {line.strip()}")
    for name in ("stderr1.log", "stderr2.log"):
        p = os.path.join(work, name)
        if os.path.exists(p):
            bad += [f"{name}: {l.strip()}" for l in open(p, errors="replace") if "panicked" in l.lower()]
    print(("PASS " if not bad else "FAIL ") + "no ERROR line in the app's logs", bad[:5] or "")
    checks.append(("no ERROR line in the logs", not bad, bad[:5]))
    failed = [c for c in checks if not c[1]]
    print(f"{len(checks) - len(failed)}/{len(checks)} checks passed")
    if keep or failed:
        print("scratch kept:", work)
    else:
        shutil.rmtree(work, ignore_errors=True)
    return 1 if failed else 0


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47110)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    sys.exit(run(a.exe, a.port, a.keep))
