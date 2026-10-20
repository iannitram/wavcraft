#!/usr/bin/env python3
"""f53 scripted run: Transport previous/next marker and go-to-start/end.

    python3 tools/alpha/f53.py --exe <release wavcraft>

Starts the release app (silent clock, WAVCRAFT_NO_AUDIO=1) with a fresh scratch HOME and a 10 s
synthetic file, adds markers at 1 s, 3 s and 6 s, then checks through STATE that
ACTION transport.next_marker / prev_marker / start / end move the cursor when stopped, and that
during playback the marker jumps move the playhead and playback carries on. Fails on any ERROR
line in the app's log.
"""
import argparse, glob, os, shutil, subprocess, sys, tempfile, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from f23 import Control, make_wav, RATE, SECS

PORT = 47153


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    o = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f53.", dir=root)
    home, logs = os.path.join(work, "home"), os.path.join(work, "logs")
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    make_wav(wav)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1", WAVCRAFT_NO_AUDIO="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([o.exe, wav], env=env, stdout=err, stderr=err)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + (f"  [{detail}]" if detail else ""), flush=True)

    try:
        for _ in range(200):
            if os.path.exists(token):
                break
            time.sleep(0.1)
        c = Control(PORT, open(token).read().strip())
        st = c.wait_open()
        print(f"MEASURED with output={st['output']} (silent test clock)")
        cur = lambda: c.state()["selection"]
        for s in (1, 3, 6):
            c.ask(f"SELECT {RATE * s} {RATE * s}")
            c.ask("ACTION marker.add")
        check("three markers added", c.ask("MARKERS").startswith("OK count=3"), c.ask("MARKERS"))

        # Stopped: the buttons move the cursor.
        c.ask("ACTION transport.start")
        check("start puts the cursor at 0", cur() == "0-0", cur())
        c.ask("ACTION transport.next_marker")
        check("next marker -> 1 s", cur() == f"{RATE}-{RATE}", cur())
        c.ask("ACTION transport.next_marker")
        c.ask("ACTION transport.next_marker")
        check("... 3 s then 6 s", cur() == f"{RATE * 6}-{RATE * 6}", cur())
        c.ask("ACTION transport.next_marker")
        check("no marker after the last: the cursor stays", cur() == f"{RATE * 6}-{RATE * 6}", cur())
        c.ask("ACTION transport.prev_marker")
        check("previous marker -> 3 s", cur() == f"{RATE * 3}-{RATE * 3}", cur())
        c.ask("ACTION transport.end")
        check("end puts the cursor at the end", cur() == f"{RATE * SECS}-{RATE * SECS}", cur())

        # Playing: the jumps move the playhead and play carries on.
        c.ask("ACTION transport.start")
        c.ask("ACTION transport.play")
        time.sleep(0.3)
        c.ask("ACTION transport.next_marker")
        st = c.state()
        check("next marker during play jumps to 1 s and keeps playing", st["play"] == "playing" and RATE <= int(st["pos"]) < RATE + 12000, f"play={st['play']} pos={st['pos']}")
        time.sleep(0.3)
        p1 = int(c.state()["pos"])
        check("... and the playhead keeps moving", p1 > RATE + 8000, f"pos={p1}")
        c.ask("ACTION transport.next_marker")
        st = c.state()
        check("next again jumps to 3 s, still playing", st["play"] == "playing" and RATE * 3 <= int(st["pos"]) < RATE * 3 + 12000, f"pos={st['pos']}")
        c.ask("ACTION transport.prev_marker")
        st = c.state()
        check("previous jumps back to 1 s, still playing", st["play"] == "playing" and RATE <= int(st["pos"]) < RATE + 12000, f"pos={st['pos']}")
        c.ask("ACTION transport.stop")
        check("stopped", c.state()["play"] == "stopped")

        check("QUIT", c.ask("QUIT discard") == "OK")
        proc.wait(15)
    except Exception as e:
        check("script", False, f"{type(e).__name__}: {e}")
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()
        err.close()
    errors = []
    for f in glob.glob(os.path.join(logs, "*")) + [os.path.join(work, "stderr.log")]:
        try:
            errors += [l.strip() for l in open(f, errors="replace") if "ERROR" in l or "panicked" in l]
        except OSError:
            pass
    check("no ERROR line in the app's log", not errors, "; ".join(errors[:3]))
    ok = all(x[1] for x in checks)
    if not ok:
        print(f"scratch kept for the logs: {work}")
    else:
        shutil.rmtree(work, ignore_errors=True)
    print("RESULT", "pass" if ok else "fail")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
