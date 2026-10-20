#!/usr/bin/env python3
"""f61 scripted run: audio host, buffer size and channel mapping (output only; no input device is opened).

    python3 tools/alpha/f61.py --exe <wavcraft> [--port 47161] [--keep]

Two runs of the app in one scratch HOME:
1. AUDIOINFO reports the host in use, the hosts on offer, the buffer (default) and empty maps.
   PREF set buffer_frames <n> is applied (a value inside the device's range shows in AUDIOINFO;
   one outside it is clamped; 0 is the device default); a bad value is refused. CHANMAP out 1 3 and
   2 4 build the map 1:3,2:4; CHANMAP in sets an input map; bad channels are refused; the
   settings file holds all of it. PREF set audio_host with the host in use keeps playing.
2. A restart: AUDIOINFO and PREF get return what was set (persistence); CHANMAP reset clears both.
Each run ends with QUIT and leaves no ERROR line in the app's log.
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave, struct, math


class Control:
    def __init__(self, port, token, tries=120):
        for _ in range(tries):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
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

    def info(self):
        a = self.ask("AUDIOINFO")
        d = {}
        if a.startswith("OK "):
            body, _, dev = a[3:].partition(" device=")
            d = dict(kv.split("=", 1) for kv in body.split(" "))
            d["device"] = dev
        return d

    def get(self, key):
        a = self.ask(f"PREF get {key}")
        return a[3:].strip() if a.startswith("OK") else a


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
        w.writeframes(b"".join(struct.pack("<hh", int(8000 * math.sin(i / 20)), int(8000 * math.sin(i / 31))) for i in range(48000 * 2)))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47161)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f61.", dir=root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    make_wav(wav)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               APPDATA=os.path.join(home, "AppData", "Roaming"), RUST_BACKTRACE="1",
               WAVCRAFT_CONTROL_PORT=str(a.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"), flush=True)

    def settings_text():
        for d, _, fs in os.walk(home):
            if "settings.txt" in fs:
                return open(os.path.join(d, "settings.txt")).read()
        return ""

    def wait_exit(proc):
        try:
            proc.wait(timeout=30)
            return True
        except subprocess.TimeoutExpired:
            return False

    def run(body):
        err = open(os.path.join(work, "stderr.log"), "a")
        proc = subprocess.Popen([a.exe, wav], env=env, stdout=err, stderr=err)
        try:
            for _ in range(240):
                if os.path.exists(token) or proc.poll() is not None:
                    break
                time.sleep(0.25)
            c = Control(a.port, open(token).read().strip())
            body(c)
            check("QUIT closes the app", c.ask("QUIT discard") == "OK" and wait_exit(proc))
        finally:
            if proc.poll() is None:
                proc.kill()

    seen = {}

    def first(c):
        i = c.info()
        print(f"     AUDIOINFO: {i}", flush=True)
        check("AUDIOINFO answers", {"host", "hosts", "buffer", "buffer_range", "out_channels", "output_map", "input_map"} <= set(i), i)
        seen["host"] = i.get("host")
        check("a host is in use and is on offer", i.get("host") not in (None, "none", "") and i["host"] in i.get("hosts", "").split("|"), i)
        check("buffer starts at the device default", i.get("buffer") == "default" and c.get("buffer_frames") == "Device default", i)
        check("maps start empty", i.get("output_map") == "" and i.get("input_map") == "", i)
        rng = i.get("buffer_range", "unknown")
        if rng != "unknown":
            lo, hi = (int(x) for x in rng.split("-"))
            want = min(max(512, lo), hi)
            check("PREF set buffer_frames 512 is applied (clamped to the device range)", c.ask("PREF set buffer_frames 512") == "OK" and c.info().get("buffer") == str(want), c.info())
            check("a buffer above the range is clamped", c.ask(f"PREF set buffer_frames {hi + 100000 if hi + 100000 <= 65536 else 65536}") == "OK" and c.info().get("buffer") in (str(hi), "default"), c.info())
            seen["buffer"] = 512
            c.ask("PREF set buffer_frames 512")
        else:
            print("     the device does not report a buffer range: sizes are stored but not applied", flush=True)
            check("PREF set buffer_frames 512 is stored", c.ask("PREF set buffer_frames 512") == "OK" and c.get("buffer_frames") == "512")
        check("a bad buffer value is refused", c.ask("PREF set buffer_frames banana").startswith("ERR") and c.get("buffer_frames") == "512")
        check("CHANMAP out 1 3", c.ask("CHANMAP out 1 3") == "OK output_map=1:3 input_map=")
        check("CHANMAP out 2 4", c.ask("CHANMAP out 2 4") == "OK output_map=1:3,2:4 input_map=")
        check("CHANMAP in 3 1", c.ask("CHANMAP in 3 1").startswith("OK output_map=1:3,2:4 input_map=3:1"))
        check("AUDIOINFO shows the map", c.info().get("output_map") == "1:3,2:4" and c.info().get("input_map") == "3:1", c.info())
        check("a bad channel is refused", c.ask("CHANMAP out 0 3").startswith("ERR") and c.ask("CHANMAP out x y").startswith("ERR") and c.ask("CHANMAP bogus").startswith("ERR"))
        check("PREF set output_map replaces it whole", c.ask("PREF set output_map 1:4,2:3") == "OK" and c.get("output_map") == "1:4,2:3")
        c.ask("CHANMAP out 1 3"); c.ask("CHANMAP out 2 4")
        check("setting audio_host to the host in use keeps playing", c.ask(f"PREF set audio_host {seen['host']}") == "OK" and c.info().get("host") == seen["host"], c.info())
        check("an unknown host falls back to the default", c.ask("PREF set audio_host NoSuchHost") == "OK" and c.info().get("host") == seen["host"], c.info())
        c.ask("PREF set audio_host ")
        text = settings_text()
        check("the settings file holds buffer and maps", "buffer_frames = 512" in text and "output_map = 1:3,2:4" in text and "input_map = 3:1" in text, text)

    def second(c):
        i = c.info()
        check("buffer_frames persisted", c.get("buffer_frames") == "512", c.get("buffer_frames"))
        check("output_map persisted and reported", i.get("output_map") == "1:3,2:4", i)
        check("input_map persisted", i.get("input_map") == "3:1", i)
        if seen.get("buffer"):
            check("the buffer was applied again on the restart", i.get("buffer") != "default", i)
        check("CHANMAP reset clears both", c.ask("CHANMAP reset") == "OK output_map= input_map=" and c.info().get("output_map") == "", c.info())
        check("buffer_frames 0 is the device default", c.ask("PREF set buffer_frames 0") == "OK" and c.info().get("buffer") == "default", c.info())

    try:
        run(first)
        run(second)
    except Exception as e:
        check("the run completed", False, f"{type(e).__name__}: {e}")
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    bad = [c for c in checks if not c[1]]
    print(f"\n{len(checks) - len(bad)}/{len(checks)} checks passed")
    if a.keep or bad:
        print(f"scratch kept: {work}")
    else:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
