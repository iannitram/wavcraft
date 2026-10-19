#!/usr/bin/env python3
"""WavCraft golden task, v1 (2026-10-19): what the app does today, on the delivered program.

    python3 tools/golden.py --exe <WavCraft.app/Contents/MacOS/wavcraft | wavcraft[.exe]> [--keep] [--core <clone>]

The app is a viewer for now (open, draw, zoom, play): editing, effects and save are in
wavcraft-core but not yet in the app (plan tasks 05-07), and it has no control channel. So v1:
a 10-minute synthetic 48 kHz stereo WAV is opened from the command line in a scratch HOME and
session folder; checked: the app stays up, the file is decoded into the session store (its full
length in float samples), a window is on screen (macOS, when tools/winlist is built), nothing
on stderr looks like a panic or an error, and the app quits on SIGTERM. With --core, the
wavcraft-core suites (edits with undo/redo, open, hostile input) run from that clone as well.
Edit, effect and save join here as tasks 05-07 land.
"""
import argparse, json, os, platform, shutil, signal, subprocess, sys, tempfile, time

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


def run(exe, keep):
    work = tempfile.mkdtemp(prefix="wavcraft-golden.")
    home, sessions = os.path.join(work, "home"), os.path.join(work, "sessions")
    os.makedirs(home)
    wav = os.path.join(work, "talk-10min.wav")
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y",
                    "-f", "lavfi", "-i", f"sine=frequency=180:sample_rate={RATE}:duration={MINUTES * 60},volume=0.5",
                    "-f", "lavfi", "-i", f"anoisesrc=d={MINUTES * 60}:c=pink:r={RATE}:a=0.05",
                    "-filter_complex", "[0][1]amix=inputs=2,aformat=channel_layouts=stereo", "-c:a", "pcm_s24le", wav], check=True)
    env = dict(os.environ, HOME=home, WAVCRAFT_SESSION_DIR=sessions, XDG_CONFIG_HOME=os.path.join(home, ".config"),
               XDG_CACHE_HOME=os.path.join(home, ".cache"), APPDATA=os.path.join(home, "AppData", "Roaming"),
               LOCALAPPDATA=os.path.join(home, "AppData", "Local"), RUST_BACKTRACE="1")
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
    if alive:
        proc.send_signal(signal.SIGTERM)
        try:
            proc.wait(10)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
    checks.append({"check": "quits on SIGTERM within 10 s", "ok": proc.returncode is not None and proc.returncode != -9, "detail": f"exit {proc.returncode}"})
    err.close()
    text = open(os.path.join(work, "stderr.log"), errors="replace").read()
    bad = [l for l in text.splitlines() if any(w in l.lower() for w in ("panicked", "error", "fatal"))]
    checks.append({"check": "nothing on stderr looks like a panic or an error", "ok": not bad, "detail": bad[:5] or f"{len(text.splitlines())} lines"})
    leftover = [d for d in os.listdir(sessions)] if os.path.isdir(sessions) else []
    result = {"app": "WavCraft", "os": platform.system(), "status": "pass" if all(c["ok"] is not False for c in checks) else "fail",
              "seconds": round(time.time() - t0, 1), "checks": checks,
              "notes": [f"session folders left after SIGTERM: {len(leftover)} (the app has no logger yet, so there is no log file to read)"]}
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
