#!/usr/bin/env python3
"""f19 scripted run: zoom navigator path (NAV), zoom buttons' actions, vertical zoom, ruler scale.

    python3 tools/alpha/f19.py --exe <path to the release wavcraft binary>

Starts the app on a synthetic 20 s stereo WAV with HOME, the config folders, the session folder
and the log folder in a fresh scratch folder (removed at the end), drives it over the control
channel (port 47119), checks STATE after each step, quits, and fails on any ERROR line in the
app's own log. Prints one JSON line and exits 0 only if every check passed.
"""
import argparse, array, json, math, os, shutil, socket, subprocess, sys, tempfile, time, wave

PORT, RATE, SECS = 47119, 48000, 20


def make_wav(path):
    n = RATE * SECS
    left, right = array.array("h"), array.array("h")
    for i in range(n):
        env = 0.1 + 0.9 * abs(math.sin(i / RATE * 0.7))  # swells so the overview has a shape
        left.append(int(20000 * env * math.sin(2 * math.pi * 220 * i / RATE)))
        right.append(int(12000 * env * math.sin(2 * math.pi * 330 * i / RATE)))
    inter = array.array("h")
    for l, r in zip(left, right):
        inter.append(l)
        inter.append(r)
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(inter.tobytes())


class Control:
    def __init__(self, port, token):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
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
            head = self.ask("STATE").partition(" undo=")[0]
            st = dict(kv.split("=", 1) for kv in head.split()[1:])
            if (st.get("busy") == "0" and int(st.get("frames", 0)) > 0) or time.time() - t0 > timeout:
                return st
            time.sleep(0.2)


def span(st):
    a, b = (float(x) for x in st["view"].split("-"))
    return a, b


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f19.", dir=scratch_root)
    checks = []

    def check(label, ok, detail=""):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    proc = None
    try:
        home = os.path.join(work, "home")
        logs = os.path.join(work, "logs")
        os.makedirs(home)
        wav = os.path.join(work, "tone.wav")
        make_wav(wav)
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_LOG_DIR=logs,
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
                   WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token)
        err = open(os.path.join(work, "stderr.log"), "w")
        proc = subprocess.Popen([exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=err)
        while not os.path.exists(token):
            time.sleep(0.2)
        c = Control(PORT, open(token).read().strip())
        st = c.state()
        frames = int(st["frames"])
        time.sleep(2.0)  # let the window lay out once, so the view has its real width
        st = c.state()
        check("STATE reports the view, vertical zoom and ruler scale", all(k in st for k in ("view", "vzoom", "ampscale")), st)
        a, b = span(st)
        check("a new file opens fitted: the view is the whole file", a == 0 and abs(b - frames) <= 1, st["view"])
        check("vertical zoom starts at 1 and the ruler at its default scale", st["vzoom"] == "1.00" and st["ampscale"] == "percent", st)

        ans = c.ask(f"NAV {RATE} {2 * RATE}")
        a, b = span(c.state())
        check("NAV sets the visible range", ans.startswith("OK") and a == RATE and abs(b - 2 * RATE) <= 1, f"{ans}")
        c.ask(f"NAV {RATE} {RATE}")
        check("NAV with an empty range is refused", c.ask(f"NAV {RATE} {RATE}").startswith("ERR"))

        c.ask("ACTION view.zoom.in")
        a2, b2 = span(c.state())
        check("view.zoom.in halves the visible range", abs((b2 - a2) - RATE / 2) < RATE * 0.01, f"{a2}-{b2}")
        c.ask("ACTION view.zoom.out")
        a3, b3 = span(c.state())
        check("view.zoom.out doubles it again", abs((b3 - a3) - RATE) < RATE * 0.01, f"{a3}-{b3}")

        c.ask(f"SELECT {5 * RATE} {7 * RATE}")
        ans = c.ask("ACTION view.zoom.selection")
        a, b = span(c.state())
        check("view.zoom.selection shows the selection", ans.startswith("OK") and abs(a - 5 * RATE) <= 1 and abs(b - 7 * RATE) <= 2, f"{a}-{b}")
        c.ask("ACTION view.zoom.full")
        time.sleep(0.5)
        a, b = span(c.state())
        check("view.zoom.full shows the whole file", a == 0 and abs(b - frames) <= 1, f"{a}-{b}")
        check("zoom to the selection needs a selection", (c.ask("SELECT 0 0"), c.ask("ACTION view.zoom.selection"))[1].startswith("ERR"))

        vz = []
        for name in ("ampIn", "ampIn", "ampOut", "ampReset"):
            ans = c.ask(f"ACTION view.zoom.{name}")
            vz.append((ans.startswith("OK"), c.state()["vzoom"]))
        check("vertical zoom: in, in, out, reset gives 2, 4, 2, 1", vz == [(True, "2.00"), (True, "4.00"), (True, "2.00"), (True, "1.00")], vz)
        for _ in range(14):
            c.ask("ACTION view.zoom.ampIn")
        check("vertical zoom stops at its limit", c.state()["vzoom"] == "1024.00", c.state()["vzoom"])
        c.ask("ACTION view.zoom.ampReset")

        for scale in ("db", "samples", "normalized", "percent", "db"):
            ans = c.ask(f"AMPSCALE {scale}")
            check(f"AMPSCALE {scale}", ans == f"OK ampscale={scale}" and c.state()["ampscale"] == scale, ans)
        check("a bad scale is refused", c.ask("AMPSCALE furlongs").startswith("ERR"))
        saved = [os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs if f == "settings.txt"]
        text = open(saved[0]).read() if saved else ""
        check("the scale is saved in the settings file", "amp_scale = db" in text, saved or "no settings.txt under the scratch HOME")

        check("QUIT answers OK", c.ask("QUIT") == "OK")
        try:
            proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            proc.kill()
            check("the app closed after QUIT", False, "killed after 30 s")
        else:
            check("the app closed after QUIT", True, proc.returncode)
        logfiles = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
        errors = [l.strip()[:200] for p in logfiles for l in open(p, errors="replace") if " ERROR " in l]
        check("the app's own log has no ERROR line", bool(logfiles) and not errors, errors[:3] or f"{len(logfiles)} log file(s)")
        stderr = open(os.path.join(work, "stderr.log"), errors="replace").read()
        check("no panic on stderr", "panicked" not in stderr, stderr[-300:])
    except Exception as e:  # report, do not hide
        check("the run completed", False, repr(e))
    finally:
        if proc and proc.poll() is None:
            proc.kill()
        failed = [c for c in checks if not c["ok"]]
        if failed:
            print("kept scratch folder for the log:", work, file=sys.stderr)
        else:
            shutil.rmtree(work, ignore_errors=True)
    ok = checks and not failed
    for c in checks:
        print(("PASS " if c["ok"] else "FAIL ") + c["check"] + ("" if c["ok"] else f"  -> {c['detail']}"))
    print(json.dumps({"passed": sum(c["ok"] for c in checks), "total": len(checks), "ok": bool(ok)}))
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
