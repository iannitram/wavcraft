#!/usr/bin/env python3
"""f117 scripted run: the session file through the control channel (port 47217).

    python3 tools/alpha/f117.py --exe <path to the release wavcraft> [--keep]

Run 1: make a session (two tracks, two file clips, mute, volume), SESSION save, QUIT.
Run 2 (a new process): SESSION open, the SESSION state must equal run 1's. QUIT.
Run 3: one media file is deleted; SESSION open answers offline=1, the clip stays and is marked
offline in the state. QUIT. Every run: no ERROR line in the app's own log.
"""
import argparse, json, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

PORT, RATE = 47217, 48000


def make_wav(path, rate, channels, secs, hz):
    with wave.open(path, "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(rate)
        n = int(rate * secs)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * hz * i / rate))) * channels for i in range(n)))


class Control:
    def __init__(self, port, token_file):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        for _ in range(50):
            if os.path.exists(token_file) and open(token_file).read().strip():
                break
            time.sleep(0.2)
        if self.ask(f"TOKEN {open(token_file).read().strip()}") != "OK":
            raise RuntimeError("the control channel refused the token")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self):
        a = self.ask("SESSION state")
        _, _, js = a.partition(" {")
        return json.loads("{" + js) if js else {}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f117.", dir=root)
    home, logs, sessions, proj = (os.path.join(work, d) for d in ("home", "logs", "sessions", "proj"))
    for d in (home, logs, sessions, os.path.join(proj, "media")):
        os.makedirs(d)
    a, b = os.path.join(proj, "media", "a.wav"), os.path.join(proj, "media", "b.wav")
    make_wav(a, RATE, 2, 2.0, 220)
    make_wav(b, RATE, 1, 1.0, 330)
    sess = os.path.join(proj, "demo.wcsession")
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail=""):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    def run(n, body):
        if os.path.exists(token):
            os.remove(token)
        err = open(os.path.join(work, f"stderr{n}.log"), "w")
        proc = subprocess.Popen([args.exe], env=env, stdout=err, stderr=err)
        try:
            c = Control(PORT, token)
            result = body(c)
            check(f"run {n}: QUIT", c.ask("QUIT") == "OK")
            for _ in range(100):
                if proc.poll() is not None:
                    break
                time.sleep(0.1)
            check(f"run {n}: the app exited", proc.poll() is not None, proc.returncode)
            return result
        finally:
            if proc.poll() is None:
                proc.kill()

    def first(c):
        check("SESSION save with no session is refused", c.ask(f'SESSION save "{sess}"').startswith("ERR no session"))
        check("SESSION new Demo", c.ask("SESSION new Demo rate=48000 depth=24") == "OK")
        c.ask("TRACK add Drums stereo")
        c.ask("TRACK add Lead mono")
        check("two clips added", c.ask(f'CLIP add Drums "{a}" 1.0') == "OK clips=1" and c.ask(f'CLIP add Lead "{b}" 0.5') == "OK clips=2")
        c.ask("TRACK mute Lead 1")
        c.ask("TRACK volume Drums -6")
        s = c.state()
        ans = c.ask(f'SESSION save "{sess}"')
        check("SESSION save", ans.startswith("OK exported=0 offline=0"), ans)
        check("the session file exists, owner-only", os.path.exists(sess) and (os.name == "nt" or oct(os.stat(sess).st_mode & 0o777) == "0o600"))
        text = open(sess).read()
        check("the file stores relative media paths", '"media/a.wav"' in text and work not in text)
        return s

    saved = run(1, first)

    def second(c):
        check("a fresh app has no session", c.ask("SESSION state").endswith(" none"))
        ans = c.ask(f'SESSION open "{sess}"')
        check("SESSION open", ans == "OK offline=0", ans)
        return c.state()

    reopened = run(2, second)
    check("the reopened SESSION state equals the saved one", reopened == saved, (saved, reopened))
    check("it has 2 tracks, 2 clips, none offline", len(reopened["tracks"]) == 2 and not any(cl["offline"] for t in reopened["tracks"] for cl in t["clips"]))

    os.remove(b)

    def third(c):
        ans = c.ask(f'SESSION open "{sess}"')
        check("opening with a file missing still opens (offline=1)", ans == "OK offline=1", ans)
        s = c.state()
        lead = [t for t in s["tracks"] if t["name"] == "Lead"][0]
        check("the Lead clip is kept and marked offline", len(lead["clips"]) == 1 and lead["clips"][0]["offline"] is True, lead)
        check("the Drums clip is online", not [t for t in s["tracks"] if t["name"] == "Drums"][0]["clips"][0]["offline"])
        check("a damaged file is refused", (open(os.path.join(work, "bad.wcsession"), "w").write("x"), c.ask(f'SESSION open "{os.path.join(work, "bad.wcsession")}"'))[1].startswith("ERR"))
        check("and the open session is untouched", c.state() == s)

    run(3, third)
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
