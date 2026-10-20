#!/usr/bin/env python3
"""f99: open the audio track of a MOV (made with ffmpeg) through the control channel."""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time

ap = argparse.ArgumentParser()
ap.add_argument("--exe", required=True)
args = ap.parse_args()
if not shutil.which("ffmpeg"):
    print("SKIP: ffmpeg is not installed")
    sys.exit(0)

PORT = 47199
base = os.path.expanduser("~/dev/craft/_wt/scratch")
os.makedirs(base, exist_ok=True)
scratch = tempfile.mkdtemp(prefix="f99.", dir=base)
logs, sessions = os.path.join(scratch, "log"), os.path.join(scratch, "sessions")
os.makedirs(logs)
os.makedirs(sessions)
token_file = os.path.join(scratch, "token")
mov = os.path.join(scratch, "t.mov")
subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100:duration=3",
                "-f", "lavfi", "-i", "testsrc=size=64x64:rate=10:duration=3", "-ac", "2", "-c:a", "pcm_s16le", "-c:v", "mpeg4", mov], check=True)
env = dict(os.environ, HOME=scratch, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions,
           WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token_file)
proc = subprocess.Popen([args.exe], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
ok = False
try:
    for _ in range(120):
        try:
            s = socket.create_connection(("127.0.0.1", PORT), timeout=60)
            break
        except OSError:
            time.sleep(0.5)
    else:
        raise RuntimeError("no control channel")
    f = s.makefile("rw", encoding="utf-8")

    def ask(line):
        f.write(line + "\n")
        f.flush()
        return f.readline().strip()

    for _ in range(40):
        if os.path.exists(token_file) and os.path.getsize(token_file) >= 64:
            break
        time.sleep(0.25)
    assert ask("TOKEN " + open(token_file).read().strip()) == "OK"
    print("OPEN:", ask("OPEN " + mov))
    st = {}
    for _ in range(300):
        a = ask("STATE")
        st = dict(kv.split("=", 1) for kv in a.split(" undo=")[0].split()[1:])
        if st.get("busy") == "0" and st.get("frames", "0") != "0":
            break
        time.sleep(0.2)
    print("STATE:", st)
    ok = st.get("frames") == str(3 * 44100) and st.get("channels") == "2" and st.get("rate") == "44100"
    print("QUIT:", ask("QUIT"))
finally:
    try:
        proc.wait(timeout=15)
    except subprocess.TimeoutExpired:
        proc.kill()
    errs = []
    for d, _, fs in os.walk(logs):
        for n in fs:
            errs += [l for l in open(os.path.join(d, n), errors="replace") if "ERROR" in l]
    for l in errs:
        print("LOG ERROR:", l.strip())
    shutil.rmtree(scratch, ignore_errors=True)
    ok = ok and not errs
print("PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
