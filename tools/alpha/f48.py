#!/usr/bin/env python3
"""f48 scripted run: edit while playing, and following the default output device.

    python3 tools/alpha/f48.py --exe <path to wavcraft> [--keep]

Headless (WAVCRAFT_NO_AUDIO=1: a silent output thread runs the real audio callback in real time).
Checks, through the control channel: PLAY moves the position; a Delete before the playhead keeps
playing and moves the position back by the deleted length (plus the time that passed); an
Undo moves it forward again; DEVICE_CHANGED reopens the output and playback continues from the
same place; the app's log holds no ERROR line.
"""
import argparse, array, json, math, os, shutil, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import golden  # noqa: E402

RATE, SECONDS = 48000, 60


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2), w.setsampwidth(2), w.setframerate(RATE)
        for s in range(SECONDS):
            a = array.array("h")
            for i in range(RATE):
                v = int(8000 * math.sin(2 * math.pi * 220 * (s * RATE + i) / RATE))
                a.extend((v, -v))
            w.writeframes(a.tobytes())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f48.", dir=root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    make_wav(wav)
    port, token_file = int(os.environ.get("WAVCRAFT_CONTROL_PORT", "47148")), os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token_file, WAVCRAFT_NO_AUDIO="1", RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([args.exe, wav], env=env, stdout=err, stderr=err)
    try:
        # The token file is made by the app on start.
        for _ in range(100):
            if os.path.exists(token_file) and os.path.getsize(token_file) >= 64:
                break
            time.sleep(0.1)
        token = open(token_file).read().strip()
        c = golden.Control(port, token)
        st = c.state()
        frames = int(st["frames"])
        check("the file opens", frames == SECONDS * RATE, st)

        a = c.ask("PLAY 100000 %d" % frames)
        time.sleep(0.6)
        st = c.state(wait=False)
        p1 = int(st["position"])
        check("PLAY starts and the position advances", a.endswith("playing=1") and st["playing"] == "1" and p1 > 100000 + RATE // 4, f"{a}; {st}")

        # Delete one second before the playhead while it plays.
        c.ask(f"SELECT 0 {RATE}")
        t0 = time.time()
        before = int(c.state(wait=False)["position"])
        c.ask("ACTION edit.delete")
        st = c.state()  # waits until the rebuild is done
        elapsed = time.time() - t0
        after = int(st["position"])
        expect = before - RATE + RATE * elapsed
        check("Delete before the playhead: still playing", st["playing"] == "1" and int(st["frames"]) == frames - RATE, st)
        check("the position moved back by the deleted length (plus time played)",
              abs(after - expect) < RATE * 0.6, f"before {before}, after {after}, expected about {int(expect)} (elapsed {elapsed:.2f} s)")
        time.sleep(0.4)
        later = int(c.state(wait=False)["position"])
        check("and keeps advancing", later > after, f"{after} -> {later}")

        # Undo brings the second back: the position moves forward by it.
        before = int(c.state(wait=False)["position"])
        t0 = time.time()
        c.ask("ACTION edit.undo")
        st = c.state()
        elapsed = time.time() - t0
        after = int(st["position"])
        expect = before + RATE + RATE * elapsed
        check("Undo moves the position forward by the restored length",
              st["playing"] == "1" and abs(after - expect) < RATE * 0.6, f"before {before}, after {after}, expected about {int(expect)}")

        # A change of the default output device.
        pre = c.state(wait=False)
        c.ask("DEVICE_CHANGED")
        deadline = time.time() + 5
        while time.time() < deadline:
            st = c.state(wait=False)
            if st["reopens"] != pre["reopens"]:
                break
            time.sleep(0.1)
        check("DEVICE_CHANGED reopens the output on the new default",
              int(st["reopens"]) == int(pre["reopens"]) + 1 and st["device"] != pre["device"], f"{pre['device']} -> {st['device']}, reopens {st['reopens']}")
        check("playback continues from the same position after the reopen",
              st["playing"] == "1" and int(st["position"]) >= int(pre["position"]), f"{pre['position']} -> {st['position']}")
        time.sleep(0.4)
        st2 = c.state(wait=False)
        check("and keeps advancing on the new device", st2["playing"] == "1" and int(st2["position"]) > int(st["position"]), f"{st['position']} -> {st2['position']}")

        c.ask("QUIT discard")
        try:
            proc.wait(15)
        except subprocess.TimeoutExpired:
            proc.kill()
            check("the app quits", False, "timeout")
    except Exception as e:  # noqa: BLE001
        check("the run completes", False, f"{type(e).__name__}: {e}")
        proc.kill()
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    status = "pass" if all(x["ok"] for x in checks) else "fail"
    print(json.dumps({"feature": "f48", "status": status, "checks": checks, "log": log_files[:1]}, indent=1))
    if args.keep or status == "fail":
        print("kept:", work)
    else:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if status == "pass" else 1)


main()
