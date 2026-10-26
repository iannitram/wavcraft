#!/usr/bin/env python3
"""f43 alpha run: Save Selection As and Export through the control channel.

    python3 tools/alpha/f43.py --exe <path to the release wavcraft>

A synthetic 16-bit stereo WAV is opened in a scratch HOME. Checked: SAVE_SELECTION writes exactly
the selected frames (both channels);
EXPORT writes the whole document; after each the document's path is unchanged and
the unsaved flag is as before (clean, then dirty after an edit); a write to an unwritable folder
fails with ERR/no temp file; QUIT discard; no ERROR line in the app's log.
"""
import argparse, os, shutil, signal, socket, struct, subprocess, sys, tempfile, time, wave

RATE, FRAMES = 48000, 3 * 48000
PORT = int(os.environ.get("WAVCRAFT_CONTROL_PORT", "47143"))


class Control:
    def __init__(self, port, token, tries=80):
        for _ in range(tries):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("the control channel refused the token")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self, timeout=60):
        t0 = time.time()
        while True:
            answer = self.ask("STATE")
            answer, _, path = answer.partition(" path=")
            head, _, labels = answer.partition(" undo=")
            d = dict(kv.split("=", 1) for kv in head.split()[1:])
            d["path"] = path
            if d.get("busy") == "0" or time.time() - t0 > timeout:
                return d
            time.sleep(0.1)


def samples(path):
    with wave.open(path, "rb") as w:
        assert w.getsampwidth() == 2, w.getsampwidth()
        n, ch = w.getnframes(), w.getnchannels()
        return ch, struct.unpack(f"<{n * ch}h", w.readframes(n))


def make_source(path):
    left = [int(12000 * ((k % 480) / 240 - 1)) for k in range(FRAMES)]
    right = [(k * 7) % 20000 - 10000 for k in range(FRAMES)]
    inter = [v for pair in zip(left, right) for v in pair]
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
        w.writeframes(struct.pack(f"<{len(inter)}h", *inter))
    return left, right


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--exe", required=True); ap.add_argument("--keep", action="store_true")
    exe = ap.parse_args().exe
    base = os.path.expanduser("~/dev/craft/_wt/scratch"); os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f43.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    src = os.path.join(work, "source.wav")
    left, right = make_source(src)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs, XDG_CONFIG_HOME=os.path.join(home, ".config"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([exe, src], env=env, stdout=err, stderr=err)
    checks = []

    def check(name, ok, detail=""):
        checks.append((name, bool(ok), detail)); print(("PASS " if ok else "FAIL ") + name, detail)

    try:
        for _ in range(80):
            if os.path.exists(token): break
            time.sleep(0.25)
        c = Control(PORT, open(token).read().strip())
        st = c.state()
        for _ in range(100):
            if int(st["frames"]) == FRAMES: break
            time.sleep(0.2); st = c.state()
        check("opened", int(st["frames"]) == FRAMES, st)
        own = st["path"]
        check("STATE shows the document's path and clean flag", own.endswith("source.wav") and st["unsaved"] == "0", (own, st["unsaved"]))
        a, b = 20000, 100000
        c.ask(f"SELECT {a} {b}")
        out = os.path.join(work, "sel.wav")
        check("SAVE_SELECTION answers OK", c.ask(f"SAVE_SELECTION {out}") == "OK")
        st = c.state()
        ch, data = samples(out)
        want = [v for pair in zip(left[a:b], right[a:b]) for v in pair]
        check("saved selection samples equal the source range", ch == 2 and list(data) == want, f"{len(data) // 2} frames")
        check("document path and clean flag unchanged after SAVE_SELECTION", st["path"] == own and st["unsaved"] == "0", (st["path"], st["unsaved"]))
        # An edit makes it unsaved; export must not clear that mark nor change the path.
        r = c.ask("ACTION edit.delete")
        check("edit made", r.startswith("OK"), r)
        st = c.state(); n_after = int(st["frames"])
        check("document is unsaved after the edit", st["unsaved"] == "1" and n_after == FRAMES - (b - a), st)
        exp = os.path.join(work, "export.wav")
        check("EXPORT answers OK", c.ask(f"EXPORT {exp}") == "OK")
        st = c.state()
        ch, data = samples(exp)
        want = [v for pair in zip(left[:a] + left[b:], right[:a] + right[b:]) for v in pair]
        # The delete's 2 ms crossfade changes samples next to the join (about 100 frames): compare away from it.
        m = 500
        same = list(data[: 2 * (a - m)]) == want[: 2 * (a - m)] and list(data[2 * (a + m):]) == want[2 * (a + m):]
        check("export holds the whole edited document", ch == 2 and len(data) == len(want) and same, f"{len(data) // 2} frames")
        check("document path unchanged and still unsaved after EXPORT", st["path"] == own and st["unsaved"] == "1", (st["path"], st["unsaved"]))
        # A failing write: the folder does not exist.
        bad = os.path.join(work, "nodir", "x.wav")
        c.ask("SELECT 0 1000")
        r = c.ask(f"SAVE_SELECTION {bad}"); st = c.state()
        check("a write to a missing folder leaves no file", not os.path.exists(bad) and not os.path.exists(os.path.dirname(bad)), r)
        check("empty selection refused", c.ask("SELECT 5 5").startswith("OK") and c.ask(f"SAVE_SELECTION {os.path.join(work, 'e.wav')}").startswith("ERR"))
        check("no temp files beside the outputs", not [f for f in os.listdir(work) if "wavcraft-tmp" in f], os.listdir(work))
        check("QUIT discard", c.ask("QUIT discard") == "OK")
        proc.wait(20)
    except Exception as e:
        check("script ran", False, f"{type(e).__name__}: {e}")
    finally:
        if proc.poll() is None:
            proc.send_signal(signal.SIGTERM)
            try: proc.wait(10)
            except subprocess.TimeoutExpired: proc.kill()
        err.close()
    lines = []
    for d, _, fs in os.walk(logs):
        for f in fs:
            lines += open(os.path.join(d, f), errors="replace").read().splitlines()
    lines += open(os.path.join(work, "stderr.log"), errors="replace").read().splitlines()
    bad = [l for l in lines if "ERROR" in l or "panicked" in l]
    check("no ERROR line in the app's log", not bad, bad[:5])
    ok = all(x[1] for x in checks)
    if not args_keep(): shutil.rmtree(work, ignore_errors=True)
    else: print("kept", work)
    print("RESULT", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


def args_keep():
    return "--keep" in sys.argv


main()
