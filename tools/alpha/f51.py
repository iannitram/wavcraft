"""f51 scripted run: save options (A262 include metadata, A263 progress and cancel).

Starts the release app with a scratch HOME, opens a synthetic 16-bit mono WAV, saves with
meta=1 and meta=0 (checking the saved files' chunks with a small RIFF reader), then starts a
slow save of an edited document, cancels it, and checks the original is byte-identical, no
temporary file is left, and the document is still unsaved. Fails on any ERROR line in the log.
"""
import argparse, glob, json, math, os, shutil, signal, socket, struct, subprocess, sys, tempfile, time, wave

RATE, FRAMES = 48000, 65536 * 8


def chunk_ids(path):
    b = open(path, "rb").read()
    assert b[:4] == b"RIFF" and struct.unpack("<I", b[4:8])[0] == len(b) - 8, "bad RIFF header or size"
    at, ids = 12, []
    while at + 8 <= len(b):
        ids.append(b[at:at + 4].decode("latin1"))
        n = struct.unpack("<I", b[at + 4:at + 8])[0]
        at += 8 + n + (n & 1)
    assert at == len(b), "chunks do not tile the file"
    return ids


class Control:
    def __init__(self, port, token):
        for _ in range(80):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.25)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        assert self.ask(f"TOKEN {token}") == "OK", "token refused"

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self):
        head = self.ask("STATE").partition(" undo=")[0]
        return dict(kv.split("=", 1) for kv in head.split()[1:])

    def wait_idle(self, timeout=60):
        t0 = time.time()
        while time.time() - t0 < timeout:
            st = self.state()
            if st["busy"] == "0":
                return st
            time.sleep(0.1)
        raise RuntimeError("still busy")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f51.", dir=scratch_root)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + (f"  [{detail}]" if detail else ""), flush=True)

    try:
        home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
        os.makedirs(home)
        src = os.path.join(work, "src.wav")
        with wave.open(src, "wb") as w:
            w.setnchannels(1); w.setsampwidth(2); w.setframerate(RATE)
            w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(i * 0.05))) for i in range(FRAMES)))
        port, token = 47151, os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
                   WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token,
                   WAVCRAFT_TEST_SAVE_CHUNK="1", WAVCRAFT_TEST_SAVE_DELAY_MS="400", RUST_BACKTRACE="1")
        err = open(os.path.join(work, "stderr.log"), "w")
        proc = subprocess.Popen([exe, src], env=env, stdout=err, stderr=err)
        try:
            for _ in range(80):
                if os.path.exists(token):
                    break
                time.sleep(0.25)
            c = Control(port, open(token).read().strip())
            t0 = time.time()
            while int(c.state()["frames"]) == 0 and time.time() - t0 < 60:
                time.sleep(0.2)
            c.wait_idle()
            check("file opened", int(c.state()["frames"]) == FRAMES, c.state()["frames"])
            check("option defaults on", c.state()["meta"] == "1")

            on = os.path.join(work, "meta_on.wav")
            check("SAVE meta=1 accepted", c.ask(f"SAVE {on} meta=1") == "OK")
            seen = []
            while True:
                st = c.state()
                if st["busy"] == "0":
                    break
                seen.append(st["save"])
                time.sleep(0.1)
            check("STATE reports save progress while saving", any(s.isdigit() for s in seen), f"{len(seen)} polls, e.g. {seen[:3]}...{seen[-2:]}")
            check("progress rises", [int(s) for s in seen if s.isdigit()] == sorted(int(s) for s in seen if s.isdigit()) and len(set(seen)) > 1, seen)
            check("meta on: fmt, data and the extra chunk", chunk_ids(on) == ["fmt ", "data", "TEST"], chunk_ids(on))

            off = os.path.join(work, "meta_off.wav")
            check("SAVE meta=0 accepted", c.ask(f"SAVE {off} meta=0") == "OK")
            st = c.wait_idle()
            check("meta off: only fmt and data", chunk_ids(off) == ["fmt ", "data"], chunk_ids(off))
            check("STATE reports the option and idle save", st["meta"] == "0" and st["save"] == "-" and st["unsaved"] == "0", f"meta={st['meta']} save={st['save']}")
            check("same audio either way", open(on, "rb").read()[44:44 + FRAMES * 2] == open(off, "rb").read()[44:44 + FRAMES * 2])

            # An edit makes the document unsaved; a slow save of it is then cancelled.
            c.ask("SELECT 1000 2000")
            check("edit applied", c.ask("ACTION edit.delete").startswith("OK"))
            check("document is unsaved", c.state()["unsaved"] == "1")
            before = open(off, "rb").read()
            check("SAVE to the file's own path", c.ask("SAVE") == "OK")
            t0 = time.time()
            while time.time() - t0 < 30 and not (c.state()["save"].isdigit() and int(c.state()["save"]) > 0):
                time.sleep(0.05)
            mid = c.state()["save"]
            check("the save is part way", mid.isdigit() and 0 < int(mid) < 100, mid)
            check("SAVE_CANCEL accepted", c.ask("SAVE_CANCEL") == "OK")
            st = c.wait_idle()
            check("original byte-identical after cancel", open(off, "rb").read() == before)
            left = [f for f in os.listdir(work) if "wavcraft-tmp" in f]
            check("no temporary file left", not left, left)
            check("document still unsaved", st["unsaved"] == "1" and st["save"] == "-", st)
            check("SAVE_CANCEL with no save answers ERR", c.ask("SAVE_CANCEL").startswith("ERR"))
            check("QUIT discard", c.ask("QUIT discard") == "OK")
            proc.wait(20)
        finally:
            if proc.poll() is None:
                proc.send_signal(signal.SIGTERM)
                try:
                    proc.wait(10)
                except subprocess.TimeoutExpired:
                    proc.kill()
        err.close()
        lines = []
        for f in glob.glob(os.path.join(logs, "*")) + [os.path.join(work, "stderr.log")]:
            lines += [l for l in open(f, errors="replace").read().splitlines() if "ERROR" in l or "panicked" in l]
        check("no ERROR line in the app's log", not lines, lines[:5])
    except Exception as e:
        check("run completed", False, f"{type(e).__name__}: {e}")
    finally:
        shutil.rmtree(work, ignore_errors=True)
    bad = [c for c in checks if not c[1]]
    print(f"{len(checks) - len(bad)}/{len(checks)} checks passed")
    sys.exit(1 if bad else 0)


main()
