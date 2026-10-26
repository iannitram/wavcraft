#!/usr/bin/env python3
"""f126 scripted run: punch in/out with a range, fake input (WAVCRAFT_FAKE_INPUT=1), silent output (WAVCRAFT_NO_AUDIO=1), port 47226.

    python3 tools/alpha/f126.py --exe <path to the release wavcraft> [--keep]

Records a full take (take 1) with punch off, then sets a punch range 3.0..4.0 s, switches punch on and
records again: the recording starts at the pre-roll (1.0 s before the range), runs past the range end,
and the new take must cover only 3.0..4.0 s, on a new playlist, with the old take's audio kept on
the first. HOME, the log and the session folder are in a scratch folder; fails on any ERROR line in
the app's own log. No real input device is opened.
"""
import argparse, json, math, os, shutil, signal, socket, struct, subprocess, sys, tempfile, time, wave

PORT, RATE = 47226, 48000


def make_wav(path, rate, channels, secs, hz):
    with wave.open(path, "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(rate)
        n = int(rate * secs)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * hz * i / rate))) * channels for i in range(n)))


class Control:
    def __init__(self, port, token):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("the control channel refused the token")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self):
        a = self.ask("SESSION state")
        head, _, js = a.partition(" {")
        return json.loads("{" + js) if js else {}





def read_take(path):
    """(channels, rate, frames claimed by the header and present in the file, first samples)."""
    d = open(path, "rb").read()
    if d[0:4] != b"RIFF" or d[8:16] != b"WAVEfmt " or d[36:40] != b"data":
        return None
    fmt, ch, rate = struct.unpack("<HHI", d[20:28])
    size = struct.unpack("<I", d[40:44])[0]
    body = d[44:44 + size]
    frames = len(body) // (4 * ch)
    n = min(frames, 200)
    first = struct.unpack("<%df" % (n * ch), body[: n * ch * 4])
    return fmt, ch, rate, frames, first


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f126.", dir=root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    back = os.path.join(work, "back.wav")
    make_wav(back, RATE, 2, 6.0, 220)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_NO_AUDIO="1", WAVCRAFT_FAKE_INPUT="1", WAVCRAFT_CONTROL_PORT=str(PORT),
               WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([args.exe], env=env, stdout=err, stderr=err)
    checks = []

    def check(label, ok, detail=""):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    try:
        c = Control(PORT, open(token).read().strip() if os.path.exists(token) else "")
    except Exception:
        time.sleep(1)
        c = Control(PORT, open(token).read().strip())
    try:
        check("SESSION new", c.ask("SESSION new Punch rate=48000 depth=24") == "OK")
        c.ask("TRACK add Backing stereo")
        c.ask("TRACK add Vox mono")
        check("CLIP add backing", c.ask(f'CLIP add Backing "{back}" 0') == "OK clips=1")
        check("TRACK arm Vox", c.ask("TRACK arm Vox input=1") == "OK")
        check("PUNCH on without a range is refused", c.ask("PUNCH on").startswith("ERR no punch range"))
        check("PUNCH set with in >= out is refused", c.ask("PUNCH set 4 3").startswith("ERR"))
        # Take 1: punch off, record 2.0 .. about 5.0 s (it covers the range 3.0..4.0).
        check("SREC start 2.0 (take 1, punch off)", c.ask("SREC start 2.0") == "OK")
        time.sleep(3.0)
        check("SREC stop (take 1)", c.ask("SREC stop") == "OK takes=1")
        s = c.state()
        t1 = s["takes"][0]
        check("take 1 is whole: starts 2.0, about 3 s", abs(t1["start"] - 2.0) < 1e-6 and 2.5 < t1["length"] < 4.0, t1)
        len1 = t1["length"]
        # Take 2: punch 3.0 .. 4.0.
        check("PUNCH set 3.0 4.0", c.ask("PUNCH set 3.0 4.0") == "OK")
        check("PUNCH on", c.ask("PUNCH on") == "OK")
        s = c.state()
        check("state shows the punch", s["punch"]["on"] is True and s["punch"]["in"] == 3.0 and s["punch"]["out"] == 4.0 and s["punch"]["preroll"] == 2.0, s.get("punch"))
        check("PUNCH preroll 1", c.ask("PUNCH preroll 1") == "OK")
        check("SREC start (from the pre-roll)", c.ask("SREC start") == "OK")
        time.sleep(0.5)
        s = c.state()
        check("recording starts at the pre-roll (2.0 s) and the backing plays", s.get("recording") is True and s.get("playing") is True and s["position"] >= 2.0, (s.get("recording"), s.get("playing"), s.get("position")))
        time.sleep(3.0)  # record past the range end: 1 s lead-in + 1 s range + about 1.5 s after
        check("SREC stop (take 2)", c.ask("SREC stop") == "OK takes=1")
        s = c.state()
        takes = s["takes"]
        check("two takes listed", len(takes) == 2, takes)
        t2 = takes[1]
        check("take 2 covers only the range: start 3.0, length 1.0", abs(t2["start"] - 3.0) < 1e-6 and abs(t2["length"] - 1.0) < 1e-6, t2)
        check("take 2 is on a new playlist, on top", t2["playlist"] == 1 and t2["on_top"] is True and takes[0]["on_top"] is False, takes)
        vox = [t for t in s["tracks"] if t["name"] == "Vox"][0]
        spans = [(round(k["start"], 3), round(k["length"], 3)) for k in vox["clips"]]
        want = [(2.0, 1.0), (3.0, 1.0), (4.0, round(len1 - 2.0, 3))]
        check("the track plays old take, punch, old take: only the range was replaced", spans == want, (spans, want))
        old = [t for t in s["tracks"] if t["name"] == "Vox"][0]
        check("the old take is untouched underneath (playlist 0 holds its whole length)", takes[0]["length"] == len1 and takes[0]["on_top"] is False, takes[0])
        f2 = os.path.join(sessions, "takes", t2["file"])
        r = read_take(f2)
        check("the file keeps everything recorded (more than the range)", r and r[3] > 2.5 * RATE, r and r[3])
        # Switch punch off again.
        check("PUNCH off", c.ask("PUNCH off") == "OK")
        check("state shows punch off", c.state()["punch"]["on"] is False)
        check("QUIT", c.ask("QUIT").startswith("OK"))
        try:
            proc.wait(timeout=15)
        except subprocess.TimeoutExpired:
            pass
    finally:
        if proc.poll() is None:
            proc.kill()
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    ok = all(x["ok"] for x in checks)
    for x in checks:
        print(("PASS " if x["ok"] else "FAIL ") + x["check"] + ("" if x["ok"] else f"  [{x['detail']}]"))
    print("RESULT", "pass" if ok else "fail", f"({sum(x['ok'] for x in checks)}/{len(checks)})")
    if args.keep or not ok:
        print("work folder kept:", work)
    else:
        shutil.rmtree(work, ignore_errors=True)
    return 0 if ok else 1


sys.exit(main())
