#!/usr/bin/env python3
"""f36 scripted run: scrub, pre/post-roll, rewind / fast forward, JKL shuttle and the speed readout.

    python3 tools/alpha/f36.py --exe <release wavcraft>

Starts the release app with a fresh scratch HOME and a 10 s synthetic mono file and
WAVCRAFT_NO_AUDIO=1, so the position is moved by the player's silent test sink (output=sink): this
measures the transport logic and the reported position, not sound. Drives KEYJKL, SCRUB and
ACTION transport.* through the control channel and reads STATE (play=, pos=, speed=, roll=).
Fails on any ERROR line in the app's log.
"""
import argparse, glob, json, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, SECS = 48000, 10


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 220 * i / RATE))) for i in range(RATE * SECS)))


class Control:
    def __init__(self, port, token):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
                break
            except OSError:
                time.sleep(0.25)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self):
        head = self.ask("STATE").partition(" undo=")[0]
        d = dict(kv.split("=", 1) for kv in head.split()[1:])
        return d

    def wait_open(self):
        for _ in range(200):
            st = self.state()
            if st.get("busy") == "0" and int(st["frames"]) > 0:
                return st
            time.sleep(0.1)
        raise RuntimeError("file did not open")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    o = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f36.", dir=root)
    home, logs = os.path.join(work, "home"), os.path.join(work, "logs")
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    make_wav(wav)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT="47136", WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1", WAVCRAFT_NO_AUDIO="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([o.exe, wav], env=env, stdout=err, stderr=err)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + (f"  [{detail}]" if detail else ""), flush=True)

    try:
        for _ in range(200):
            if os.path.exists(token):
                break
            time.sleep(0.1)
        c = Control(47136, open(token).read().strip())
        st = c.wait_open()
        check("file opens, transport stopped, speed 0", st["play"] == "stopped" and st["speed"] == "0", f"output={st['output']}")
        print("MEASURED with the silent test clock (WAVCRAFT_NO_AUDIO=1): this checks the transport logic and the position, not sound")
        pos = lambda: int(c.state()["pos"])
        spd = lambda: float(c.state()["speed"])
        mid = RATE * 5

        # JKL: L steps the speed up through the jkl_speed_steps at 1x or faster (1, 2, 4 by default); K stops.
        c.ask(f"SELECT {mid} {mid}")
        seen = []
        for _ in range(5):
            r = c.ask("KEYJKL l")
            time.sleep(0.05)
            seen.append(spd())
        check("L plays forward and each press steps the speed up: 1x 2x 4x (then stays)", seen == [1.0, 2.0, 4.0, 4.0, 4.0], str(seen))
        st = c.state()
        check("STATE reports playing, speed and a position beyond the start", st["play"] == "playing" and int(st["pos"]) > mid, f"pos={st['pos']} speed={st['speed']}")
        check("the readout label comes back with the key", c.ask("KEYJKL k").startswith("OK speed=0x"))
        st = c.state()
        check("K stops; the cursor returns to where play started", st["play"] == "stopped" and st["speed"] == "0" and st["pos"] == str(mid), f"pos={st['pos']}")

        # Forward speed really changes how fast the position moves.
        rates = {}
        for n, lab in ((1, "1x"), (3, "4x")):
            c.ask(f"SELECT {RATE} {RATE}")
            for _ in range(n):
                c.ask("KEYJKL l")
            p0, t0 = pos(), time.time()
            time.sleep(0.4)
            p1, t1 = pos(), time.time()
            rates[lab] = (p1 - p0) / ((t1 - t0) * RATE)
            c.ask("KEYJKL k")
        check("measured speed: about 1 s of file per second at 1x and about 4 at 4x", 0.7 < rates["1x"] < 1.4 and 2.8 < rates["4x"] < 5.5, str({k: round(v, 2) for k, v in rates.items()}))

        # J: backward; reads the file backwards, positions go down.
        c.ask(f"SELECT {mid} {mid}")
        c.ask("KEYJKL j")
        p0 = pos()
        time.sleep(0.4)
        p1 = pos()
        check("J plays backward at -1x: the position goes down", spd() == -1.0 and p1 < p0 < mid + 1 and p1 < mid - 10000, f"{p0} -> {p1}")
        c.ask("KEYJKL j")
        check("J again goes to -2x", spd() == -2.0)
        c.ask("KEYJKL l")
        check("L after J starts forward at 1x", spd() == 1.0)
        c.ask("KEYJKL k")

        # Backward to the start finishes by itself.
        c.ask(f"SELECT {RATE // 2} {RATE // 2}")
        for _ in range(4):
            c.ask("KEYJKL j")
        t0 = time.time()
        while c.state()["play"] != "stopped" and time.time() - t0 < 3:
            time.sleep(0.05)
        st = c.state()
        check("backward play runs to the start of the file and stops", st["play"] == "stopped" and st["speed"] == "0", f"play={st['play']} pos={st['pos']} after {time.time() - t0:.2f}s")

        # K+L / K+J nudge: slow crawl while held, still after.
        c.ask(f"SELECT {mid} {mid}")
        r = c.ask("KEYJKL k+l")
        t0 = time.time()
        time.sleep(0.8)
        p1 = pos()
        moved = (p1 - mid) / ((time.time() - t0) * RATE)
        check("K+L crawls forward at 0.25x while held", r.startswith("OK speed=0.25x") and spd() == 0.25 and 0.1 < moved < 0.45, f"{r}; moved {moved:.2f} s per s")
        c.ask("KEYJKL up")
        st1 = c.state()
        time.sleep(0.3)
        st2 = c.state()
        check("letting go holds the position (paused)", st1["play"] == "paused" and st1["pos"] == st2["pos"], f"{st1['pos']} then {st2['pos']}")
        c.ask("ACTION transport.stop")
        r = c.ask("KEYJKL k+j")
        time.sleep(0.5)
        check("K+J crawls backward at -0.25x", r.startswith("OK speed=-0.25x") and spd() == -0.25 and pos() < mid, r)
        c.ask("KEYJKL up")
        c.ask("ACTION transport.stop")

        # Rewind / Fast Forward steps.
        c.ask("SELECT 100000 100000")
        c.ask("ACTION transport.fastForward")
        a = c.state()["selection"]
        c.ask("ACTION transport.rewind")
        c.ask("ACTION transport.rewind")
        b = c.state()["selection"]
        check("Fast Forward steps the cursor on 1 s, Rewind back", a == f"{100000 + RATE}-{100000 + RATE}" and b == f"{100000 - RATE}-{100000 - RATE}", f"{a} then {b}")
        c.ask("SELECT 10 10")
        c.ask("ACTION transport.rewind")
        check("Rewind stops at the start of the file", c.state()["selection"] == "0-0")
        c.ask(f"SELECT {mid} {mid}")
        c.ask("ACTION transport.play")
        time.sleep(0.2)
        p0 = pos()
        c.ask("ACTION transport.fastForward")
        p1 = pos()
        check("while playing, Fast Forward jumps the playhead 1 s on", p1 - p0 > RATE * 0.9 and c.state()["play"] == "playing", f"{p0} -> {p1}")
        c.ask("ACTION transport.stop")

        # Pre-roll / post-roll (the preferences default to 2 s; this check uses 1 s each).
        c.ask("PREF set preroll_s 1")
        c.ask("PREF set postroll_s 1")
        sel_a, sel_b = RATE * 4, RATE * 5
        c.ask(f"SELECT {sel_a} {sel_b}")
        c.ask("ACTION transport.play")
        time.sleep(0.15)
        first = pos()
        c.ask("ACTION transport.stop")
        check("roll off: a selection starts at its own start", sel_a <= first < sel_a + RATE // 2, f"pos={first}")
        check("roll toggled on", c.state()["roll"] == "0" and (c.ask("ACTION transport.roll").startswith("OK")) and c.state()["roll"] == "1")
        c.ask(f"SELECT {sel_a} {sel_b}")
        c.ask("ACTION transport.play")
        seen, t0 = [], time.time()
        while time.time() - t0 < 5:
            st = c.state()
            if st["play"] == "stopped":
                break
            seen.append(int(st["pos"]))
            time.sleep(0.02)
        st = c.state()
        check("roll on: starts 1 s before the selection", seen and RATE * 3 <= seen[0] < RATE * 3 + RATE // 2, f"first pos={seen[0] if seen else None}")
        check("... and carries on 1 s after it, then stops with the selection unchanged", max(seen) > sel_b + RATE // 2 and max(seen) <= sel_b + RATE and st["play"] == "stopped" and st["selection"] == f"{sel_a}-{sel_b}", f"max pos={max(seen) if seen else None} selection={st['selection']}")
        c.ask("ACTION transport.roll")

        # Scrub: drag the playhead, grains at the drag's speed.
        c.ask("SELECT 0 0")
        c.ask("SCRUB 100000")
        time.sleep(0.1)
        r = c.ask("SCRUB 104800")
        spd_fwd = float(r.split("speed=")[1])
        st = c.state()
        check("scrub forward at about 1x: playhead at the drag position, a grain sounding", r.startswith("OK pos=104800") and 0.5 < spd_fwd < 2.0 and st["pos"] == "104800" and st["speed"] != "0" and st["play"] == "stopped", f"{r}; STATE speed={st['speed']} scrubspeed={st['scrubspeed']}")
        time.sleep(0.1)
        r = c.ask("SCRUB 100000")
        spd_back = float(r.split("speed=")[1])
        st = c.state()
        check("scrub backward: negative speed, a backward grain", spd_back < -0.5 and float(st["speed"]) < 0, f"{r}; STATE speed={st['speed']}")
        time.sleep(0.3)
        st = c.state()
        check("the grain is short: sound stops by itself, the playhead stays", st["speed"] == "0" and st["pos"] == "100000", f"speed={st['speed']} pos={st['pos']}")
        c.ask("SCRUB 100000")
        time.sleep(0.05)
        r = c.ask("SCRUB 400000")
        check("a fast drag is clamped to 4x", float(r.split("speed=")[1]) == 4.0, r)
        c.ask("SCRUB end")
        st = c.state()
        check("letting go leaves the cursor at the playhead", st["selection"] == "400000-400000", st["selection"])
        c.ask(f"SELECT {mid} {mid + 1000}")
        c.ask("SCRUB 20000")
        c.ask("SCRUB end")
        check("scrubbing with a selection keeps it", c.state()["selection"] == f"{mid}-{mid + 1000}")
        check("scrub beyond the file is clamped", c.ask(f"SCRUB {RATE * SECS * 3}").startswith(f"OK pos={RATE * SECS}"))
        c.ask("SCRUB end")

        check("QUIT", c.ask("QUIT discard") == "OK")
        proc.wait(15)
    except Exception as e:
        check("script", False, f"{type(e).__name__}: {e}")
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()
        err.close()
    errors = []
    for f in glob.glob(os.path.join(logs, "*")) + [os.path.join(work, "stderr.log")]:
        try:
            errors += [l.strip() for l in open(f, errors="replace") if "ERROR" in l or "panicked" in l]
        except OSError:
            pass
    check("no ERROR line in the app's log", not errors, "; ".join(errors[:3]))
    ok = all(x[1] for x in checks)
    if not ok:
        print(f"scratch kept for the logs: {work}")
    else:
        shutil.rmtree(work, ignore_errors=True)
    print("RESULT", "pass" if ok else "fail")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
