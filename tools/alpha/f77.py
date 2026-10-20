#!/usr/bin/env python3
"""f77 scripted run: several inputs of one device into one multichannel take, fake input only.

    python3 tools/alpha/f77.py --exe <wavcraft> [--keep]

No real input device is opened: WAVCRAFT_FAKE_INPUT=sine:400+800+1200+2400 replaces it with a
four-input device whose input k plays tone k. Checks each recorded channel's tone and the file's
channel count, a chosen subset, a refused input, and that a SIGKILL mid-take leaves a readable
4-channel file. Fails on any ERROR line in the app's log.
"""
import argparse, json, os, shutil, signal, struct, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

PORT = 47177
TONES = [400, 800, 1200, 2400]


def scratch():
    base = os.environ.get("ALPHA_SCRATCH") or tempfile.gettempdir()
    os.makedirs(base, exist_ok=True)
    return tempfile.mkdtemp(prefix="f77.", dir=base)


def read_take(path):
    """Channels, rate, frames and each channel's pitch (zero crossings), read with the wave module."""
    with wave.open(path, "rb") as w:
        n, ch, rate, width = w.getnframes(), w.getnchannels(), w.getframerate(), w.getsampwidth()
        raw = w.readframes(n)
    assert width == 2, "the script records 16-bit"
    samples = struct.unpack("<%dh" % (len(raw) // 2), raw)
    hz = []
    for k in range(ch):
        s = samples[k::ch]
        cross = sum(1 for a, b in zip(s, s[1:]) if a <= 0 < b)
        hz.append(cross * rate / max(len(s), 1))
    return {"channels": ch, "rate": rate, "frames": n, "hz": hz}


def tones_ok(got, want):
    return len(got) == len(want) and all(abs(g - w) < w * 0.03 + 3 for g, w in zip(got, want))


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
               WAVCRAFT_FAKE_INPUT="sine:" + "+".join(map(str, TONES)), WAVCRAFT_RECORD_DIR=takes,
               STDERR=os.path.join(work, "stderr.log"), RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    t0 = time.time()
    proc, c = launch(a.exe, env)
    try:
        # All four inputs into one 4-channel take.
        r = c.ask("RECORD start ch=4 inputs=1,2,3,4 bits=16")
        path = r.partition("path=")[2]
        check("RECORD start ch=4 inputs=1,2,3,4 answers OK", r.startswith("OK path="), r)
        st = c.state(wait=False)
        check("STATE shows 4 channels from inputs 1,2,3,4", (st["rec_ch"], st["rec_inputs"]) == ("4", "1,2,3,4"), st)
        time.sleep(2.0)
        stop = c.ask("RECORD stop")
        t = read_take(path)
        check("the stop answer is OK with no drops", stop.startswith("OK") and "dropped=0" in stop, stop)
        check("the file has 4 channels and about 2 s", t["channels"] == 4 and abs(t["frames"] - 2 * 48000) < 0.5 * 48000, t)
        check("each channel holds its own tone", tones_ok(t["hz"], TONES), t["hz"])
        wait_idle(c)

        # A chosen subset: inputs 2 and 4 only.
        r = c.ask("RECORD start inputs=4,2 bits=16")
        path = r.partition("path=")[2]
        time.sleep(1.2)
        st = c.state(wait=False)
        check("STATE shows the subset 2,4", (st["rec_ch"], st["rec_inputs"]) == ("2", "2,4"), st)
        c.ask("RECORD stop")
        t = read_take(path)
        check("the subset take has 2 channels carrying inputs 2 and 4", t["channels"] == 2 and tones_ok(t["hz"], [800, 2400]), t)
        wait_idle(c)

        # An input the device does not have, and a mismatch, are refused and record nothing.
        before = set(os.listdir(takes))
        r1 = c.ask("RECORD start inputs=5")
        r2 = c.ask("RECORD start ch=3 inputs=1,2")
        check("an input the device lacks is refused", r1.startswith("ERR"), r1)
        check("ch not matching inputs is refused", r2.startswith("ERR"), r2)
        check("a refused start leaves no new file and no recording", set(os.listdir(takes)) == before and c.state(wait=False)["recording"] == "0", sorted(set(os.listdir(takes)) - before))
        c.ask("QUIT discard")
        proc.wait(timeout=20)
    finally:
        if proc.poll() is None:
            proc.kill()

    # SIGKILL mid-take: a readable 4-channel file is left.
    proc, c = launch(a.exe, env)
    killed = None
    try:
        before = set(os.listdir(takes))
        r = c.ask("RECORD start ch=4 inputs=1,2,3,4 bits=16")
        killed = r.partition("path=")[2]
        time.sleep(3.0)  # past two header updates
        proc.send_signal(signal.SIGKILL)
        proc.wait(timeout=20)
    finally:
        if proc.poll() is None:
            proc.kill()
    try:
        t = read_take(killed)
        check("after SIGKILL the file reads as 4 channels", t["channels"] == 4, t)
        check("it holds at least 1.5 s", t["frames"] >= 1.5 * 48000, t["frames"])
        check("every channel still has its tone", tones_ok(t["hz"], TONES), t["hz"])
    except Exception as e:  # noqa: BLE001
        check("after SIGKILL the file is a readable 4-channel WAV", False, repr(e))

    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f.startswith("wavcraft") and f.endswith(".log")]
    lines = [l.strip() for p in log_files for l in open(p, errors="replace")]
    errors = [l[:200] for l in lines if " ERROR " in l]
    check("the app's log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    result = {"feature": "f77 multi-input recording", "status": "pass" if all(x["ok"] for x in checks) else "fail", "seconds": round(time.time() - t0, 1), "checks": checks}
    if a.keep or result["status"] != "pass":
        result["work"] = work
    else:
        shutil.rmtree(work, ignore_errors=True)
    print(json.dumps(result, indent=1))
    return 0 if result["status"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
