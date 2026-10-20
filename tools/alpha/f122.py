#!/usr/bin/env python3
"""f122 (Video panel) scripted run: a synthetic WAV is opened, an ffmpeg-made 2 s H.264 test clip
(testsrc, 24 fps) is attached with `VIDEO attach`, and `VIDEO frame <seconds>` must answer the
frame index and size. Also: an MPEG-4 Part 2 clip (a codec the decoder does not have), a file
with no video track and a missing file must be reported, not crash. The app's log must hold no
ERROR line. Run: python3 tools/alpha/f122.py --exe <path to the release binary>"""
import argparse, glob, os, shutil, socket, subprocess, sys, tempfile, time

PORT = 47222
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, ".."))
import golden  # noqa: E402  (the Control class: one line per request, one per answer)


def ffmpeg(*args):
    subprocess.run(["ffmpeg", "-v", "error", "-y", *args], check=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    exe = ap.parse_args()
    keep = exe.keep
    exe = exe.exe
    base = os.environ.get("F122_SCRATCH") or tempfile.gettempdir()
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f122.", dir=base)
    for d in ("home", "logs", "sessions"):
        os.makedirs(os.path.join(work, d))
    env = dict(os.environ, HOME=os.path.join(work, "home"), WAVCRAFT_LOG_DIR=os.path.join(work, "logs"),
               WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_CONTROL_PORT=str(PORT),
               WAVCRAFT_CONTROL_TOKEN_FILE=os.path.join(work, "control.token"))
    wav, mp4, odd = (os.path.join(work, n) for n in ("a.wav", "clip.mp4", "odd.mp4"))
    ffmpeg("-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=2", "-ac", "2", "-c:a", "pcm_s16le", wav)
    ffmpeg("-f", "lavfi", "-i", "testsrc=duration=2:size=160x96:rate=24", "-c:v", "libx264", "-profile:v", "baseline", "-pix_fmt", "yuv420p", "-g", "12", mp4)
    ffmpeg("-f", "lavfi", "-i", "testsrc=duration=1:size=64x48:rate=24", "-c:v", "mpeg4", odd)
    checks = []

    def check(label, ok, detail):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + "  [" + str(detail) + "]", flush=True)

    proc = subprocess.Popen([exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ok_run = False
    try:
        c = None
        for _ in range(60):
            if os.path.exists(env["WAVCRAFT_CONTROL_TOKEN_FILE"]):
                break
            time.sleep(0.5)
        token = open(env["WAVCRAFT_CONTROL_TOKEN_FILE"]).read().strip()
        c = golden.Control(PORT, token)
        st = c.state()
        check("the WAV is open", st.get("frames") == "96000", st)
        check("no video: VIDEO frame says so", c.ask("VIDEO frame 0.5").startswith("ERR no video"), "")
        check("attach answers at once", c.ask(f"VIDEO attach {mp4}") == "OK", "")
        a = c.ask("VIDEO frame 1.0")
        check("frame at 1.0 s is index 24, 160x96", a.startswith("OK index=24 size=160x96"), a)
        a = c.ask("VIDEO frame 0")
        check("frame at 0 s is index 0", a.startswith("OK index=0 size=160x96"), a)
        a = c.ask("VIDEO frame 1.9999")
        check("frame near the end is index 47", a.startswith("OK index=47 size=160x96"), a)
        a = c.ask("VIDEO frame 0.5")
        check("frame at 0.5 s is index 12 (decoding back from a later frame)", a.startswith("OK index=12 size=160x96"), a)
        a = c.ask("VIDEO frame -1")
        check("a negative time has no picture", a.startswith("ERR"), a)
        s = c.ask("VIDEO status")
        check("status names the codec and size", s.startswith("OK 160x96") and "48 frames" in s, s)
        c.ask(f"VIDEO attach {odd}")
        a = c.ask("VIDEO frame 0")
        s = c.ask("VIDEO status")
        check("an undecodable codec is reported", a.startswith("ERR") and "not supported" in s, f"{a} | {s}")
        c.ask(f"VIDEO attach {wav}")
        a = c.ask("VIDEO frame 0")
        check("a file that is not a movie is reported", a.startswith("ERR") and "Could not open" in c.ask("VIDEO status"), a)
        c.ask(f"VIDEO attach {os.path.join(work, 'missing.mov')}")
        check("a missing file is reported", c.ask("VIDEO frame 0").startswith("ERR"), "")
        check("the app is still alive and answering", c.state().get("frames") == "96000", "")
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
        if not keep:
            shutil.rmtree(work, ignore_errors=True)
    print("RESULT", "pass" if ok_run else "FAIL", f"{sum(ok for _, ok, _ in checks)}/{len(checks)} checks")
    sys.exit(0 if ok_run else 1)


main()
