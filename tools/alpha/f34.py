#!/usr/bin/env python3
"""f34 scripted run: pause/resume, timed, quick and dropped-sample recording, fake input only.

    python3 tools/alpha/f34.py --exe <wavcraft> [--keep]

No real input device is opened: WAVCRAFT_FAKE_INPUT replaces it (sine:440; the overflow run adds
ring=100,stall=1500 so the writer waits 1.5 s behind a 100 ms queue). Checks lengths from the files
and the STATE fields; fails on any ERROR line in the app's log.
"""
import argparse, json, os, shutil, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

PORT = 47134


def scratch():
    base = os.environ.get("ALPHA_SCRATCH") or tempfile.gettempdir()
    os.makedirs(base, exist_ok=True)
    return tempfile.mkdtemp(prefix="f34.", dir=base)


def read_take(path):
    with wave.open(path, "rb") as w:
        return {"frames": w.getnframes(), "channels": w.getnchannels(), "width": w.getsampwidth(), "rate": w.getframerate()}


def launch(exe, env):
    proc = subprocess.Popen([exe], env=env, stdout=subprocess.DEVNULL, stderr=open(env["STDERR"], "a"))
    token = env["WAVCRAFT_CONTROL_TOKEN_FILE"]
    for _ in range(120):
        if os.path.exists(token) or proc.poll() is not None:
            break
        time.sleep(0.25)
    return proc, Control(PORT, open(token).read().strip())


def wait_idle(c, secs=15):
    t0 = time.time()
    while time.time() - t0 < secs:
        st = c.state(wait=False)
        if st["recording"] == "0":
            return c.state(timeout=30)  # and the take's open in the editor has finished
        time.sleep(0.1)
    return c.state(wait=False)


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
    proc, c = launch(a.exe, env)
    try:
        # C029: pause and resume inside one take.
        r = c.ask("RECORD pause")
        check("pause with no take is refused", r.startswith("ERR"), r)
        path = c.ask("RECORD start").partition("path=")[2]
        time.sleep(1.5)
        check("pause answers OK", c.ask("RECORD pause") == "OK", None)
        time.sleep(0.3)
        st1 = c.state(wait=False)
        check("STATE says paused", st1["rec_paused"] == "1", st1)
        check("a second pause is refused", c.ask("RECORD pause").startswith("ERR"), None)
        time.sleep(1.5)
        st2 = c.state(wait=False)
        check("no frames are recorded while paused", st2["rec_frames"] == st1["rec_frames"] and st2["rec_dropped"] == "0", (st1["rec_frames"], st2["rec_frames"]))
        check("resume answers OK", c.ask("RECORD resume") == "OK", None)
        time.sleep(1.5)
        mid = c.state(wait=False)
        check("STATE logs one resume at about 1.5 s into the file", mid["rec_paused"] == "0" and mid["rec_resumes"] == "1"
              and abs(int(mid["rec_resume_at"]) - 72000) < 24000, mid)
        stop = c.ask("RECORD stop")
        t = read_take(path)
        check("the take holds about 3 s, not 4.5 s", stop.startswith("OK") and abs(t["frames"] - 3 * 48000) < 0.5 * 48000, (stop, t))
        wait_idle(c)

        # C028: a maximum duration; the take stops by itself and is kept.
        r = c.ask("RECORD start max=2")
        path = r.partition("path=")[2]
        check("RECORD start max=2 answers OK", r.startswith("OK path="), r)
        st = c.state(wait=False)
        check("STATE shows the limit", st["rec_max_s"] == "2", st)
        time.sleep(3.5)
        st = wait_idle(c)
        t = read_take(path)
        check("it stopped by itself with about 2 s kept", st["recording"] == "0" and abs(t["frames"] - 2 * 48000) < 0.35 * 48000, (st["recording"], t))
        # A start time: armed, then it starts and stops by itself.
        before = set(os.listdir(takes))
        r = c.ask("RECORD start in=2 max=1")
        check("RECORD start in=2 answers armed", r.startswith("OK armed"), r)
        st = c.state(wait=False)
        check("STATE shows armed, no take yet", st["recording"] == "0" and st["rec_armed"] != "-1" and set(os.listdir(takes)) == before, st)
        time.sleep(1.2)
        check("still nothing recorded before the start time", set(os.listdir(takes)) == before, os.listdir(takes))
        time.sleep(3.3)
        new = sorted(set(os.listdir(takes)) - before)
        t = read_take(os.path.join(takes, new[0])) if new else {}
        check("the timed take ran and stopped, about 1 s", len(new) == 1 and abs(t["frames"] - 48000) < 0.35 * 48000, (new, t))
        r = c.ask("RECORD start in=30")
        check("a cancelled start time records nothing", c.ask("RECORD cancel") == "OK" and c.state(wait=False)["rec_armed"] == "-1" and r.startswith("OK armed"), r)
        check("cancel with nothing armed is refused", c.ask("RECORD cancel").startswith("ERR"), None)

        # fix150: a timed start whose time comes while an open or save is running must not be dropped.
        before = set(os.listdir(takes))
        c.ask("RECORD start in=1 max=1")
        check("a manual start is refused while busy (hook)", c.ask("HOLD_BUSY 5000") == "OK" and c.ask("RECORD start max=1").startswith("ERR"), None)
        check("the busy hold shows in STATE", c.state(wait=False)["busy"] == "1", None)
        time.sleep(1.6)
        st = c.state(wait=False)
        check("the timed take started at its time although busy", st["recording"] == "1" and st["rec_start_busy"] == "1", st)
        check("the delay is reported (small)", 0 <= int(st["rec_late_ms"]) < 1500, st)
        time.sleep(2.0)
        new = sorted(set(os.listdir(takes)) - before)
        t = read_take(os.path.join(takes, new[0])) if new else {}
        check("that take ran and kept about 1 s", len(new) == 1 and abs(t["frames"] - 48000) < 0.35 * 48000, (new, t))
        time.sleep(2.0)
        wait_idle(c)

        # C031: record at once with the last settings, no dialog.
        r = c.ask("RECORD start rate=44100 ch=2 bits=16 max=1")
        time.sleep(2.0)
        wait_idle(c)
        before = set(os.listdir(takes))
        r = c.ask("RECORD quick")
        path = r.partition("path=")[2]
        check("RECORD quick starts at once", r.startswith("OK path=") and c.state(wait=False)["recording"] == "1", r)
        st = c.state(wait=False)
        check("it uses the last settings (44.1 kHz stereo 16-bit)", (st["rec_rate"], st["rec_ch"], st["rec_bits"]) == ("44100", "2", "16"), st)
        time.sleep(1.0)
        c.ask("RECORD stop")
        t = read_take(path)
        check("the quick take is 44.1 kHz stereo 16-bit", (t["rate"], t["channels"], t["width"]) == (44100, 2, 2) and t["frames"] > 20000, t)
        c.ask("QUIT discard")
        proc.wait(timeout=20)
    finally:
        if proc.poll() is None:
            proc.kill()

    # A fresh app has no last settings: quick uses the default (48 kHz mono 24-bit).
    proc, c = launch(a.exe, env)
    try:
        path = c.ask("RECORD quick").partition("path=")[2]
        time.sleep(0.8)
        c.ask("RECORD stop")
        t = read_take(path)
        check("a fresh app's quick take is 48 kHz mono 24-bit", (t["rate"], t["channels"], t["width"]) == (48000, 1, 3), t)
        c.ask("QUIT discard")
        proc.wait(timeout=20)
    finally:
        if proc.poll() is None:
            proc.kill()

    # C035: overflow on purpose (100 ms queue, writer waits 1.5 s).
    env2 = dict(env, WAVCRAFT_FAKE_INPUT="sine:440,ring=100,stall=1500")
    proc, c = launch(a.exe, env2)
    try:
        path = c.ask("RECORD start").partition("path=")[2]
        time.sleep(3.0)
        st = c.state(wait=False)
        dropped = int(st["rec_dropped"])
        check("STATE counts dropped frames", dropped > 30000, st)
        stop = c.ask("RECORD stop")
        t = read_take(path)
        got = int(stop.partition("dropped=")[2].split()[0]) if "dropped=" in stop else -1
        check("the stop answer reports the dropped count", stop.startswith("OK") and got >= dropped, stop)
        check("the take is readable and holds what was kept", t["frames"] > 0 and abs(t["frames"] + got - 3 * 48000) < 0.5 * 48000, (t, got))
        c.ask("QUIT discard")
        proc.wait(timeout=20)
    finally:
        if proc.poll() is None:
            proc.kill()

    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f.startswith("wavcraft") and f.endswith(".log")]
    lines = [l.strip() for p in log_files for l in open(p, errors="replace")]
    errors = [l[:200] for l in lines if " ERROR " in l]
    check("the app's log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    warn = [l[:200] for l in lines if "frames (about" in l and "lost so far" in l]
    check("the dropped frames were logged as a warning", bool(warn), warn[:1])
    check("the pause and resume were logged", any("recording resumed at frame" in l for l in lines), None)
    result = {"feature": "f34 recording 2", "status": "pass" if all(x["ok"] for x in checks) else "fail", "seconds": round(time.time() - t0, 1), "checks": checks}
    if a.keep or result["status"] != "pass":
        result["work"] = work
    else:
        shutil.rmtree(work, ignore_errors=True)
    print(json.dumps(result, indent=1))
    return 0 if result["status"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
