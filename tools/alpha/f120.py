#!/usr/bin/env python3
"""f120 scripted run: multitrack recording, fake input (WAVCRAFT_FAKE_INPUT=1), silent output (WAVCRAFT_NO_AUDIO=1), port 47220.

    python3 tools/alpha/f120.py --exe <path to the release wavcraft> [--keep]

A synthetic backing track plays while the armed track records (overdub). Two takes over the same
range are checked for placement (at the record start) and numbering/stacking (newest on top), and
the take file's contents are checked against the fake input's formula for the chosen channel.
A third take is cut short by SIGKILL; its file must still have a readable header and the audio up
to the last header patch. HOME, the log and the session folder are in a scratch folder. Fails on
any ERROR line in the app's own log; no real input device is opened.
"""
import argparse, json, math, os, shutil, signal, socket, struct, subprocess, sys, tempfile, time, wave

PORT, RATE = 47220, 48000


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
    work = tempfile.mkdtemp(prefix="f120.", dir=root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    back = os.path.join(work, "back.wav")
    make_wav(back, RATE, 2, 4.0, 220)
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
        check("SESSION new", c.ask("SESSION new Rec rate=48000 depth=24") == "OK")
        c.ask("TRACK add Backing stereo")
        c.ask("TRACK add Vox mono")
        check("CLIP add backing", c.ask(f'CLIP add Backing "{back}" 0') == "OK clips=1")
        check("SREC start with nothing armed is refused", c.ask("SREC start 1").startswith("ERR no track is armed"))
        check("arm with a bad input is refused", c.ask("TRACK arm Vox input=0").startswith("ERR"))
        check("TRACK arm Vox input=3", c.ask("TRACK arm Vox input=3") == "OK")
        s = c.state()
        vox = [t for t in s["tracks"] if t["name"] == "Vox"][0]
        check("state shows the arm and the input", vox["armed"] is True and vox["input"] == "In 3", vox)
        for n in (1, 2):
            check(f"SREC start 1.0 (take {n})", c.ask("SREC start 1.0") == "OK")
            time.sleep(0.5)
            s = c.state()
            check("recording and the backing plays (overdub)", s.get("recording") is True and s.get("playing") is True and s["position"] > 1.2,
                  (s.get("recording"), s.get("playing"), s.get("position")))
            check("a second SREC start is refused", c.ask("SREC start 1.0").startswith("ERR already recording"))
            time.sleep(0.8)
            check(f"SREC stop (take {n})", c.ask("SREC stop") == "OK takes=1")
        s = c.state()
        takes = s["takes"]
        check("two takes listed", len(takes) == 2, takes)
        check("numbered 1 and 2", [t["take"] for t in takes] == [1, 2], takes)
        check("placed at the record start (1.0 s)", all(abs(t["start"] - 1.0) < 1e-6 for t in takes), takes)
        check("takes are about 1.3 s long", all(0.9 < t["length"] < 1.9 for t in takes), [t["length"] for t in takes])
        check("stacked: separate playlists, newest on top", [t["playlist"] for t in takes] == [0, 1] and [t["on_top"] for t in takes] == [False, True], takes)
        vox = [t for t in s["tracks"] if t["name"] == "Vox"][0]
        check("the track plays just the newest take", len(vox["clips"]) == 1 and vox["clips"][0]["name"] == "Vox take 02", vox["clips"])
        check("the backing is untouched", [t for t in s["tracks"] if t["name"] == "Backing"][0]["clips"][0]["length"] == 4.0)
        f1 = os.path.join(sessions, "takes", takes[0]["file"])
        r = read_take(f1)
        check("take file 1 is a float WAV, mono, 48 kHz", r and r[:3] == (3, 1, RATE), r and r[:3])
        want = [0.25 * math.sin(2 * math.pi * 660 * i / RATE) for i in range(200)]
        check("take 1 holds input channel 3 (660 Hz, the fake input)", r and max(abs(a - b) for a, b in zip(r[4], want)) < 1e-4)
        # A third take, killed mid-write.
        check("SREC start 2.0 (take 3)", c.ask("SREC start 2.0") == "OK")
        time.sleep(1.6)
        proc.send_signal(signal.SIGKILL)
        proc.wait(timeout=10)
        f3 = os.path.join(sessions, "takes", "Vox_take03.wav")
        r3 = read_take(f3) if os.path.exists(f3) else None
        check("SIGKILL mid-take: the take file is there with a readable header", r3 is not None, os.path.exists(f3))
        check("it holds the audio up to the last header patch (>= 0.75 s)", r3 and r3[3] >= 0.75 * RATE, r3 and r3[3])
        want3 = [0.25 * math.sin(2 * math.pi * 660 * i / RATE) for i in range(200)]
        check("and its samples are right", r3 and max(abs(a - b) for a, b in zip(r3[4], want3)) < 1e-4)
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
