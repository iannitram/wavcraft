#!/usr/bin/env python3
"""f100 scripted run: AIFF Save writes the markers as a MARK chunk; reopening restores them.

    python3 tools/alpha/f100.py --exe <path to the release wavcraft> [--port 47200] [--keep]

A fresh scratch HOME; a 2 s synthetic stereo WAV is opened, two point markers and two range markers are
added and renamed, then SAVE x.aiff (16-bit), OPEN of the saved file, MARKERS must equal the original
list; the same with a 24-bit AIFF and a float AIFF; a WAV save still keeps its markers; no temporary file
is left; the app log holds no ERROR line.
"""
import argparse, os, shutil, signal, socket, subprocess, sys, tempfile, time

RATE, SECONDS = 48000, 2


class Control:
    def __init__(self, port, token):
        for _ in range(80):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.25)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self, timeout=60):
        t0 = time.time()
        while True:
            d = dict(kv.split("=", 1) for kv in self.ask("STATE").partition(" undo=")[0].split()[1:])
            if d.get("busy") == "0" or time.time() - t0 > timeout:
                return d
            time.sleep(0.1)

    def markers(self):
        head, _, rest = self.ask("MARKERS").partition(" ")
        body = rest.split(" ", 1)[1] if " " in rest else ""
        return [(int(s), int(e), n) for _, s, e, n in (x.split(",", 3) for x in body.split(";") if x)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47200)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f100.", dir=base)
    checks = []

    def check(label, ok, detail=""):
        checks.append(bool(ok))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"), flush=True)

    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    src = os.path.join(work, "src.wav")
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", f"sine=frequency=330:sample_rate={RATE}:duration={SECONDS}",
                    "-ac", "2", "-c:a", "pcm_s16le", src], check=True)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions, XDG_CONFIG_HOME=os.path.join(home, ".config"),
               XDG_CACHE_HOME=os.path.join(home, ".cache"), RUST_BACKTRACE="1", WAVCRAFT_CONTROL_PORT=str(a.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([a.exe, src], env=env, stdout=err, stderr=err)
    try:
        for _ in range(120):
            if os.path.exists(token):
                break
            time.sleep(0.25)
        c = Control(a.port, open(token).read().strip())
        for _ in range(100):
            st = c.state()
            if int(st.get("frames", 0)) == SECONDS * RATE:
                break
            time.sleep(0.2)
        check("opened the synthetic file", int(st["frames"]) == SECONDS * RATE, st)
        for sel in ("10000 10000", "30000 40000", "60000 60000", "80000 90000"):
            c.ask("SELECT " + sel)
            c.ask("ACTION marker.add")
        for i, n in enumerate(("Intro", "Chorus", "Bridge", "Outro")):
            c.ask(f"MARKER rename {i} {n}")
        want = c.markers()
        check("four markers (two points, two ranges)", len(want) == 4 and sorted(set(e - s for s, e, _ in want)) == [0, 10000], want)
        for name, args in [("m16.aiff", "bits=16"), ("m24.aif", "bits=24"), ("mf.aiff", "float=1"), ("m16.wav", "bits=16")]:
            path = os.path.join(work, name)
            ans = c.ask(f"SAVE {path} {args}")
            st = c.state(timeout=120)
            check(f"Save As {name}", ans == "OK" and os.path.exists(path), f"{ans}; {st}")
            if name.endswith((".aiff", ".aif")):
                check(f"{name} holds a MARK chunk", b"MARK" in open(path, "rb").read())
            ans = c.ask(f"OPEN {path}")
            for _ in range(100):
                st = c.state()
                if int(st.get("frames", 0)) == SECONDS * RATE:
                    break
                time.sleep(0.2)
            got = c.markers()
            check(f"reopened {name}: markers restored", ans == "OK" and got == want, f"{ans}; {got} != {want}")
        left = [n for n in os.listdir(work) if ".wavcraft-tmp-" in n]
        check("no temporary file left behind", not left, left)
        check("QUIT", c.ask("QUIT discard") == "OK")
        proc.wait(20)
    except Exception as e:
        check("scripted run", False, f"{type(e).__name__}: {e}")
        proc.kill()
    if proc.poll() is None:
        proc.send_signal(signal.SIGTERM)
    err.close()
    text = ""
    for d, _, files in os.walk(work):
        for n in files:
            if n.endswith(".log"):
                text += open(os.path.join(d, n), errors="replace").read()
    bad = [l for l in text.splitlines() if "ERROR" in l or "panicked" in l]
    check("no ERROR line in the app's log or stderr", not bad, bad[:5])
    ok = all(checks)
    print("RESULT", "PASS" if ok else "FAIL", f"{sum(checks)}/{len(checks)}", work if a.keep or not ok else "")
    if not a.keep and ok:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


main()
