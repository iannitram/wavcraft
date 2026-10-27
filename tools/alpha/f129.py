#!/usr/bin/env python3
"""f129 (Video track) scripted run: a session gets a Video track and an ffmpeg-made 2 s, 24 fps
H.264 clip placed at 1.0 s. `VIDEO frame <session_s>` must answer the picture of the clip under
that session time (frame 0 at 1.0 s, frame 12 at 1.5 s, the last frame near 2.99 s) and refuse
times with no clip (0.5 s and 3.5 s). The app's log must hold no ERROR line.
Run: python3 tools/alpha/f129.py --exe <path to the release binary>"""
import argparse, glob, os, shutil, subprocess, sys, tempfile, time

PORT = 47229
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, ".."))
import golden  # noqa: E402


def ffmpeg(*args):
    subprocess.run(["ffmpeg", "-v", "error", "-y", *args], check=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    base = os.environ.get("F129_SCRATCH") or tempfile.gettempdir()
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f129.", dir=base)
    for d in ("home", "logs", "sessions"):
        os.makedirs(os.path.join(work, d))
    env = dict(os.environ, HOME=os.path.join(work, "home"), WAVCRAFT_LOG_DIR=os.path.join(work, "logs"),
               WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_CONTROL_PORT=str(PORT),
               WAVCRAFT_CONTROL_TOKEN_FILE=os.path.join(work, "control.token"))
    mp4 = os.path.join(work, "clip.mp4")
    ffmpeg("-f", "lavfi", "-i", "testsrc=duration=2:size=160x96:rate=24", "-c:v", "libx264", "-profile:v", "baseline", "-pix_fmt", "yuv420p", "-g", "12", mp4)
    checks = []

    def check(label, ok, detail):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + "  [" + str(detail) + "]", flush=True)

    proc = subprocess.Popen([a.exe], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ok_run = False
    try:
        for _ in range(60):
            if os.path.exists(env["WAVCRAFT_CONTROL_TOKEN_FILE"]):
                break
            time.sleep(0.5)
        c = golden.Control(PORT, open(env["WAVCRAFT_CONTROL_TOKEN_FILE"]).read().strip())
        check("session", c.ask("SESSION new Demo").startswith("OK"), "")
        check("TRACK add video", c.ask("TRACK add video").startswith("OK"), "")
        r = c.ask(f"CLIP add video {mp4} 1.0")
        check("CLIP add on the video track", r == "OK clips=1", r)
        st = c.ask("SESSION state")
        check("state lists the video track and clip at 1.0 s", '"kind":"video"' in st and '"start":1.0' in st and '"length":2.0' in st, st)
        r = c.ask("VIDEO frame 0.5")
        check("no clip under 0.5 s", r.startswith("ERR"), r)
        r = c.ask("VIDEO frame 1.0")
        check("session 1.0 s is frame 0", r.startswith("OK index=0 size=160x96"), r)
        r = c.ask("VIDEO frame 1.5")
        check("session 1.5 s is frame 12", r.startswith("OK index=12 size=160x96"), r)
        r = c.ask("VIDEO frame 2.99")
        check("session 2.99 s is the last frame (47)", r.startswith("OK index=47 size=160x96"), r)
        r = c.ask("VIDEO frame 3.5")
        check("no clip under 3.5 s", r.startswith("ERR"), r)
        r = c.ask("TRACK add")
        r = c.ask(f"CLIP add 1 {mp4} 0")
        check("a movie on an audio track is refused", r.startswith("ERR"), r)
        check("QUIT", c.ask("QUIT") == "OK", "")
        proc.wait(timeout=30)
        errors = []
        for f in glob.glob(os.path.join(work, "logs", "*")):
            if os.path.isfile(f):
                errors += [l.strip() for l in open(f, errors="replace") if "ERROR" in l or "panicked" in l]
        check("no ERROR line in the app's log", not errors, errors[:3])
        ok_run = all(ok for _, ok, _ in checks)
    finally:
        if proc.poll() is None:
            proc.kill()
        if not a.keep:
            shutil.rmtree(work, ignore_errors=True)
    print("RESULT", "pass" if ok_run else "FAIL", f"{sum(ok for _, ok, _ in checks)}/{len(checks)} checks")
    sys.exit(0 if ok_run else 1)


main()
