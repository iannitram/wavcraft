#!/usr/bin/env python3
"""f06 alpha run (roadmap B003): the background job runner, through the control channel.

Starts the release app with a fresh scratch HOME, opens a synthetic 60 s stereo WAV, then checks:
a test-only slow job (JOB testslow) shows busy=1 and progress between 0 and 100 while the channel
keeps answering, edits are refused while it runs, it ends with one "Test Job" undo step and the
same length; a second job is cancelled part way and leaves the history alone; a 10-minute
Insert Silence is one undo step; QUIT; the app's log holds no ERROR line.
"""
import argparse, os, shutil, struct, subprocess, sys, tempfile, time, wave, math

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from golden import Control, free_port  # noqa: E402

RATE, SECONDS = 48000, 60


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
        for s in range(SECONDS):
            frames = b"".join(struct.pack("<hh", int(8000 * math.sin(2 * math.pi * 220 * (s * RATE + i) / RATE)),
                                          int(8000 * math.sin(2 * math.pi * 330 * (s * RATE + i) / RATE))) for i in range(RATE))
            w.writeframes(frames)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    o = ap.parse_args()
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f06.", dir=scratch_root)
    checks = []

    def check(label, ok, detail):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    proc = None
    try:
        home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
        os.makedirs(home)
        wav = os.path.join(work, "tone.wav")
        make_wav(wav)
        env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"), RUST_BACKTRACE="1")
        port, token = 47106, os.path.join(work, "control.token")
        env.update(WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
        err = open(os.path.join(work, "stderr.log"), "w")
        proc = subprocess.Popen([o.exe, wav], env=env, stdout=err, stderr=err)
        c = Control(port, open_token(token))
        st = c.state()
        frames = int(st["frames"])
        check("the file opens", frames == SECONDS * RATE and st["steps"] == "1", st)

        # 1. A slow job: busy, progress rising, the channel still answering, edits refused.
        a = c.ask("JOB testslow 150")
        seen, refused, answered_fast = [], None, True
        t0 = time.time()
        while time.time() - t0 < 60:
            t1 = time.time()
            st = c.state(wait=False)
            answered_fast &= time.time() - t1 < 1.0
            if st["busy"] == "0":
                break
            seen.append(int(st["progress"]))
            if refused is None:
                refused = c.ask("ACTION edit.delete")
            time.sleep(0.2)
        check("a job starts", a == "OK", a)
        check("progress shows between 0 and 100 while busy=1", any(0 < p < 100 for p in seen) and seen == sorted(seen), seen[:20])
        check("the control channel keeps answering while the job runs", answered_fast and len(seen) >= 3, f"{len(seen)} answers")
        check("an edit is refused while the job runs", refused is not None and refused.startswith("ERR"), refused)
        st = c.state()
        check("it ends with one 'Test Job' undo step and the same length", st["steps"] == "2" and st["undo"] == "Test Job" and int(st["frames"]) == frames and st["progress"] == "0", st)

        # 2. Cancel part way.
        c.ask("JOB testslow 150")
        t0 = time.time()
        while time.time() - t0 < 30 and int(c.state(wait=False)["progress"]) < 5:
            time.sleep(0.1)
        cancel = c.ask("JOB cancel")
        st = c.state()
        check("Cancel stops the job and adds no undo step", cancel == "OK" and st["steps"] == "2" and st["undo"] == "Test Job" and int(st["frames"]) == frames, f"{cancel}; {st}")

        # 3. A long Insert Silence is one undo step.
        c.ask("SELECT 0 0")
        a = c.ask("INSERT_SILENCE 600")
        st = c.state()
        check("Insert Silence of 10 minutes is one undo step", st["steps"] == "3" and st["undo"] == "Insert Silence" and int(st["frames"]) == frames + 600 * RATE, f"{a}; {st}")

        check("QUIT discard", c.ask("QUIT discard") == "OK", "")
        proc.wait(20)
    except Exception as e:
        check("the run completed", False, f"{type(e).__name__}: {e}")
    finally:
        if proc is not None and proc.poll() is None:
            proc.kill()
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(os.path.join(work, "logs")) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    ok = all(c["ok"] for c in checks)
    for c in checks:
        print(("PASS " if c["ok"] else "FAIL ") + c["check"] + "  " + str(c["detail"])[:200])
    if not ok:
        print("scratch kept for the logs:", work)
    else:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


def open_token(path, tries=60):
    for _ in range(tries):
        if os.path.exists(path) and os.path.getsize(path) >= 64:
            return open(path).read().strip()
        time.sleep(0.5)
    raise RuntimeError("no token file")


if __name__ == "__main__":
    main()
