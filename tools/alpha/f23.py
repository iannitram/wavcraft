#!/usr/bin/env python3
"""f23 scripted run: the transport (play, pause, stop, loop, skip selection, auto-scroll).

    python3 tools/alpha/f23.py --exe <release wavcraft> [--device]

Starts the release app with a fresh scratch HOME and a 10 s synthetic mono file, drives the
transport through the control channel (ACTION transport.*) and reads the result from STATE
(play=, pos=, wraps=, view=). By default the app is started with WAVCRAFT_NO_AUDIO=1, so the
position is moved by the player's silent clock (test mode, output=none): this measures the
transport logic, not the sound device. With --device the default output device is used if there
is one (output=device) and the same checks run against the audio callback; if there is none the
run says so and falls back to the silent clock. Fails on any ERROR line in the app's log.
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
    ap.add_argument("--device", action="store_true")
    o = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f23.", dir=root)
    home, logs = os.path.join(work, "home"), os.path.join(work, "logs")
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    make_wav(wav)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT="47123", WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    if not o.device:
        env["WAVCRAFT_NO_AUDIO"] = "1"
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
        c = Control(47123, open(token).read().strip())
        st = c.wait_open()
        check("file opens, transport stopped", st["play"] == "stopped" and st["frames"] == str(RATE * SECS), st["output"] and f"output={st['output']}")
        mode = st["output"]
        print(f"MEASURED with output={mode} ({'sound device callback' if mode == 'device' else 'silent test clock, no sound device'})")
        pos = lambda: int(c.state()["pos"])

        # Play / Pause / resume / Stop returns to the start.
        c.ask(f"SELECT {RATE} {RATE}")
        c.ask("ACTION transport.play")
        time.sleep(0.5)
        st = c.state()
        check("play moves the playhead from the cursor", st["play"] == "playing" and int(st["pos"]) > RATE + 5000, f"pos={st['pos']}")
        c.ask("ACTION transport.pause")
        st1 = c.state()
        time.sleep(0.4)
        st2 = c.state()
        check("pause holds the position", st1["play"] == "paused" and st1["pos"] == st2["pos"], f"{st1['pos']} then {st2['pos']}")
        c.ask("ACTION transport.play")
        time.sleep(0.4)
        st3 = c.state()
        check("play after pause resumes from the same place", st3["play"] == "playing" and int(st3["pos"]) > int(st2["pos"]) and int(st3["pos"]) < int(st2["pos"]) + RATE, f"{st2['pos']} -> {st3['pos']}")
        a = c.ask("ACTION transport.stop")
        st = c.state()
        check("stop returns the cursor to where play started", st["play"] == "stopped" and st["selection"] == f"{RATE}-{RATE}" and st["pos"] == str(RATE), a)
        c.ask("ACTION transport.returnOnStop")
        check("Shift+X action turns return-on-stop off", c.state()["return"] == "0")
        c.ask("ACTION transport.play")
        time.sleep(0.4)
        c.ask("ACTION transport.stop")
        st = c.state()
        check("with it off, stop leaves the cursor where playback stopped", int(st["pos"]) > RATE + 5000 and st["selection"] == f"{st['pos']}-{st['pos']}", f"selection={st['selection']}")
        c.ask("ACTION transport.returnOnStop")
        check("... and back on", c.state()["return"] == "1")

        # Loop a selection.
        c.ask("SELECT 0 12000")
        c.ask("ACTION transport.loop")
        c.ask("ACTION transport.play")
        time.sleep(1.3)
        st = c.state()
        check("loop repeats the selection (0.25 s) about five times in 1.3 s", st["play"] == "playing" and st["loop"] == "1" and int(st["wraps"]) >= 3 and int(st["pos"]) < 12000, f"wraps={st['wraps']} pos={st['pos']}")
        c.ask("ACTION transport.stop")
        # Loop the view when nothing is selected.
        c.ask("ACTION view.zoomOutFull")
        for _ in range(5):
            c.ask("ACTION view.zoomIn")
        time.sleep(0.3)
        v = c.state()["view"]
        c.ask("SELECT 0 0")
        c.ask("ACTION transport.play")
        time.sleep(1.3)
        st = c.state()
        a_, b_ = (int(x) for x in v.split("-"))
        check("loop with no selection loops the view", st["play"] == "playing" and int(st["wraps"]) >= 2 and a_ <= int(st["pos"]) < b_, f"view={v} pos={st['pos']} wraps={st['wraps']}")
        c.ask("ACTION transport.stop")
        c.ask("ACTION transport.loop")
        check("loop off again", c.state()["loop"] == "0")
        c.ask("ACTION view.zoomOutFull")
        time.sleep(0.3)

        # Skip selection: 1.0 s-1.3 s skipped, played from 0 to 1.3 + 2 s.
        c.ask(f"SELECT {RATE} {int(RATE * 1.3)}")
        c.ask("ACTION transport.skipSelection")
        c.ask("ACTION transport.play")
        seen, t0 = [], time.time()
        while time.time() - t0 < 6:
            st = c.state()
            if st["play"] == "stopped":
                break
            seen.append(int(st["pos"]))
            time.sleep(0.02)
        inside = [p for p in seen if RATE <= p < int(RATE * 1.3)]
        before = [p for p in seen if p < RATE]
        after = [p for p in seen if p >= int(RATE * 1.3) and p < int(RATE * 3.3)]
        st = c.state()
        check("skip selection never plays the selection, plays before and after, then stops", not inside and len(before) > 5 and len(after) > 5 and st["play"] == "stopped",
              f"{len(seen)} samples: before={len(before)} inside={len(inside)} after={len(after)} final={st['play']}")
        check("the selection is untouched", st["selection"] == f"{RATE}-{int(RATE * 1.3)}", st["selection"])
        c.ask("ACTION transport.skipSelection")
        check("skip off again", c.state()["skip"] == "0")

        # Plays to the end by itself and stops; stop then returns the cursor.
        c.ask(f"SELECT {RATE * SECS - 24000} {RATE * SECS - 24000}")
        c.ask("ACTION transport.play")
        time.sleep(0.2)
        t0 = time.time()
        while c.state()["play"] != "stopped" and time.time() - t0 < 3:
            time.sleep(0.05)
        st = c.state()
        check("play runs to the end of the file and stops, cursor back at its start", st["play"] == "stopped" and st["selection"] == f"{RATE * SECS - 24000}-{RATE * SECS - 24000}", f"play={st['play']} sel={st['selection']}")

        # Auto-scroll: pagewise, centred, off.
        for _ in range(5):
            c.ask("ACTION view.zoomIn")
        time.sleep(0.3)
        v0 = c.state()["view"]
        a0, b0 = (int(x) for x in v0.split("-"))
        c.ask(f"SELECT {a0 + 100} {a0 + 100}")
        c.ask("ACTION transport.play")
        time.sleep(0.9)
        st = c.state()
        a1, b1 = (int(x) for x in st["view"].split("-"))
        check("auto-scroll (pagewise) pages the view on to keep the playhead in view", a1 > a0 and a1 <= int(st["pos"]) < b1, f"view {v0} -> {st['view']} pos={st['pos']}")
        c.ask("ACTION transport.followCenter")
        time.sleep(0.5)
        st = c.state()
        a2, b2 = (int(x) for x in st["view"].split("-"))
        mid = (a2 + b2) // 2
        check("centred mode keeps the playhead in the middle", st["follow"] == "center" and abs(int(st["pos"]) - mid) < (b2 - a2) * 0.15, f"view={st['view']} pos={st['pos']}")
        c.ask("ACTION transport.autoScroll")
        time.sleep(0.2)
        s1 = c.state()
        time.sleep(0.4)
        s2 = c.state()
        check("auto-scroll off: the view stays while the playhead moves", s1["autoscroll"] == "0" and s1["view"] == s2["view"] and int(s2["pos"]) > int(s1["pos"]), f"{s1['view']} / pos {s1['pos']} -> {s2['pos']}")
        c.ask("ACTION transport.stop")

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
