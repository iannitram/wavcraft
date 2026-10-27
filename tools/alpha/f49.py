#!/usr/bin/env python3
"""f49 scripted run: snap to the ruler, coarse and fine (A125, A126, A129), through the control channel.

    python3 tools/alpha/f49.py --exe <path to the release wavcraft>

Starts the app with HOME and its logs in a scratch folder and opens a synthetic 4 s mono WAV.
Checks through STATE and `SNAP <frame> <frames_per_px>`: the defaults (coarse on, fine off), coarse
vs fine at three zoom levels, the toggles (`ACTION view.snap.ruler_coarse.toggle`,
`view.snap.ruler_fine.toggle`, `view.snap.edges.toggle`), and that the choices persist: after QUIT
the app is started again with no file and STATE shows them. Any ERROR line in the log fails the run.
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave

RATE, FRAMES, PORT = 48000, 4 * 48000, 47149


def make_wav(path):
    w = wave.open(path, "wb")
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(RATE)
    w.writeframes(b"\x00\x00" * FRAMES)
    w.close()


class Control:
    def __init__(self, port, token):
        for _ in range(80):
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
        self.f.write(line + "\n"); self.f.flush()
        return self.f.readline().strip()

    def state(self, timeout=60):
        t0 = time.time()
        while True:
            head, _, labels = self.ask("STATE").partition(" undo=")
            fields = dict(kv.split("=", 1) for kv in head.split()[1:])
            if fields.get("busy") == "0" or time.time() - t0 > timeout:
                return fields
            time.sleep(0.2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f49.", dir=base)
    checks, procs = [], []

    def check(label, ok, detail):
        checks.append(bool(ok))
        print(("PASS " if ok else "FAIL ") + label + " | " + str(detail))

    def start(args):
        home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
        os.makedirs(home, exist_ok=True)
        token = os.path.join(work, "control.token")
        if os.path.exists(token):
            os.remove(token)
        env = dict(os.environ, HOME=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions, XDG_CONFIG_HOME=os.path.join(home, ".config"),
                   XDG_CACHE_HOME=os.path.join(home, ".cache"), WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token)
        p = subprocess.Popen([exe] + args, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        procs.append(p)
        for _ in range(80):
            if os.path.exists(token):
                break
            time.sleep(0.25)
        return p, Control(PORT, open(token).read().strip())

    try:
        wav = os.path.join(work, "silence.wav")
        make_wav(wav)
        proc, c = start([wav])
        for _ in range(100):
            st = c.state()
            if int(st["frames"]) == FRAMES:
                break
            time.sleep(0.2)
        flags = lambda st: (st["snap"], st["snaprc"], st["snaprf"], st["snapedges"], st["snapzero"])
        check("defaults: snapping on, ruler coarse on, fine off, edges on, zero crossings off", flags(st) == ("1", "1", "0", "1", "0"), flags(st))
        snap = lambda f, fpp: c.ask(f"SNAP {f} {fpp}")
        # 100 frames a pixel: major 0.5 s = 24000, minor 0.1 s = 4800, reach 800 frames.
        check("100 fpp coarse: 24500 -> major tick 24000", snap(24500, 100) == "OK frame=24000", snap(24500, 100))
        check("100 fpp coarse: 28000 stays (a minor tick 800 away is not a target)", snap(28000, 100) == "OK frame=28000", snap(28000, 100))
        a = c.ask("ACTION view.snap.ruler_fine.toggle")
        st = c.state()
        check("ruler_fine.toggle turns fine on (STATE snaprf=1)", a.startswith("OK") and st["snaprf"] == "1" and st["snaprc"] == "1", f"{a}; {flags(st)}")
        check("100 fpp coarse+fine: 28000 -> minor tick 28800", snap(28000, 100) == "OK frame=28800", snap(28000, 100))
        a = c.ask("ACTION view.snap.ruler_coarse.toggle")
        st = c.state()
        check("ruler_coarse.toggle turns coarse off (fine only)", a.startswith("OK") and flags(st) == ("1", "0", "1", "1", "0"), f"{a}; {flags(st)}")
        check("100 fpp fine only: 24500 -> 24000 (majors are minor ticks too)", snap(24500, 100) == "OK frame=24000", snap(24500, 100))
        # 1000 fpp: major 5 s = 240000 (reach 8000), minor 1 s = 48000. 150000 is 6000 from 144000.
        check("1000 fpp fine only: 150000 -> 144000", snap(150000, 1000) == "OK frame=144000", snap(150000, 1000))
        # 4 fpp: major 10 ms = 480, minor 1 ms = 48, reach 32. 700 is 20 from 720.
        check("4 fpp fine only: 700 -> 720", snap(700, 4) == "OK frame=720", snap(700, 4))
        c.ask("ACTION view.snap.ruler_fine.toggle")
        check("neither ruler switch: 24500 stays", snap(24500, 100) == "OK frame=24500", snap(24500, 100))
        c.ask("ACTION view.snap.ruler_coarse.toggle")
        # 250 fpp: major 1 s = 48000 (reach 2000 frames), minor 0.1 s = 4800.
        r = (snap(46500, 250), snap(40000, 250))
        check("250 fpp coarse: 46500 -> 48000, 40000 stays", r == ("OK frame=48000", "OK frame=40000"), r)
        c.ask("SELECT 50000 60000")
        check("selection edge is a target (edges on)", snap(50300, 100) == "OK frame=50000", snap(50300, 100))
        c.ask("ACTION view.snap.edges.toggle")
        check("edges off: the edge is no target", snap(50300, 100) == "OK frame=50300" and c.state()["snapedges"] == "0", snap(50300, 100))
        # Leave a distinctive setting for the persistence check: coarse off, fine on, edges off.
        c.ask("ACTION view.snap.ruler_coarse.toggle")
        c.ask("ACTION view.snap.ruler_fine.toggle")
        st = c.state()
        check("set coarse off, fine on, edges off", flags(st) == ("1", "0", "1", "0", "0"), flags(st))
        check("QUIT", c.ask("QUIT") == "OK", "")
        proc.wait(20)
        # A second run with no file: the choices were kept in the settings file.
        proc, c = start([])
        st = c.state()
        check("after a restart (no file) the choices persist", flags(st) == ("1", "0", "1", "0", "0"), flags(st))
        check("QUIT again", c.ask("QUIT") == "OK", "")
        proc.wait(20)
        text = ""
        for d, _, files in os.walk(os.path.join(work, "logs")):
            for f in files:
                text += open(os.path.join(d, f), errors="replace").read()
        bad = [l for l in text.splitlines() if "ERROR" in l]
        check("no ERROR line in the app's log", not bad, bad[:3] or f"{len(text.splitlines())} log lines")
    except Exception as e:
        check("script ran", False, f"{type(e).__name__}: {e}")
    finally:
        for p in procs:
            if p.poll() is None:
                p.kill()
        shutil.rmtree(work, ignore_errors=True)
    print("RESULT", "pass" if all(checks) else "FAIL", f"{sum(checks)}/{len(checks)}")
    sys.exit(0 if all(checks) else 1)


main()
