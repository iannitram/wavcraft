#!/usr/bin/env python3
"""f05 scripted run: views and playback read from the document store, no whole-file RAM copy.

    python3 tools/alpha/f05.py --exe <wavcraft> [--minutes 60] [--keep]

Opens a synthetic stereo WAV (written here, 48 kHz 16-bit) in a scratch HOME, then through the
control channel (port 47105): waits for the open, plays it from the start and checks that the
playhead advances with no read-ahead underrun, deletes a range (the views are rebuilt from the
store) and plays again, and compares the app's resident memory with what a whole-file f32 copy
would take. QUIT; the app's log must hold no ERROR line. Scratch folder removed at the end.
"""
import argparse, json, math, os, platform, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, PORT = 48000, 47105


def write_wav(path, minutes):
    """Three different minutes of two tones and a little hash noise, cycled to `minutes`."""
    import random
    rnd = random.Random(5)
    blocks = []
    for k in range(3):
        fl, fr = 220.0 * (k + 1), 330.0 * (k + 1)
        frames = []
        for i in range(RATE * 60):
            t = i / RATE
            n = (rnd.random() - 0.5) * 0.02
            frames.append(struct.pack("<hh", int(8000 * math.sin(6.2831853 * fl * t) + n * 20000), int(8000 * math.sin(6.2831853 * fr * t) + n * 20000)))
        blocks.append(b"".join(frames))
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        for m in range(minutes):
            w.writeframes(blocks[m % 3])


class Control:
    def __init__(self, port, token):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=120)
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

    def state(self, timeout=300):
        t0 = time.time()
        while True:
            fields = dict(kv.split("=", 1) for kv in self.ask("STATE").split()[1:] if "=" in kv)
            if fields.get("busy") == "0" or time.time() - t0 > timeout:
                return fields
            time.sleep(0.3)

    def playback(self):
        return dict(kv.split("=", 1) for kv in self.ask("PLAYBACK").split()[1:])


def footprint_mb(pid):
    """macOS physical footprint (what Activity Monitor calls Memory), from vmmap; falls back to the resident size."""
    import re
    out = subprocess.run(["vmmap", "-summary", str(pid)], capture_output=True, text=True).stdout
    m = re.search(r"Physical footprint:\s+([\d.]+)([KMG]?)", out)
    if not m:
        return rss_mb(pid)
    return float(m.group(1)) * {"": 1 / 1048576, "K": 1 / 1024, "M": 1, "G": 1024}[m.group(2)]


def rss_mb(pid):
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    return int(out) / 1024 if out else 0.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--minutes", type=int, default=60)
    ap.add_argument("--keep", action="store_true")
    o = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f05.", dir=base)
    checks, notes = [], []
    proc = None
    try:
        home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
        os.makedirs(home)
        wav = os.path.join(work, "long.wav")
        write_wav(wav, o.minutes)
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs, XDG_CONFIG_HOME=os.path.join(home, ".config"),
                   XDG_CACHE_HOME=os.path.join(home, ".cache"), WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
        err = open(os.path.join(work, "stderr.log"), "w")
        proc = subprocess.Popen([o.exe, wav], env=env, stdout=err, stderr=err)
        for _ in range(120):
            if os.path.exists(token):
                break
            time.sleep(0.25)
        c = Control(PORT, open(token).read().strip())
        t0 = time.time()
        st = c.state()
        frames = int(st["frames"])
        want = o.minutes * 60 * RATE
        copy_mb = frames * 2 * 4 / 1048576
        checks.append({"check": "the file opens", "ok": frames == want and st["channels"] == "2", "detail": f"{st} in {time.time() - t0:.0f} s"})
        time.sleep(3)
        rss_open = footprint_mb(proc.pid)
        checks.append({"check": "memory after open is well under a whole-file f32 copy", "ok": rss_open < 0.65 * copy_mb,
                       "detail": f"footprint {rss_open:.0f} MB; a whole-file copy alone would be {copy_mb:.0f} MB"})
        # Playback from the store: the playhead moves, the read-ahead keeps up.
        pb0 = c.playback()
        out_ok = pb0.get("output") == "ok"
        if not out_ok:
            notes.append("no audio output device here: playback checks skipped")
            checks.append({"check": "playback advances with no underrun", "ok": None, "detail": "skipped: no output device"})
        else:
            c.ask(f"PLAY 1000000 {frames}")
            time.sleep(4)
            pb = c.playback()
            moved = int(pb["pos"]) - 1000000
            checks.append({"check": "playback advances with no underrun", "ok": pb["playing"] == "1" and 3 * RATE < moved < 6 * RATE and pb["underruns"] == "0", "detail": f"{pb}; moved {moved} frames in 4 s"})
            # Seek far into the file: the read-ahead refills from there.
            c.ask(f"PLAY {frames - 30 * RATE} {frames}")
            time.sleep(3)
            pb = c.playback()
            moved = int(pb["pos"]) - (frames - 30 * RATE)
            checks.append({"check": "a far seek plays from there, no underrun", "ok": pb["playing"] == "1" and 2 * RATE < moved < 5 * RATE and pb["underruns"] == "0", "detail": f"{pb}; moved {moved}"})
        # An edit: the views are rebuilt from the store, nothing is copied.
        c.ask("PLAY 0 0")
        c.ask(f"SELECT {10 * 60 * RATE} {20 * 60 * RATE}")
        a = c.ask("ACTION edit.delete")
        st = c.state()
        checks.append({"check": "Delete 10 min: the document and views follow", "ok": a.endswith("changed=1") and int(st["frames"]) == frames - 10 * 60 * RATE, "detail": f"{a}; frames {st['frames']}"})
        time.sleep(2)
        rss_edit = footprint_mb(proc.pid)
        # The edit rebuilds the peaks by reading the whole document once through the store's block
        # cache (512 MB budget on an 8 GB machine), so resident memory may grow by about that much.
        checks.append({"check": "memory after the edit stays within the block cache budget, below a whole-file copy",
                       "ok": rss_edit < rss_open + 300 and rss_edit < 0.75 * copy_mb, "detail": f"footprint {rss_edit:.0f} MB (open {rss_open:.0f}; copy {copy_mb:.0f})"})
        if out_ok:
            c.ask("PLAY 0 2880000")
            time.sleep(2.5)
            pb = c.playback()
            checks.append({"check": "the edited document plays, no underrun", "ok": pb["playing"] == "1" and int(pb["pos"]) > RATE and pb["underruns"] == "0", "detail": str(pb)})
        notes.append(f"footprint MB: open {rss_open:.0f}, after edit {rss_edit:.0f}; whole-file copy would be {copy_mb:.0f} MB")
        c.ask("QUIT discard")
        proc.wait(30)
        err.close()
        bad = [l for l in open(os.path.join(work, "stderr.log"), errors="replace") if any(w in l.lower() for w in ("panicked", "fatal"))]
        checks.append({"check": "nothing on stderr looks like a panic", "ok": not bad, "detail": bad[:3] or "clean"})
        log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
        errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
        checks.append({"check": "the app's own log has no ERROR", "ok": bool(log_files) and not errors, "detail": errors[:3] or f"{len(log_files)} log file(s)"})
    except Exception as e:
        checks.append({"check": "run", "ok": False, "detail": f"{type(e).__name__}: {e}"})
        if proc and proc.poll() is None:
            proc.kill()
    finally:
        if not o.keep:
            shutil.rmtree(work, ignore_errors=True)
    status = "pass" if checks and all(c["ok"] is not False for c in checks) else "fail"
    print(json.dumps({"app": "WavCraft f05", "os": platform.system(), "status": status, "checks": checks, "notes": notes}, indent=1))
    sys.exit(0 if status == "pass" else 1)


if __name__ == "__main__":
    main()
