#!/usr/bin/env python3
"""f22 scripted run: Edit > Mix Paste through the control channel, results checked in saved files.

    python3 tools/alpha/f22.py --exe <path to the release wavcraft>

Synthetic 16-bit stereo files only. HOME, the session folder, the log and the token live in a
fresh scratch folder that is deleted at the end. Fails on any ERROR line in the app's log.
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, PORT = 48000, 47122


def make_wav(path, frames, left, right):
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
        w.writeframes(struct.pack("<hh", left, right) * frames)


def read_wav(path):
    """Normalised samples of a 16- or 24-bit PCM WAV: ([left], [right])."""
    with wave.open(path, "rb") as w:
        ch, width, n = w.getnchannels(), w.getsampwidth(), w.getnframes()
        raw = w.readframes(n)
    out = [[], []]
    for f in range(n):
        for c in range(2):
            b = raw[(f * ch + c) * width:(f * ch + c + 1) * width]
            out[c].append(int.from_bytes(b, "little", signed=True) / float(1 << (8 * width - 1)))
    return out


class Control:
    def __init__(self, port, token):
        for _ in range(80):
            try:
                self.f = socket.create_connection(("127.0.0.1", port), timeout=60).makefile("rw", encoding="utf-8")
                break
            except OSError:
                time.sleep(0.25)
        else:
            raise RuntimeError("no control channel")
        assert self.ask(f"TOKEN {token}") == "OK"

    def ask(self, line):
        self.f.write(line + "\n"); self.f.flush()
        return self.f.readline().strip()

    def state(self, wait=True):
        for _ in range(300):
            head, _, labels = self.ask("STATE").partition(" undo=")
            st = dict(kv.split("=", 1) for kv in head.split()[1:])
            st["undo"], _, st["redo"] = labels.partition(" redo=")
            if not wait or st.get("busy") == "0":
                return st
            time.sleep(0.1)
        return st


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--exe", required=True); ap.add_argument("--keep", action="store_true")
    exe = ap.parse_args().exe
    root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f22.", dir=root)
    home, logs, sessions, token = (os.path.join(work, n) for n in ("home", "logs", "sessions", "control.token"))
    os.makedirs(home)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_CONTROL_PORT=str(PORT),
               WAVCRAFT_CONTROL_TOKEN_FILE=token, XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"), RUST_BACKTRACE="1")
    base, clip = os.path.join(work, "base.wav"), os.path.join(work, "clip.wav")
    make_wav(base, 3 * RATE, 4000, 2000)   # 3 s: L 4000, R 2000
    make_wav(clip, RATE // 2, 1000, 3000)  # 0.5 s: L 1000, R 3000
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))

    def near(got, want):
        return abs(got - want / 32768.0) < 3 / 32768.0

    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([exe, base], env=env, stdout=err, stderr=err)
    try:
        for _ in range(300):
            if os.path.exists(token) or proc.poll() is not None:
                break
            time.sleep(0.1)
        if not os.path.exists(token):
            raise RuntimeError(f"no control token after 30 s (app exit {proc.poll()})")
        c = Control(PORT, open(token).read().strip())
        st = c.state()
        while st.get("frames") == "0":
            time.sleep(0.2); st = c.state()
        n0 = int(st["frames"]); check("opened 3 s stereo", n0 == 3 * RATE, st)

        def saved(name):
            p = os.path.join(work, name)
            assert c.ask(f"SAVE {p}") == "OK"
            c.state()
            return read_wav(p)

        def at(s, frame, l, r, label):
            check(label, near(s[0][frame], l) and near(s[1][frame], r), f"frame {frame}: got {s[0][frame]*32768:.1f},{s[1][frame]*32768:.1f} want {l},{r}")

        # 1. Overlap from a file: existing 50 %, copied 100 %, 2 loops, at 1 s -> [1 s, 2 s).
        c.ask(f"SELECT {RATE} {RATE}")
        a = c.ask(f"MIXPASTE mode=overlap copied=100 existing=50 loops=2 file={clip}")
        st = c.state()
        check("Overlap from file: one step, labelled Mix Paste, length kept", st["undo"] == "Mix Paste" and st["steps"] == "2" and int(st["frames"]) == n0, f"{a}; {st}")
        s = saved("s1.wav")
        at(s, RATE - 1, 4000, 2000, "before the paste is untouched")
        at(s, RATE, 3000, 4000, "Overlap start: 4000*0.5+1000, 2000*0.5+3000")
        at(s, RATE + RATE // 2 + 5, 3000, 4000, "second loop repeats the clip")
        at(s, 2 * RATE - 1, 3000, 4000, "Overlap last frame")
        at(s, 2 * RATE, 4000, 2000, "after the paste is untouched")
        # 2. Undo restores the original.
        c.ask("ACTION edit.undo")
        s = saved("s2.wav"); at(s, RATE, 4000, 2000, "Undo restores the original")
        # 3. Overwrite from the file at the start, copied 50 %.
        c.ask("SELECT 0 0")
        c.ask(f"MIXPASTE mode=overwrite copied=50 file={clip}")
        s = saved("s3.wav")
        at(s, 0, 500, 1500, "Overwrite: clip at 50 %")
        at(s, RATE // 2 - 1, 500, 1500, "Overwrite last frame")
        at(s, RATE // 2, 4000, 2000, "Overwrite stops at the clip's length")
        # 4. Modulate at 1 s: existing 4000/2000 times clip 1000/3000.
        c.ask(f"SELECT {RATE} {RATE}")
        c.ask(f"MIXPASTE mode=modulate file={clip}")
        s = saved("s4.wav")
        at(s, RATE + 10, 4000 * 1000 / 32768.0, 2000 * 3000 / 32768.0, "Modulate multiplies")
        # 5. Insert the clipboard, inverted, twice: copy the first 1000 frames, paste at 2 s.
        c.ask("SELECT 0 1000"); c.ask("ACTION edit.copy")
        c.ask(f"SELECT {2 * RATE} {2 * RATE}")
        c.ask("MIXPASTE mode=insert invert=1 loops=2")
        st = c.state()
        check("Insert from the clipboard grows the file by 2 x 1000 frames", int(st["frames"]) == n0 + 2000 and st["undo"] == "Mix Paste", st)
        s = saved("s5.wav")
        at(s, 2 * RATE, -500, -1500, "Insert inverted (first loop)")
        at(s, 2 * RATE + 1000, -500, -1500, "Insert inverted (second loop)")
        at(s, 2 * RATE + 2000, 4000, 2000, "the rest moved later")
        a = c.ask("MIXPASTE mode=blend")
        check("an unknown mode is refused", a.startswith("ERR"), a)
        c.ask("QUIT discard")
        proc.wait(20)
    except Exception as e:
        check("script", False, f"{type(e).__name__}: {e}")
    finally:
        if proc.poll() is None:
            proc.kill(); proc.wait()
        err.close()
    errors = [l.strip()[:200] for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log" for l in open(os.path.join(d, f), errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR line", not errors, errors[:3])
    ok = all(x[1] for x in checks)
    for label, good, detail in checks:
        print(("PASS " if good else "FAIL ") + label + ("" if good else f"  [{detail}]"))
    print("RESULT", "pass" if ok else "fail")
    if not args_keep(sys.argv):
        shutil.rmtree(work, ignore_errors=True)
    else:
        print("kept", work)
    sys.exit(0 if ok else 1)


def args_keep(argv):
    return "--keep" in argv


if __name__ == "__main__":
    main()
