#!/usr/bin/env python3
"""f97 scripted run: audio hardware settings kept per device setup (no input device is opened).

    python3 tools/alpha/f97.py --exe <wavcraft> [--port 47197] [--keep]

Setup A (simulated devices Built-in, Speakers) is given buffer 512 and map 1:2; setup B (Built-in,
Speakers, USB_Interface) gets buffer 128, map 1:3,2:4 and a missing device. Switching with
AUDIOPROFILE simulate restores each setup's settings (checked through AUDIOINFO and PREF get); a
stored device that is not on offer falls back to the system default; a restart keeps the profiles.
Ends with QUIT and no ERROR line in the app's log.
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
        try:
            return self.f.readline().strip()
        except OSError:
            print(f"     no reply to {line!r}", flush=True)
            raise

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
    ap.add_argument("--port", type=int, default=47197)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f97.", dir=root)
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
            try:
                body(c)
            except Exception:
                if os.environ.get("F97_SAMPLE"):
                    subprocess.run(["sample", str(proc.pid), "2", "-file", os.path.join(work, "sample.txt")], timeout=60)
                raise
            check("QUIT closes the app", c.ask("QUIT discard") == "OK" and wait_exit(proc))
        finally:
            if proc.poll() is None:
                proc.kill()

    def wait_active(c):
        for _ in range(100):
            if "active=none" not in c.ask("AUDIOPROFILE list"):
                return True
            time.sleep(0.2)
        return False

    A = "AUDIOPROFILE simulate Built-in Speakers"
    B = "AUDIOPROFILE simulate Built-in Speakers USB_Interface"

    def first(c):
        check("the launch picks a profile for the real setup", wait_active(c), c.ask("AUDIOPROFILE list"))
        real = c.info().get("profile")
        r = c.ask(A)
        check("simulate A is a new setup", r.startswith("OK outcome=NewSetup") or r.startswith("OK outcome=Adopted"), r)
        check("AUDIOINFO reports a profile", c.info().get("profile") not in (None, "none", real), c.info())
        keyA = c.info().get("profile")
        check("set buffer and map in A", c.ask("PREF set buffer_frames 512") == "OK" and c.ask("PREF set output_map 1:2") == "OK")
        r = c.ask(B)
        check("simulate B (interface plugged in) is another setup, from the defaults", r.startswith("OK outcome=NewSetup"), r)
        keyB = c.info().get("profile")
        check("B has its own key", keyB != keyA, (keyA, keyB))
        check("B starts at the defaults", c.get("buffer_frames") == "Device default" and c.get("output_map") == "", c.get("buffer_frames"))
        check("set B", c.ask("PREF set buffer_frames 128") == "OK" and c.ask("PREF set output_map 1:2,2:1") == "OK")
        r = c.ask(A)
        check("simulate A (unplugged) restores A", r.startswith("OK outcome=Restored") and c.get("buffer_frames") == "512" and c.get("output_map") == "1:2", (r, c.get("buffer_frames")))
        check("AUDIOINFO shows A's map and key", c.info().get("output_map") == "1:2" and c.info().get("profile") == keyA, c.info())
        r = c.ask(B)
        check("simulate B restores B", r.startswith("OK outcome=Restored") and c.get("buffer_frames") == "128" and c.get("output_map") == "1:2,2:1", (r, c.get("buffer_frames")))
        check("the same setup again changes nothing", c.ask(B).startswith("OK outcome=Unchanged"))
        c.ask(A)
        seen["keyB"] = keyB
        check("list shows the profiles", " count=3 " in c.ask("AUDIOPROFILE list"), c.ask("AUDIOPROFILE list"))
        check("a bad request is refused", c.ask("AUDIOPROFILE simulate").startswith("ERR") and c.ask("AUDIOPROFILE bogus").startswith("ERR"))

    def second(c):
        check("restart: a profile is picked", wait_active(c))
        r = c.ask(B)
        check("restart: B's missing device falls back to the system default, the rest is restored",
              r.startswith("OK outcome=RestoredWithFallback") and c.get("output_device") == "System default" and c.get("buffer_frames") == "128", (r, c.get("output_device")))
        check("restart: B's map came back", c.get("output_map") == "1:2,2:1", c.get("output_map"))
        r = c.ask(A)
        check("restart: A's settings come back from the file", r.startswith("OK outcome=Restored") and c.get("buffer_frames") == "512" and c.get("output_map") == "1:2", (r, c.get("buffer_frames")))

    def stale_device():
        # A device that was in B's profile and is gone now (the file is edited between the runs).
        path = next((os.path.join(d, "audio-profiles.txt") for d, _, fs in os.walk(home) if "audio-profiles.txt" in fs), None)
        text = open(path).read() if path else ""
        key = seen.get("keyB", "?")
        head, sep, tail = text.partition(f"[setup {key}]")
        tail = tail.replace("output_device = \n", "output_device = Ghost Device\n", 1)
        check("the profile file holds B's section", bool(sep) and "buffer_frames = 128" in tail and "output_map = 1:2,2:1" in tail, text[:300])
        if path:
            open(path, "w").write(head + sep + tail)

    seen = {}
    try:
        run(first)
        stale_device()
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
