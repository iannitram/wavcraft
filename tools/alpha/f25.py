#!/usr/bin/env python3
"""f25 scripted run: recording a take through the control channel, with a fake input only.

    python3 tools/alpha/f25.py --exe <wavcraft> [--keep]

No real input device is opened: WAVCRAFT_FAKE_INPUT=sine:440 replaces it. Run 1 records about
3 s (default format: 48 kHz, mono, 24-bit), stops, and checks STATE and the file's length,
format and frequency. Run 2 records and is killed with SIGKILL mid-take: the take on disk must be
readable and hold nearly what was recorded; a third run opens it. Every run's log must hold no ERROR line.
"""
import argparse, json, os, shutil, signal, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control, free_port  # noqa: E402

PORT = 47125
FREQ = 440.0


def scratch():
    base = os.environ.get("ALPHA_SCRATCH") or tempfile.gettempdir()
    os.makedirs(base, exist_ok=True)
    return tempfile.mkdtemp(prefix="f25.", dir=base)


def read_take(path):
    with wave.open(path, "rb") as w:
        n, ch, width, rate = w.getnframes(), w.getnchannels(), w.getsampwidth(), w.getframerate()
        raw = w.readframes(n)
    step = ch * width
    samples = [int.from_bytes(raw[i:i + width], "little", signed=True) for i in range(0, len(raw) - step + 1, step)]
    crossings = sum(1 for a, b in zip(samples, samples[1:]) if a <= 0 < b)
    hz = crossings / (n / rate) if n else 0
    return {"frames": n, "channels": ch, "width": width, "rate": rate, "hz": hz, "peak": max((abs(s) for s in samples), default=0)}


def launch(exe, env, args=()):
    proc = subprocess.Popen([exe, *args], env=env, stdout=subprocess.DEVNULL, stderr=open(env["STDERR"], "a"))
    token = env["WAVCRAFT_CONTROL_TOKEN_FILE"]
    for _ in range(120):
        if os.path.exists(token) or proc.poll() is not None:
            break
        time.sleep(0.25)
    return proc, Control(PORT, open(token).read().strip())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    work = scratch()
    home, logs, sessions, takes = (os.path.join(work, d) for d in ("home", "logs", "sessions", "takes"))
    for d in (home, sessions):
        os.makedirs(d)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=os.path.join(work, "control.token"),
               WAVCRAFT_FAKE_INPUT=f"sine:{int(FREQ)}", WAVCRAFT_RECORD_DIR=takes, STDERR=os.path.join(work, "stderr.log"), RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    t0 = time.time()
    # Run 1: record, stop, check the file and what the editor opened.
    proc, c = launch(a.exe, env)
    try:
        st = c.state()
        check("STATE before recording says recording=0", st.get("recording") == "0", st)
        r = c.ask("RECORD start")
        path = r.partition("path=")[2]
        check("RECORD start answers OK with the new file's path", r.startswith("OK path=") and os.path.exists(path), r)
        again = c.ask("RECORD start")
        check("a second RECORD start is refused", again.startswith("ERR"), again)
        time.sleep(1.2)
        mid = c.state()
        check("STATE reports the take growing, no drops", mid["recording"] == "1" and int(mid["rec_frames"]) > 20000 and mid["rec_dropped"] == "0"
              and int(mid["rec_bytes"]) > 44 and int(mid["rec_left_s"]) != 0, mid)
        header_mid = os.path.getsize(path)
        check("the file is on disk and growing while recording", header_mid > 44, header_mid)
        time.sleep(1.9)
        stop = c.ask("RECORD stop")
        check("RECORD stop answers OK", stop.startswith("OK frames="), stop)
        st = c.state(timeout=30)
        check("the take is open in the editor, mono 48 kHz", st["channels"] == "1" and st["rate"] == "48000" and abs(int(st["frames"]) - 3 * 48000) < 0.5 * 48000, st)
        t = read_take(path)
        check("file: 24-bit mono 48 kHz, about 3 s", (t["channels"], t["width"], t["rate"]) == (1, 3, 48000) and abs(t["frames"] - 3 * 48000) < 0.5 * 48000, t)
        check("the editor's length equals the file's", int(st["frames"]) == t["frames"], f"{st['frames']} vs {t['frames']}")
        check("file: the sine is 440 Hz at about half scale", abs(t["hz"] - FREQ) < 3 and 0.45 < t["peak"] / 8388607 < 0.55, t)
        check("STATE after stop says recording=0", st["recording"] == "0", st.get("recording"))
        q = c.ask("QUIT discard")
        check("QUIT", q == "OK", q)
        proc.wait(timeout=20)
    finally:
        if proc.poll() is None:
            proc.kill()

    # Run 2: stereo 16-bit through the dialog's arguments, killed mid-take.
    proc, c = launch(a.exe, env)
    try:
        r = c.ask("RECORD start rate=44100 ch=2 bits=16")
        killed = r.partition("path=")[2]
        check("RECORD start with rate, channels and bits", r.startswith("OK path="), r)
        time.sleep(3.5)
        st = c.state(wait=False)
        recorded = int(st["rec_frames"])
        proc.send_signal(signal.SIGKILL)
        proc.wait(timeout=10)
        check("the app was killed mid-take", st["recording"] == "1", st)
    finally:
        if proc.poll() is None:
            proc.kill()
    t = read_take(killed)
    lost = (recorded - t["frames"]) / 44100
    check("the killed take reads: 16-bit stereo 44.1 kHz", (t["channels"], t["width"], t["rate"]) == (2, 2, 44100), t)
    check("the killed take holds all but at most ~1.5 s of what was recorded", t["frames"] > 0 and -0.05 <= lost <= 1.5, f"recorded {recorded}, on disk {t['frames']}, lost {lost:.2f} s")
    check("the killed take is still the sine", abs(t["hz"] - FREQ) < 3, t)

    # Run 3: the killed take opens in the editor.
    env2 = dict(env, WAVCRAFT_FAKE_INPUT="")
    proc, c = launch(a.exe, env2, [killed])
    try:
        st = c.state(timeout=30)
        check("the app opens the killed take with its length", int(st["frames"]) == t["frames"] and st["channels"] == "2", st)
        c.ask("QUIT discard")
        proc.wait(timeout=20)
    finally:
        if proc.poll() is None:
            proc.kill()

    takes_left = sorted(os.listdir(takes))
    check("both takes are still in the recordings folder", len(takes_left) == 2, takes_left)
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    result = {"feature": "f25 recording", "status": "pass" if all(c["ok"] for c in checks) else "fail", "seconds": round(time.time() - t0, 1), "checks": checks}
    if a.keep or result["status"] != "pass":
        result["work"] = work
    else:
        shutil.rmtree(work, ignore_errors=True)
    print(json.dumps(result, indent=1))
    return 0 if result["status"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
