#!/usr/bin/env python3
"""f83 scripted run: the Spectral Displays, Time Display, Loudness, General and Appearance preference keys.

    python3 tools/alpha/f83.py --exe <wavcraft> [--port 47183] [--keep]

Three runs of the app in one scratch HOME: (1) PREF get shows every default, PREF set changes every
new key, bad values are refused and change nothing, and the settings file holds the new values;
(2) after a restart PREF get returns what was set; (3) after --reset-preferences all are defaults again.
Each run ends with QUIT discard and must leave no ERROR line in the app's log.
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave, struct, math

DEFAULTS = {"spec_window": "hann", "spec_resolution": "2048", "spec_db_range": "100", "spec_play_selected_freqs": "false",
            "time_format": "decimal", "frame_rate": "30", "loudness_target_lufs": "-16", "true_peak_ceiling_dbtp": "-1",
            "zoom_factor": "2", "extend_selection": "nearest", "context_channel_editing": "true", "theme": "dark",
            "waveform_colour": "green", "ui_brightness": "1"}
CHANGED = {"spec_window": "blackman-harris", "spec_resolution": "4096", "spec_db_range": "120", "spec_play_selected_freqs": "true",
           "time_format": "smpte", "frame_rate": "29.97", "loudness_target_lufs": "-23", "true_peak_ceiling_dbtp": "-2.5",
           "zoom_factor": "3", "extend_selection": "anchor", "context_channel_editing": "false", "theme": "light",
           "waveform_colour": "amber", "ui_brightness": "0.8"}
BAD = [("spec_window", "triangle"), ("spec_resolution", "1000"), ("spec_db_range", "161"), ("spec_play_selected_freqs", "yes"),
       ("time_format", "hex"), ("frame_rate", "0"), ("loudness_target_lufs", "1"), ("true_peak_ceiling_dbtp", "0.5"),
       ("zoom_factor", "1"), ("extend_selection", "left"), ("context_channel_editing", "1"), ("theme", "sepia"),
       ("waveform_colour", "#ff0000"), ("ui_brightness", "inf")]


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
    ap.add_argument("--port", type=int, default=47183)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f83.", dir=root)
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

    def settings_files():
        return sorted(os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs if f.startswith("settings.txt"))

    def wait_exit(proc):
        try:
            proc.wait(timeout=30)
            return True
        except subprocess.TimeoutExpired:
            return False

    def run(extra, body):
        err = open(os.path.join(work, "stderr.log"), "a")
        proc = subprocess.Popen([a.exe, *extra, wav], env=env, stdout=err, stderr=err)
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

    def first(c):
        for k, v in DEFAULTS.items():
            check(f"default {k} = {v}", c.get(k) == v, c.get(k))
        for k, v in CHANGED.items():
            check(f"PREF set {k}", c.ask(f"PREF set {k} {v}") == "OK" and c.get(k) == v, c.get(k))
        for k, v in BAD:
            before = c.get(k)
            check(f"bad {k} = {v!r} refused, unchanged", c.ask(f"PREF set {k} {v}").startswith("ERR") and c.get(k) == before, c.get(k))
        text = open(settings_files()[0]).read() if settings_files() else ""
        check("a settings file was written inside the scratch HOME", len(settings_files()) == 1, settings_files())
        check("it holds every new value", all(f"{k} = {v}" in text for k, v in CHANGED.items()), text)

    def second(c):
        for k, v in CHANGED.items():
            check(f"{k} persisted across a restart", c.get(k) == v, c.get(k))

    def third(c):
        for k, v in DEFAULTS.items():
            check(f"after --reset-preferences {k} is back to {v}", c.get(k) == v, c.get(k))

    try:
        run([], first)
        run([], second)
        run(["--reset-preferences"], third)
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
