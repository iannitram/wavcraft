#!/usr/bin/env python3
"""f76 scripted run: sample-type dialog hook, high-DPI scale in STATE, window kept on screen.

    python3 tools/alpha/f76.py --exe <path to the release wavcraft> [--port 47176]

Two runs of the app, each with HOME in a fresh scratch folder and a synthetic 3 s stereo WAV:
 A. window.txt holds a rectangle far off every monitor: after start STATE says startup=clamped
    and the window rectangle overlaps the primary monitor; the file is rewritten with a
    rectangle that is on screen.
 B. window.txt holds a rectangle on the primary monitor: startup=restored and the window sits
    close to it.
Both: STATE reports pixels_per_point > 0, the window rectangle and the primary monitor;
`ACTION edit.convertSampleType` answers OK (the real Convert Sample Type dialog, main's ConvertDialog,
has no STATE key to read back); QUIT closes the app; the app's log has no ERROR line.
"""
import argparse, math, os, shutil, signal, socket, struct, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)) + "/..")
import golden  # noqa: E402  (Control, free_port)


def make_wav(path, secs=3, rate=48000):
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(rate)
        frames = b"".join(struct.pack("<hh", int(12000 * math.sin(2 * math.pi * 220 * i / rate)), int(8000 * math.sin(2 * math.pi * 330 * i / rate))) for i in range(secs * rate))
        w.writeframes(frames)


def rect(text):
    try:
        x, y, w, h = (int(v) for v in text.split(","))
        return x, y, w, h
    except ValueError:
        return None


def overlap(a, b):
    w = min(a[0] + a[2], b[0] + b[2]) - max(a[0], b[0])
    h = min(a[1] + a[3], b[1] + b[3]) - max(a[1], b[1])
    return max(w, 0), max(h, 0)


def start(exe, scratch, port, wav, saved_rect):
    home = os.path.join(scratch, "home")
    os.makedirs(home, exist_ok=True)
    if saved_rect:
        for sub in ("Library/Application Support", ".config", "AppData/Roaming"):
            d = os.path.join(home, sub, "wavcraft")
            os.makedirs(d, exist_ok=True)
            open(os.path.join(d, "window.txt"), "w").write(saved_rect + "\n")
    logs = os.path.join(scratch, "logs")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=os.path.join(scratch, "sessions"), WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), APPDATA=os.path.join(home, "AppData", "Roaming"),
               WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=os.path.join(scratch, "control.token"), RUST_BACKTRACE="1")
    err = open(os.path.join(scratch, "stderr.log"), "w")
    proc = subprocess.Popen([exe, wav], env=env, stdout=err, stderr=err)
    token = os.path.join(scratch, "control.token")
    for _ in range(80):
        if os.path.exists(token):
            break
        time.sleep(0.25)
    c = golden.Control(port, open(token).read().strip())
    return proc, c, home, logs


def state(c):
    st = c.state()
    return st


def wait_for(c, pred, secs=15):
    t0, st = time.time(), None
    while time.time() - t0 < secs:
        st = c.state()
        if pred(st):
            return st
        time.sleep(0.3)
    return st


def run(exe, port, keep):
    checks = []

    def check(label, ok, detail):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    base = os.environ.get("F76_SCRATCH_BASE") or tempfile.gettempdir()
    os.makedirs(base, exist_ok=True)
    scratch = tempfile.mkdtemp(prefix="f76.", dir=base)
    wav = os.path.join(scratch, "tone.wav")
    make_wav(wav)
    logs_all, primary = [], None
    try:
        for name, saved in (("A off-screen", "-30000,-30000,900,600"), ("B on-screen", None)):
            sub = os.path.join(scratch, name[0])
            os.makedirs(sub, exist_ok=True)
            if name[0] == "B":
                x, y, w, h = primary
                saved = f"{x + 120},{y + 90},1000,640"
            proc, c, home, logs = start(exe, sub, port, wav, saved)
            logs_all.append(logs)
            try:
                st = wait_for(c, lambda s: s.get("frames") not in (None, "0") and s.get("startup") not in (None, "pending"))
                check(f"{name}: STATE reports scale, window and primary monitor", float(st.get("pixels_per_point", 0)) > 0 and rect(st.get("window", "")) and rect(st.get("primary", "")), {k: st.get(k) for k in ("pixels_per_point", "window", "primary", "monitors", "startup")})
                prim = rect(st.get("primary", ""))
                if name[0] == "A":
                    primary = prim
                    st = wait_for(c, lambda s: s.get("startup") == "clamped" and (rect(s.get("window", "")) or (0, 0, 0, 0))[0] > -20000)
                    win = rect(st.get("window", ""))
                    ow, oh = overlap(win, prim) if win and prim else (0, 0)
                    check("A: a saved rectangle off every monitor is clamped onto the primary monitor", st.get("startup") == "clamped" and ow >= 120 and oh >= 48, f"startup={st.get('startup')} window={st.get('window')} primary={st.get('primary')}")
                    time.sleep(2.0)  # the keeper rewrites the file once the window is there
                    wf = [os.path.join(home, s, "wavcraft", "window.txt") for s in ("Library/Application Support", ".config", "AppData/Roaming")]
                    written = [rect(open(p).read().strip().replace("\n", "")) for p in wf if os.path.exists(p)]
                    check("A: the saved file now holds an on-screen rectangle", any(r and r[0] > -20000 and overlap(r, prim)[0] >= 120 for r in written), written)
                else:
                    st = wait_for(c, lambda s: s.get("startup") == "restored" and abs((rect(s.get("window", "")) or (0, 0, 0, 0))[0] - (prim[0] + 120)) < 60)
                    win = rect(st.get("window", ""))
                    want = rect(saved)
                    near = win and all(abs(a - b) <= 60 for a, b in zip(win, want))
                    check("B: an on-screen saved rectangle is restored", st.get("startup") == "restored" and near, f"startup={st.get('startup')} window={st.get('window')} wanted={saved}")
                a = c.ask("ACTION edit.convertSampleType")
                check("ACTION edit.convertSampleType is accepted with a file open", a.startswith("OK"), a)
                check("QUIT closes the app", c.ask("QUIT") == "OK" and proc.wait(20) == 0, f"exit {proc.returncode}")
            except Exception as e:
                check(f"{name}: run", False, f"{type(e).__name__}: {e}")
            finally:
                if proc.poll() is None:
                    proc.send_signal(signal.SIGTERM)
                    try:
                        proc.wait(10)
                    except subprocess.TimeoutExpired:
                        proc.kill()
        errors = [l.strip()[:200] for lg in logs_all for d, _, fs in os.walk(lg) for f in fs if f == "wavcraft.log" for l in open(os.path.join(d, f), errors="replace") if " ERROR " in l]
        found = [os.path.join(d, f) for lg in logs_all for d, _, fs in os.walk(lg) for f in fs if f == "wavcraft.log"]
        check("the app's log has no ERROR line", bool(found) and not errors, errors[:3] or f"{len(found)} log file(s)")
        for lg in found:
            for l in open(lg, errors="replace"):
                if "window start-up" in l:
                    print("log:", l.strip()[-200:])
    finally:
        if keep:
            print("scratch kept:", scratch)
        else:
            shutil.rmtree(scratch, ignore_errors=True)
    for c in checks:
        print(("PASS" if c["ok"] else "FAIL"), c["check"], "|", c["detail"])
    ok = all(c["ok"] for c in checks)
    print("f76 scripted run:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47176)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    sys.exit(run(a.exe, a.port, a.keep))
