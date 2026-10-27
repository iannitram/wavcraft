#!/usr/bin/env python3
"""f69 scripted run: Zoom to Time slots, zoom in at In/Out, the zoom factor preference.

    python3 tools/alpha/f69.py --exe <path to the release wavcraft binary>

Starts the app on a synthetic 20 s stereo WAV with HOME, the config folders, the session folder
and the log folder in a fresh scratch folder (removed at the end), drives it over the control
channel (port 47169), checks STATE after each step, quits, and fails on any ERROR line in the
app's own log. Prints one JSON line and exits 0 only if every check passed.
"""
import argparse, array, json, math, os, shutil, socket, subprocess, sys, tempfile, time, wave

PORT, RATE, SECS = 47169, 48000, 20


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
    scratch_root = os.environ.get("WAVCRAFT_SCRATCH_ROOT") or tempfile.gettempdir()
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f69.", dir=scratch_root)
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
        check("STATE reports the view and the zoom factor", all(k in st for k in ("view", "zfactor")), st)
        a, b = span(st)
        check("a new file opens fitted: the view is the whole file", a == 0 and abs(b - frames) <= 1, st["view"])
        check("the zoom factor starts at 2", st["zfactor"] == "2.00", st["zfactor"])


        def width():
            a, b = span(c.state())
            return b - a

        def near(x, y, tol=RATE * 0.01):
            return abs(x - y) <= tol

        # Zoom to Time slots: recall the defaults (1 s is slot 2, 5 s is slot 3), centred on the view.
        c.ask(f"NAV {8 * RATE} {12 * RATE}")
        ans = c.ask("ACTION view.zoom.preset.2")
        a, b = span(c.state())
        check("preset slot 2 (default 1 s) shows 1 s around the middle", ans.startswith("OK") and near(b - a, RATE) and near((a + b) / 2, 10 * RATE), f"{a}-{b}")
        ans = c.ask("ACTION view.zoom.preset.3")
        check("preset slot 3 (default 5 s) shows 5 s", ans.startswith("OK") and near(width(), 5 * RATE), width())
        c.ask("ACTION view.zoom.full")
        time.sleep(0.5)
        # Set a slot from the current view, recall it from another view.
        c.ask(f"NAV {RATE} {4 * RATE}")
        ans = c.ask("ZOOMPRESET set 4")
        check("ZOOMPRESET set 4 stores the 3 s view", ans.startswith("OK slot=4") and abs(float(ans.split("secs=")[1]) - 3.0) < 0.03, ans)
        c.ask(f"NAV {10 * RATE} {11 * RATE}")
        c.ask("ACTION view.zoom.preset.4")
        check("recalling slot 4 shows 3 s", near(width(), 3 * RATE), width())
        check("ZOOMPRESET get 4 reads it back", abs(float(c.ask("ZOOMPRESET get 4").split("secs=")[1]) - 3.0) < 0.03)
        check("a bad slot is refused", c.ask("ZOOMPRESET set 6").startswith("ERR") and c.ask("ZOOMPRESET set 0").startswith("ERR"))
        check("action preset.6 does not exist", c.ask("ACTION view.zoom.preset.6").startswith("ERR"))

        # Zoom in at In / Out: the selection edge stays at the same screen position.
        c.ask(f"NAV {2 * RATE} {10 * RATE}")
        c.ask(f"SELECT {4 * RATE} {8 * RATE}")
        a0, b0 = span(c.state())
        x_in = (4 * RATE - a0) / (b0 - a0)
        ans = c.ask("ACTION view.zoom.in_at_in")
        a1, b1 = span(c.state())
        check("in_at_in halves the view", ans.startswith("OK") and near(b1 - a1, (b0 - a0) / 2), f"{a1}-{b1}")
        check("in_at_in keeps the selection start at the same place", abs((4 * RATE - a1) / (b1 - a1) - x_in) < 0.01, f"{(4 * RATE - a1) / (b1 - a1):.3f} vs {x_in:.3f}")
        c.ask(f"NAV {2 * RATE} {10 * RATE}")
        x_out = (8 * RATE - a0) / (b0 - a0)
        c.ask("ACTION view.zoom.in_at_out")
        a2, b2 = span(c.state())
        check("in_at_out keeps the selection end at the same place", near(b2 - a2, (b0 - a0) / 2) and abs((8 * RATE - a2) / (b2 - a2) - x_out) < 0.01, f"{(8 * RATE - a2) / (b2 - a2):.3f} vs {x_out:.3f}")

        # The zoom factor preference drives zoom in / out and the in-at actions.
        ans = c.ask("PREF set zoom_factor 4")
        check("PREF set zoom_factor 4", ans == "OK" and c.state()["zfactor"] == "4.00", ans)
        c.ask(f"NAV {2 * RATE} {10 * RATE}")
        c.ask("ACTION view.zoom.in")
        check("zoom in now quarters the view", near(width(), 2 * RATE), width())
        c.ask("ACTION view.zoom.out")
        check("zoom out is the inverse", near(width(), 8 * RATE), width())
        c.ask("ACTION view.zoom.in_at_in")
        check("in_at_in uses the factor too", near(width(), 2 * RATE), width())
        check("a factor of 1000 is refused and changes nothing", c.ask("PREF set zoom_factor 1000").startswith("ERR") and c.state()["zfactor"] == "4.00")
        check("a factor that is not a number is refused", c.ask("PREF set zoom_factor abc").startswith("ERR") and c.state()["zfactor"] == "4.00")
        c.ask("PREF set zoom_factor 1.5")

        saved = [os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs if f == "settings.txt"]
        text = open(saved[0]).read() if saved else ""
        line = next((l for l in text.splitlines() if l.startswith("zoom_presets")), "")
        slots = [float(x) for x in line.partition("=")[2].split(",") if x.strip()]
        check("the factor and slots are saved in the settings file", "zoom_factor = 1.5" in text and len(slots) == 5 and abs(slots[3] - 3.0) < 0.03, text[-200:])

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
