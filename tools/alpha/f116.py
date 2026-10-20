#!/usr/bin/env python3
"""f116 scripted run: edit a clip's source in the Waveform editor and return (port 47216).

    python3 tools/alpha/f116.py --exe <path to the release wavcraft> [--keep]

A 4 s synthetic stereo WAV goes on a track as a clip at 1.0 s; CLIP edit opens its source in the
Waveform editor, one second is deleted there, SESSION return puts the edited audio back. Checked
through STATE and SESSION state: the clip keeps its start and is trimmed to 3 s, the Multitrack
view shows, the Waveform editor is empty again, a second return is refused. Then QUIT; fails on
any ERROR line in the app's own log.
"""
import argparse, json, math, os, shutil, struct, subprocess, sys, tempfile, time, wave

PORT, RATE = 47216, 48000


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


import socket


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f116.", dir=root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    a = os.path.join(work, "a.wav")
    make_wav(a, RATE, 2, 4.0, 220)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
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

    def wait_idle():
        for _ in range(200):
            st = c.ask("STATE")
            if " busy=0 " in st:
                return st
            time.sleep(0.1)
        return c.ask("STATE")

    try:
        check("SESSION new + track", c.ask("SESSION new Round rate=48000") == "OK" and c.ask("TRACK add Music stereo").startswith("OK"))
        check("CLIP add at 1.0 s", c.ask(f'CLIP add Music "{a}" 1.0') == "OK clips=1")
        clip = c.state()["tracks"][0]["clips"][0]
        check("the clip is 4 s long at 1.0 s", (clip["start"], clip["length"]) == (1.0, 4.0), clip)
        check("SESSION return with nothing open is refused", c.ask("SESSION return").startswith("ERR"))
        check("CLIP edit with a bad number is refused", c.ask("CLIP edit Music 7").startswith("ERR"))
        check("CLIP edit Music 1", c.ask("CLIP edit Music 1") == "OK")
        st = wait_idle()
        check("the source is the open document (4 s, stereo, 48 kHz)", "frames=192000 channels=2 rate=48000" in st, st)
        check("the Waveform editor is shown", c.ask("SESSION state").startswith("OK view=waveform"))
        check("a second CLIP edit is refused", c.ask("CLIP edit Music 1").startswith("ERR"))
        check("SELECT the first second", c.ask("SELECT 0 48000").startswith("OK"))
        check("ACTION edit.delete", c.ask("ACTION edit.delete").endswith("changed=1"))
        st = wait_idle()
        check("the document is 3 s now", "frames=144000 " in st, st)
        check("the session still has the old 4 s clip before returning", c.state()["tracks"][0]["clips"][0]["length"] == 4.0)
        ans = c.ask("SESSION return")
        check("SESSION return answers with the change", ans == "OK frames=144000 was=192000 trimmed=1 removed=0", ans)
        s = c.state()
        clip = s["tracks"][0]["clips"][0]
        check("the clip kept its start and was trimmed to 3 s", (clip["start"], clip["length"]) == (1.0, 3.0), clip)
        check("the session ends at 4.0 s", abs(s["end"] - 4.0) < 1e-9, s["end"])
        check("the Multitrack view is shown", c.ask("SESSION state").startswith("OK view=multitrack"))
        st = wait_idle()
        check("the Waveform editor is empty again", "frames=0 " in st, st)
        check("a second SESSION return is refused", c.ask("SESSION return").startswith("ERR"))
        time.sleep(0.5)
        check("the app is still up", proc.poll() is None)
        check("QUIT", c.ask("QUIT") == "OK")
        for _ in range(100):
            if proc.poll() is not None:
                break
            time.sleep(0.1)
        check("the app exited", proc.poll() is not None, proc.returncode)
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
