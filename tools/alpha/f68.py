#!/usr/bin/env python3
"""f68 scripted run: channel layout (layered/stacked), channel names, channel enable (CHANNEL toggle).

    python3 tools/alpha/f68.py --exe <path to the release wavcraft binary>

Starts the app on a synthetic 20 s stereo WAV with HOME, the config folders, the session folder
and the log folder in a fresh scratch folder (removed at the end), drives it over the control
channel (port 47168), checks STATE after each step, quits, and fails on any ERROR line in the
app's own log. Prints one JSON line and exits 0 only if every check passed.
"""
import argparse, array, json, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

PORT, RATE, SECS = 47168, 48000, 4


def make_6ch_wav(path, mask):
    """A 6-channel WAVE_FORMAT_EXTENSIBLE file (16-bit) with the given speaker mask."""
    n = RATE * 2
    data = array.array("h")
    for i in range(n):
        for ch in range(6):
            data.append(int(8000 * math.sin(2 * math.pi * (110 * (ch + 1)) * i / RATE)))
    fmt = struct.pack("<HHIIHH", 0xFFFE, 6, RATE, RATE * 12, 12, 16)
    fmt += struct.pack("<HHIH", 22, 16, mask, 1) + bytes([0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71])
    body = b"WAVE" + b"fmt " + struct.pack("<I", len(fmt)) + fmt + b"data" + struct.pack("<I", len(data) * 2) + data.tobytes()
    with open(path, "wb") as f:
        f.write(b"RIFF" + struct.pack("<I", len(body)) + body)


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
        check("STATE reports layout, channel names and enabled channels",
              all(k in st for k in ("layout", "channelnames", "enabled")), st)
        check("a new session is stacked", st["layout"] == "stacked", st["layout"])
        check("stereo is named L,R and both channels are enabled", st["channelnames"] == "L,R" and st["enabled"] == "0,1", st)

        ans = c.ask("ACTION view.channels.layered")
        check("view.channels.layered", ans.startswith("OK") and c.state()["layout"] == "layered", ans)
        saved = [os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs if f == "settings.txt"]
        text = open(saved[0]).read() if saved else ""
        check("the layout is saved in the settings file", "channel_layout = layered" in text, saved or "no settings.txt")
        ans = c.ask("ACTION view.channels.stacked")
        check("view.channels.stacked", ans.startswith("OK") and c.state()["layout"] == "stacked", ans)

        ans = c.ask("CHANNEL toggle 1")
        check("CHANNEL toggle 1 turns the right channel off", ans == "OK enabled=0" and c.state()["enabled"] == "0", ans)
        check("the last enabled channel cannot be turned off", c.ask("CHANNEL toggle 0").startswith("ERR"))
        check("a channel the file lacks is refused", c.ask("CHANNEL toggle 5").startswith("ERR"))
        check("a bad channel is refused", c.ask("CHANNEL toggle x").startswith("ERR"))
        # Edits act on the enabled channels only: Silence on channel 0, which changes the history.
        c.ask(f"SELECT {RATE} {2 * RATE}")
        ans = c.ask("ACTION edit.silence")
        check("an edit runs with one channel enabled", ans.startswith("OK") and "changed=1" in ans, ans)
        ans = c.ask("CHANNEL toggle 1")
        check("CHANNEL toggle 1 turns it back on", ans == "OK enabled=0,1", ans)

        mask_dir = os.path.join(work, "sixch")
        os.makedirs(mask_dir)
        f51 = os.path.join(mask_dir, "five-one.wav")
        make_6ch_wav(f51, 0x3F)
        ans = c.ask(f"OPEN {f51}")
        t0 = time.time()
        while time.time() - t0 < 60:
            st = c.state()
            if st.get("channels") == "6":
                break
            time.sleep(0.3)
        check("a 5.1 file with a speaker mask is named L,R,C,LFE,Ls,Rs", st.get("channelnames") == "L,R,C,LFE,Ls,Rs" and st["enabled"] == "0,1,2,3,4,5", st)
        check("the layout choice survives opening another file", st["layout"] == "stacked", st["layout"])
        c.ask("CHANNEL toggle 3")
        check("the LFE channel can be turned off", c.state()["enabled"] == "0,1,2,4,5", c.state()["enabled"])
        c.ask("ACTION view.channels.layered")
        time.sleep(1.0)
        check("the app keeps answering with six layered channels", c.state()["layout"] == "layered")

        check("QUIT answers OK", c.ask("QUIT discard") == "OK")
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
