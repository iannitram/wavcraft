#!/usr/bin/env python3
"""Alpha check for feature f13: Edit > Insert > Audio from File (control verb INSERT_FILE).

    python3 tools/alpha/f13.py --exe <wavcraft> [--port 47113] [--keep]

Starts the release app with HOME and the session/log folders in a fresh scratch folder, opens a
synthetic 48 kHz stereo WAV, and through the control channel: inserts a mono 48 kHz file at the
cursor (length grows, History label, selection is the inserted range, Undo restores), inserts a
stereo file over a selection (replaces it), refuses a 44.1 kHz file (no new step, a message in
the app's log), then QUITs. Fails on any ERROR line in the app's log or stderr.
Drag-and-drop is an OS event the control channel cannot send; its path choice is unit-tested.
"""
import argparse, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

LABEL = "Insert Audio from File"


def write_wav(path, rate, channels, seconds, freq):
    with wave.open(path, "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(rate)
        n = int(rate * seconds)
        frames = bytearray()
        for i in range(n):
            v = int(12000 * math.sin(2 * math.pi * freq * i / rate))
            frames += struct.pack("<h", v) * channels
        w.writeframes(bytes(frames))


def wait_idle(c, timeout=60):
    t0 = time.time()
    while True:
        st = c.state(wait=False)
        if st.get("busy") == "0" and int(st.get("frames", 0)) >= 0 and (time.time() - t0 > 0.3):
            return st
        if time.time() - t0 > timeout:
            return st
        time.sleep(0.1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47113)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f13.", dir=scratch_root)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    rate = 48000
    main_wav, mono_wav, stereo_wav, odd_wav = (os.path.join(work, n) for n in ("main.wav", "mono.wav", "stereo.wav", "r44.wav"))
    write_wav(main_wav, rate, 2, 10, 220)
    write_wav(mono_wav, rate, 1, 2, 440)
    write_wav(stereo_wav, rate, 2, 1, 880)
    write_wav(odd_wav, 44100, 2, 1, 330)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(a.port), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([a.exe, main_wav], env=env, stdout=err, stderr=err)
    checks = []

    def check(label, ok, detail):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"), flush=True)

    try:
        for _ in range(80):
            if os.path.exists(token):
                break
            time.sleep(0.25)
        c = Control(a.port, open(token).read().strip())
        st = wait_idle(c)
        f0 = int(st["frames"])
        check("the base file opens", f0 == 10 * rate and st["channels"] == "2", st)
        c.ask(f"SELECT {2 * rate} {2 * rate}")
        r = c.ask(f"INSERT_FILE {mono_wav}")
        st = wait_idle(c)
        check("INSERT_FILE at the cursor adds the file's length and one step labelled like Audition's history entry",
              r == "OK" and int(st["frames"]) == f0 + 2 * rate and st["undo"] == LABEL and st["steps"] == "2" and st["channels"] == "2" and st["unsaved"] == "1", f"{r}; {st}")
        check("the inserted audio is selected", st["selection"] == f"{2 * rate}-{4 * rate}", st)
        c.ask("ACTION edit.undo")
        st = wait_idle(c)
        check("Undo takes it out in one step", int(st["frames"]) == f0 and st["redo"] == LABEL, st)
        c.ask("ACTION edit.redo")
        st = wait_idle(c)
        check("Redo puts it back", int(st["frames"]) == f0 + 2 * rate, st)
        base = int(st["frames"])
        c.ask(f"SELECT {rate} {4 * rate}")
        r = c.ask(f"INSERT_FILE {stereo_wav}")
        st = wait_idle(c)
        check("INSERT_FILE over a 3 s selection replaces it with the 1 s file",
              r == "OK" and int(st["frames"]) == base - 3 * rate + rate and st["selection"] == f"{rate}-{2 * rate}" and st["steps"] == "3", f"{r}; {st}")
        steps = st["steps"]
        r = c.ask(f"INSERT_FILE {odd_wav}")
        st = wait_idle(c)
        check("a 44.1 kHz file is refused: no new step, same length", r == "OK" and st["steps"] == steps and int(st["frames"]) == base - 2 * rate, f"{r}; {st}")
        r = c.ask("INSERT_FILE " + os.path.join(work, "missing.wav"))
        st = wait_idle(c)
        check("a missing file is refused too", st["steps"] == steps, f"{r}; {st}")
        r = c.ask("QUIT discard")
        check("QUIT", r == "OK", r)
        proc.wait(20)
    except Exception as e:
        check("script", False, f"{type(e).__name__}: {e}")
        proc.kill()
    finally:
        if proc.poll() is None:
            proc.kill()
        err.close()
    logtext = ""
    for d, _, files in os.walk(logs):
        for f in files:
            logtext += open(os.path.join(d, f), errors="replace").read()
    errtext = open(os.path.join(work, "stderr.log"), errors="replace").read()
    bad = [l for l in (logtext + "\n" + errtext).splitlines() if "ERROR" in l or "panicked" in l]
    check("no ERROR or panic line in the app's log or stderr", not bad, bad[:5])
    refusals = [l for l in logtext.splitlines() if "Insert Audio from File" in l]
    check("the refusals are logged with a clear message",
          any("44100 Hz" in l and "48000 Hz" in l and "nothing was inserted" in l for l in refusals) and any("missing.wav" in l for l in refusals), refusals[:4])
    ok = all(x[1] for x in checks)
    if not a.keep:
        shutil.rmtree(work, ignore_errors=True)
    else:
        print("kept", work)
    print("ALPHA RUN " + ("PASS" if ok else "FAIL"))
    sys.exit(0 if ok else 1)


main()
