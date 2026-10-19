#!/usr/bin/env python3
"""WavCraft golden task, v3 (2026-10-19): what the app does today, on the delivered program.

    python3 tools/golden.py --exe <WavCraft.app/Contents/MacOS/wavcraft | wavcraft[.exe]> [--keep] [--core <clone>]

The app is a viewer for now (open, draw, zoom, play): editing, effects and save are in
wavcraft-core but not yet in the app (plan tasks 05-07), and it has no control channel. So v1:
a 10-minute synthetic 48 kHz stereo WAV is opened from the command line in a scratch HOME and
session folder; checked: the app stays up, the file is decoded into the session store (its full
length in float samples), a window is on screen (macOS, when tools/winlist is built), nothing
on stderr looks like a panic or an error, and the app quits on SIGTERM. With --core, the
wavcraft-core suites (edits with undo/redo, open, hostile input) run from that clone as well.
v2 (task 07a): through the app's control channel (WAVCRAFT_CONTROL_PORT), on the opened
file: Delete 10 s, Undo, Redo, Silence on the left channel only, Cut then Paste, checking the
length and the History labels after each; then QUIT through the channel, which must remove the
session folder (a clean close), and the app's own log must hold no ERROR line.
v3 (task 06a): after the edits, SAVE to a new WAV through the channel; the unsaved mark goes
on with an edit, off with the save, on again with Undo and off with Redo (back at the saved
step); a plain QUIT then closes. The saved file is checked with ffprobe (24-bit PCM, the right
length) and reopened in a second run, which must decode it to the edited length.
Effects join here as later tasks land.
"""
import argparse, json, os, platform, shutil, signal, socket, subprocess, sys, tempfile, time

MINUTES, RATE, CHANNELS = 10, 48000, 2
STORE_BYTES = MINUTES * 60 * RATE * CHANNELS * 4  # f32 samples in the session store


def store_bytes(root):
    total, packs = 0, 0
    for d, _, files in os.walk(root):
        for f in files:
            if f.startswith("pack-"):
                packs += 1
                total += os.path.getsize(os.path.join(d, f))
    return total, packs


def windows(pid):
    tool = os.path.join(os.path.dirname(os.path.abspath(__file__)), "winlist")
    if platform.system() != "Darwin" or not os.access(tool, os.X_OK):
        return None
    out = subprocess.run([tool, str(pid)], capture_output=True, text=True).stdout.strip()
    return out


class Control:
    """The app's control channel: one line per request, one per answer."""

    def __init__(self, port, token, tries=60):
        for _ in range(tries):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
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

    def state(self, wait=True, timeout=60):
        t0 = time.time()
        while True:
            answer = self.ask("STATE")
            head, _, labels = answer.partition(" undo=")
            fields = dict(kv.split("=", 1) for kv in head.split()[1:])
            undo, _, redo = labels.partition(" redo=")
            fields.update(undo=undo, redo=redo)
            if not wait or fields.get("busy") == "0" or time.time() - t0 > timeout:
                return fields
            time.sleep(0.2)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def ffprobe(path):
    out = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries",
                          "stream=codec_name,sample_rate,channels,bits_per_raw_sample,duration_ts", "-of", "json", path],
                         capture_output=True, text=True).stdout
    streams = json.loads(out or "{}").get("streams", [{}])
    return streams[0] if streams else {}


def edits(c, checks, saved_path=None):
    """The v2 edits; each check records what it saw."""
    f0 = int(c.state()["frames"])
    sec = RATE

    def check(label, ok, detail):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    # The channel must answer while the window is minimised (eframe runs no ui pass then).
    c.ask("WINDOW minimize")
    time.sleep(1.5)
    t0 = time.time()
    st = c.state(wait=False)
    hidden_ok = st.get("frames") == str(f0) and time.time() - t0 < 2.0
    c.ask("WINDOW restore")
    time.sleep(1.0)
    check("the control channel answers while the window is minimised", hidden_ok, f"{time.time() - t0:.2f} s; {st}")
    c.ask(f"SELECT {60 * sec} {70 * sec}")
    a = c.ask("ACTION edit.delete")
    st = c.state()
    check("Delete 10 s shortens the file", a.endswith("changed=1") and int(st["frames"]) == f0 - 10 * sec and st["undo"] == "Delete", f"{a}; {st}")
    c.ask("ACTION edit.undo")
    st = c.state()
    check("Undo brings it back", int(st["frames"]) == f0 and st["redo"] == "Delete", st)
    c.ask("ACTION edit.redo")
    st = c.state()
    check("Redo deletes again", int(st["frames"]) == f0 - 10 * sec, st)
    c.ask("ACTION edit.leftChannel")
    c.ask(f"SELECT 0 {sec}")
    c.ask("ACTION edit.silence")
    st = c.state()
    check("Silence on the left channel keeps the length", int(st["frames"]) == f0 - 10 * sec and st["undo"] == "Silence", st)
    c.ask("ACTION edit.leftChannel")
    c.ask(f"SELECT {2 * sec} {3 * sec}")
    c.ask("ACTION edit.cut")
    c.ask("SELECT 0 0")
    a = c.ask("ACTION edit.paste")
    st = c.state()
    check("Cut then Paste keeps the length", int(st["frames"]) == f0 - 10 * sec and st["undo"] == "Paste" and "selection=0-48000" in a, f"{a}; {st}")
    check("History holds Open plus 4 steps", st["steps"] == "5", st)
    if saved_path is None:
        return f0 - 10 * sec
    check("an edit marks the file unsaved", st.get("unsaved") == "1", st)
    a = c.ask(f"SAVE {saved_path}")
    st = c.state()
    check("Save writes the file and clears the mark", a == "OK" and st.get("unsaved") == "0" and os.path.exists(saved_path), f"{a}; {st}")
    c.ask("ACTION edit.undo")
    st = c.state()
    check("Undo after the save marks it unsaved again", st.get("unsaved") == "1", st)
    c.ask("ACTION edit.redo")
    st = c.state()
    check("Redo back to the saved step clears the mark", st.get("unsaved") == "0", st)
    info = ffprobe(saved_path)
    check("the saved WAV is 24-bit PCM, stereo, 48 kHz, the edited length",
          info.get("codec_name") == "pcm_s24le" and str(info.get("channels")) == "2" and info.get("sample_rate") == str(RATE)
          and int(info.get("duration_ts", -1)) == f0 - 10 * sec, info)
    return f0 - 10 * sec


def run(exe, keep):
    work = tempfile.mkdtemp(prefix="wavcraft-golden.")
    home, sessions = os.path.join(work, "home"), os.path.join(work, "sessions")
    os.makedirs(home)
    wav = os.path.join(work, "talk-10min.wav")
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y",
                    "-f", "lavfi", "-i", f"sine=frequency=180:sample_rate={RATE}:duration={MINUTES * 60},volume=0.5",
                    "-f", "lavfi", "-i", f"anoisesrc=d={MINUTES * 60}:c=pink:r={RATE}:a=0.05",
                    "-filter_complex", "[0][1]amix=inputs=2,aformat=channel_layouts=stereo", "-c:a", "pcm_s24le", wav], check=True)
    logs = os.path.join(work, "logs")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs, XDG_CONFIG_HOME=os.path.join(home, ".config"),
               XDG_CACHE_HOME=os.path.join(home, ".cache"), APPDATA=os.path.join(home, "AppData", "Roaming"),
               LOCALAPPDATA=os.path.join(home, "AppData", "Local"), RUST_BACKTRACE="1")
    port, token = free_port(), os.path.join(work, "control.token")
    env.update(WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    checks, t0 = [], time.time()
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([exe, wav], env=env, stdout=err, stderr=err)
    decoded, last, still = None, -1, 0
    while time.time() - t0 < 90 and proc.poll() is None:
        size, packs = store_bytes(sessions)
        still = still + 1 if size == last and size > 0 else 0
        last = size
        if size >= STORE_BYTES and still >= 2:
            decoded = round(time.time() - t0, 1)
            break
        time.sleep(0.5)
    time.sleep(2)
    alive = proc.poll() is None
    size, packs = store_bytes(sessions)
    win = windows(proc.pid) if alive else None
    checks.append({"check": "app stays up with the file open", "ok": alive, "detail": f"exit {proc.returncode}" if not alive else "running"})
    checks.append({"check": "file decoded into the session store", "ok": size >= STORE_BYTES,
                   "detail": f"{size} of {STORE_BYTES} bytes in {packs} pack(s), done at {decoded} s"})
    if win is None:
        checks.append({"check": "a window is on screen", "ok": None, "detail": "skipped: no winlist on this OS"})
    else:
        checks.append({"check": "a window is on screen", "ok": win.startswith("on-screen-windows=") and not win.startswith("on-screen-windows=0"), "detail": win})
    quit_clean, edited_len = False, None
    if alive:
        try:
            for _ in range(40):
                if os.path.exists(token):
                    break
                time.sleep(0.25)
            c = Control(port, open(token).read().strip())
            saved_path = os.path.join(work, "saved.wav")
            edited_len = edits(c, checks, saved_path)
            quit_clean = c.ask("QUIT") == "OK"
            proc.wait(15)
        except Exception as e:  # a failed step is a failed run, with its reason
            checks.append({"check": "control channel", "ok": False, "detail": f"{type(e).__name__}: {e}"})
        if proc.poll() is None:
            proc.send_signal(signal.SIGTERM)
            try:
                proc.wait(10)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
    checks.append({"check": "quits through the control channel", "ok": quit_clean and proc.returncode == 0, "detail": f"exit {proc.returncode}"})
    err.close()
    text = open(os.path.join(work, "stderr.log"), errors="replace").read()
    bad = [l for l in text.splitlines() if any(w in l.lower() for w in ("panicked", "error", "fatal"))]
    checks.append({"check": "nothing on stderr looks like a panic or an error", "ok": not bad, "detail": bad[:5] or f"{len(text.splitlines())} lines"})
    # A second run opens what the first one saved.
    saved_path = os.path.join(work, "saved.wav")
    if os.path.exists(saved_path):
        proc2 = subprocess.Popen([exe, saved_path], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            c2 = Control(port, open(token).read().strip())
            st = c2.state()
            for _ in range(100):
                if int(st.get("frames", 0)) > 0 and st.get("busy") == "0":
                    break
                time.sleep(0.2)
                st = c2.state()
            checks.append({"check": "the saved file reopens at the edited length", "ok": int(st.get("frames", 0)) == edited_len and st.get("unsaved") == "0", "detail": st})
            c2.ask("QUIT")
            proc2.wait(15)
        except Exception as e:
            checks.append({"check": "the saved file reopens at the edited length", "ok": False, "detail": f"{type(e).__name__}: {e}"})
            proc2.kill()
    leftover = [d for d in os.listdir(sessions)] if os.path.isdir(sessions) else []
    checks.append({"check": "a clean close removes the session folder", "ok": not leftover, "detail": f"{len(leftover)} left"})
    # WAVCRAFT_LOG_DIR puts the log in the work folder on every OS (on Windows the config
    # folder comes from the system, not from APPDATA).
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    checks.append({"check": "the app's own log has no ERROR", "ok": bool(log_files) and not errors, "detail": errors[:3] or f"{len(log_files)} log file(s)"})
    result = {"app": "WavCraft", "os": platform.system(), "status": "pass" if all(c["ok"] is not False for c in checks) else "fail",
              "seconds": round(time.time() - t0, 1), "checks": checks,
              "notes": [f"log: {log_files[0]}" if log_files else "no log file found"]}
    if keep:
        result["work"] = work
    else:
        shutil.rmtree(work, ignore_errors=True)
    return result


def core(clone):
    r = subprocess.run(["cargo", "test", "--release", "-p", "wavcraft-core"], cwd=clone, capture_output=True, text=True)
    lines = [l for l in (r.stdout + r.stderr).splitlines() if l.startswith("test result")]
    passed = sum(int(l.split()[3]) for l in lines)
    failed = sum(int(l.split()[5].rstrip(";")) for l in lines)
    return {"check": "wavcraft-core suites", "ok": r.returncode == 0 and failed == 0, "detail": f"{passed} passed, {failed} failed"}


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--core", metavar="CLONE", help="also run the wavcraft-core suites from this clone")
    o = ap.parse_args()
    if not shutil.which("ffmpeg"):
        raise SystemExit("ffmpeg is needed on PATH")
    res = run(o.exe, o.keep)
    if o.core:
        c = core(o.core)
        res["checks"].append(c)
        if c["ok"] is False:
            res["status"] = "fail"
    print(json.dumps(res, indent=1))
    sys.exit(0 if res["status"] == "pass" else 1)


if __name__ == "__main__":
    main()
