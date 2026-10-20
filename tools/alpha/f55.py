#!/usr/bin/env python3
"""f55 scripted run: an interrupted take is offered, repaired and opened; nothing is lost.

    python3 tools/alpha/f55.py --exe <wavcraft> [--keep]

Fake input only (WAVCRAFT_FAKE_INPUT=sine:440); no real device is opened. Records, SIGKILLs the app
mid-take, restarts with the same scratch HOME, RECOVERTAKE, checks the length and the tone, and a
further restart finds nothing left. Fails on any ERROR line in the app's log.
"""
import argparse, json, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

PORT = 47155


def scratch():
    base = os.environ.get("ALPHA_SCRATCH") or tempfile.gettempdir()
    os.makedirs(base, exist_ok=True)
    return tempfile.mkdtemp(prefix="f55.", dir=base)


def samples24(path):
    with wave.open(path, "rb") as w:
        n, rate, width = w.getnframes(), w.getframerate(), w.getsampwidth()
        raw = w.readframes(n)
    vals = [int.from_bytes(raw[i:i + 3], "little", signed=True) / 8388608.0 for i in range(0, len(raw) - 2, 3)]
    return vals, rate, width


def launch(exe, env):
    proc = subprocess.Popen([exe], env=env, stdout=subprocess.DEVNULL, stderr=open(env["STDERR"], "a"))
    token = env["WAVCRAFT_CONTROL_TOKEN_FILE"]
    for _ in range(120):
        if os.path.exists(token) or proc.poll() is not None:
            break
        time.sleep(0.25)
    return proc, Control(PORT, open(token).read().strip())


def scanned(c, secs=20):
    t0 = time.time()
    while time.time() - t0 < secs:
        r = c.ask("AUTOSAVE")
        f = dict(kv.split("=", 1) for kv in r.split()[1:])
        if f.get("scanned") == "1":
            return f
        time.sleep(0.2)
    return f


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
               WAVCRAFT_FAKE_INPUT="sine:440", WAVCRAFT_RECORD_DIR=takes, STDERR=os.path.join(work, "stderr.log"), RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    t0 = time.time()
    # 1. Record, then SIGKILL mid-take.
    proc, c = launch(a.exe, env)
    path = ""
    recorded = 0
    try:
        scanned(c)
        r = c.ask("RECORD start")
        path = r.partition("path=")[2]
        time.sleep(3.4)
        st = c.state(wait=False)
        recorded = int(st["rec_frames"])
        check("recording ran before the kill", st["recording"] == "1" and recorded > 100000, st)
        check("the marker exists while recording", os.path.exists(path + ".recording"), path + ".recording")
        proc.kill()  # SIGKILL: no clean stop
        proc.wait(timeout=20)
    finally:
        if proc.poll() is None:
            proc.kill()
    check("the kill left the take and its marker", os.path.exists(path) and os.path.exists(path + ".recording"), None)
    size_after_kill = os.path.getsize(path)

    # 2. Restart with the same scratch HOME: the take is offered; recover it.
    proc, c = launch(a.exe, env)
    try:
        f = scanned(c)
        check("the scan finds one unfinished take", f.get("takes") == "1", f)
        check("the scan deleted nothing", os.path.exists(path) and os.path.getsize(path) == size_after_kill, None)
        r = c.ask("RECOVERTAKE")
        check("RECOVERTAKE answers OK", r == "OK", r)
        st = c.state(timeout=60)
        frames = int(st["frames"])
        check("the opened take's length is close to what was recorded", st["channels"] == "1" and st["rate"] == "48000" and recorded <= frames <= recorded + 48000,
              {"recorded_at_state": recorded, "opened": frames})
        check("the file keeps every byte after the header", os.path.getsize(path) == size_after_kill, None)
        check("the marker is gone", not os.path.exists(path + ".recording"), None)
        vals, rate, width = samples24(path)
        check("the file's header now holds all the frames", len(vals) == frames and width == 3, len(vals))
        mid = vals[48000:96000]
        peak = max(abs(v) for v in mid)
        cross = sum(1 for i in range(1, len(mid)) if (mid[i - 1] < 0) != (mid[i] < 0))
        hz = cross / 2 * rate / len(mid)
        check("the audio is the fake tone (440 Hz, peak 0.5)", abs(hz - 440) < 3 and abs(peak - 0.5) < 0.01, {"hz": round(hz, 1), "peak": round(peak, 3)})
        check("a second RECOVERTAKE has nothing to recover", c.ask("RECOVERTAKE").startswith("ERR"), None)
        c.ask("QUIT discard")
        proc.wait(timeout=20)
    finally:
        if proc.poll() is None:
            proc.kill()

    # 3. Another restart: nothing left.
    proc, c = launch(a.exe, env)
    try:
        f = scanned(c)
        check("a further start shows nothing to recover", f.get("takes") == "0" and os.path.exists(path), f)
        c.ask("QUIT discard")
        proc.wait(timeout=20)
    finally:
        if proc.poll() is None:
            proc.kill()

    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f.startswith("wavcraft") and f.endswith(".log")]
    lines = [l.strip() for p in log_files for l in open(p, errors="replace")]
    errors = [l[:200] for l in lines if " ERROR " in l]
    check("the app's log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    result = {"feature": "f55 take recovery", "status": "pass" if all(x["ok"] for x in checks) else "fail", "seconds": round(time.time() - t0, 1), "checks": checks}
    if a.keep or result["status"] != "pass":
        result["work"] = work
    else:
        shutil.rmtree(work, ignore_errors=True)
    print(json.dumps(result, indent=1))
    return 0 if result["status"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
