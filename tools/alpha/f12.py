#!/usr/bin/env python3
"""Alpha run for f12 (A152, A153): five internal clipboards and the system clipboard.

    python3 tools/alpha/f12.py --exe <wavcraft> [--keep]

Starts the release app with HOME in a fresh scratch folder and a synthetic 4 s stereo WAV, then
through the control channel (port 47112): copies different lengths into clipboards 1 and 2 and
checks they survive switching (STATE clipboard= / clip_frames=); pastes from the current one;
checks that Copy put a 24-bit WAV with the copied samples on the system clipboard (OSCLIP);
puts a WAV on the system clipboard as another program would and pastes it; checks that a second
Paste (into an empty clipboard) does not read it again; QUITs; and fails on any ERROR line in
the app's log or a temp folder left behind.
"""
import argparse, math, os, shutil, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import golden  # noqa: E402  (the control-channel client)

RATE, PORT = 48000, 47112


def tone(f, n, amp=8000):
    return [int(amp * math.sin(2 * math.pi * f * k / RATE)) for k in range(n)]


def write_wav(path, chans):
    with wave.open(path, "wb") as w:
        w.setnchannels(len(chans)); w.setsampwidth(2); w.setframerate(RATE)
        n = len(chans[0])
        w.writeframes(b"".join(int(chans[c][k]).to_bytes(2, "little", signed=True) for k in range(n) for c in range(len(chans))))


def read_wav(path, start, count):
    with wave.open(path, "rb") as w:
        ch, width, n = w.getnchannels(), w.getsampwidth(), w.getnframes()
        w.setpos(start)
        raw = w.readframes(count)
    vals = [int.from_bytes(raw[i:i + width], "little", signed=True) for i in range(0, len(raw), width)]
    return ch, width, n, [vals[c::ch] for c in range(ch)]


def run(exe, keep):
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f12.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    for d in (home, sessions, logs):
        os.makedirs(d)
    left, right = tone(440, 4 * RATE), tone(660, 4 * RATE)
    src = os.path.join(work, "src.wav")
    write_wav(src, [left, right])
    foreign = os.path.join(work, "other-app.wav")
    write_wav(foreign, [[1000] * 5000])  # mono, a constant: easy to find after the paste
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail=""):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([exe, src], env=env, stdout=err, stderr=err)
    pid = proc.pid
    try:
        for _ in range(80):
            if os.path.exists(token):
                break
            time.sleep(0.25)
        c = golden.Control(PORT, open(token).read().strip())
        st = c.state()
        f0 = int(st["frames"])
        check("the file opens (4 s stereo)", f0 == 4 * RATE and st["channels"] == "2", st)
        check("clipboard 1 is current and empty", st["clipboard"] == "1" and st["clip_frames"] == "0", st)
        c.ask("SELECT 0 24000")
        a = c.ask("ACTION edit.copy")
        st = c.state()
        check("Copy fills clipboard 1", a.startswith("OK") and st["clip_frames"] == "24000", f"{a}; {st}")
        a = c.ask("ACTION edit.clipboard.2")
        st = c.state()
        check("Set Current Clipboard 2: empty", a.startswith("OK") and st["clipboard"] == "2" and st["clip_frames"] == "0", f"{a}; {st}")
        c.ask("SELECT 48000 60000")
        c.ask("ACTION edit.copy")
        st = c.state()
        check("Copy fills clipboard 2 separately", st["clip_frames"] == "12000", st)
        c.ask("ACTION edit.clipboard.1")
        st = c.state()
        check("switching back: clipboard 1 still holds its clip", st["clipboard"] == "1" and st["clip_frames"] == "24000", st)
        c.ask("ACTION edit.clipboard.2")
        st = c.state()
        check("switching again: clipboard 2 still holds its clip", st["clip_frames"] == "12000", st)
        # The system clipboard holds the last Copy (clipboard 2): a 24-bit WAV with those samples.
        ans = c.ask("OSCLIP")
        path = ans.split("path=", 1)[1] if "path=" in ans else ""
        ok = ans.startswith("OK ours=1") and path.endswith(".wav") and os.path.isfile(path)
        check("Copy put a WAV file on the system clipboard", ok, ans)
        if ok:
            ch, width, n, data = read_wav(path, 0, 12000)
            same = ch == 2 and width == 3 and n == 12000 and data[0] == [v * 256 for v in left[48000:60000]] and data[1] == [v * 256 for v in right[48000:60000]]
            check("that WAV is 24-bit stereo with the copied samples", same, f"{ch} ch, {width} bytes, {n} frames")
        # Paste uses the current clipboard (1: 24000 frames) and not the system clipboard's newer entry.
        c.ask("ACTION edit.clipboard.1")
        c.ask("SELECT 100000 100000")
        a = c.ask("ACTION edit.paste")
        st = c.state()
        check("Paste uses the current clipboard", a.endswith("changed=1") and int(st["frames"]) == f0 + 24000, f"{a}; {st}")
        # Another program's audio on the system clipboard is what Paste takes next.
        a = c.ask(f"OSCLIP set {foreign}")
        check("another program's WAV put on the system clipboard", a == "OK", a)
        c.ask("SELECT 0 0")
        a = c.ask("ACTION edit.paste")
        st = c.state()
        check("Paste takes the other program's audio", a.endswith("changed=1") and int(st["frames"]) == f0 + 24000 + 5000 and st["clip_frames"] == "5000", f"{a}; {st}")
        out = os.path.join(work, "out.wav")
        c.ask(f"SAVE {out}")
        c.state()
        ch, width, n, data = read_wav(out, 2400, 1)
        check("the pasted samples are in the document", data[0][0] == 1000 and data[1][0] == 1000, f"{data}")
        c.ask("ACTION edit.clipboard.3")
        a = c.ask("ACTION edit.paste")
        st = c.state()
        check("the system clipboard's audio is not read a second time", a.endswith("changed=0") and int(st["frames"]) == f0 + 24000 + 5000, f"{a}; {st}")
        check("clipboard 1 still holds the pasted-in audio", c.ask("ACTION edit.clipboard.1").startswith("OK") and c.state()["clip_frames"] == "5000", c.state())
        check("quits", c.ask("QUIT discard") == "OK")
        proc.wait(20)
    except Exception as e:  # a failed step is a failed run, with its reason
        check("run", False, f"{type(e).__name__}: {e}")
    finally:
        if proc.poll() is None:
            proc.kill(); proc.wait()
        err.close()
    left_over = [d for d in os.listdir(tempfile.gettempdir()) if d == f"wavcraft-clipboard-{pid}"]
    check("the clipboard temp folder is removed on exit", not left_over, left_over)
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    status = "pass" if all(x["ok"] for x in checks) else "fail"
    for x in checks:
        print(("ok   " if x["ok"] else "FAIL ") + x["check"] + ("" if x["ok"] else f"  [{x['detail']}]"))
    print("f12 scripted run:", status)
    if keep:
        print("work:", work)
    else:
        shutil.rmtree(work, ignore_errors=True)
    return status == "pass"


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    sys.exit(0 if run(a.exe, a.keep) else 1)
