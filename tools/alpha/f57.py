#!/usr/bin/env python3
"""f57 scripted run: open many files, Open Append to current and to new.

    python3 tools/alpha/f57.py --exe <wavcraft> [--port 47157] [--scratch DIR]

Writes synthetic 16-bit WAVs (a, b, c at 48 kHz stereo, d at 44.1 kHz, e mono 48 kHz, plus a
missing file), starts the app with none, then: OPEN_MANY with three files and one that does not
exist (three documents, the failure reported, the others unharmed); APPEND current with two files
(one more history step, length is the sum, an undo gives the old length back, a redo the sum); a
44.1 kHz file refused with a message and no step; APPEND new with three files (a new document of
the sum of their lengths, the sample rate mismatch reported); a mono file appended to stereo; and
the audio is checked by saving the appended document and reading it back. Then CLOSE discards and
QUITs. Fails on an ERROR line in the app's log. HOME, the session folder, the log folder and the
token file are all inside a fresh scratch folder, removed at the end.
"""
import argparse, array, math, os, shutil, socket, subprocess, sys, tempfile, time, wave


def write_wav(path, rate, ch, secs, freq):
    n = int(rate * secs)
    data = array.array("h")
    for i in range(n):
        v = int(12000 * math.sin(2 * math.pi * freq * i / rate))
        data.extend([v] * ch)
    with wave.open(path, "wb") as w:
        w.setnchannels(ch)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(data.tobytes())
    return n


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47157)
    ap.add_argument("--scratch", default=os.path.expanduser("~/dev/craft/_wt/scratch"))
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(args.scratch, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f57.", dir=args.scratch)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    token_file = os.path.join(work, "control.token")
    p = lambda n: os.path.join(work, n)
    lens = {
        "a": write_wav(p("a.wav"), 48000, 2, 2.0, 220),
        "b": write_wav(p("b.wav"), 48000, 2, 1.5, 330),
        "c": write_wav(p("c.wav"), 48000, 2, 1.0, 440),
        "d": write_wav(p("d.wav"), 44100, 2, 1.0, 550),
        "e": write_wav(p("e.wav"), 48000, 1, 0.5, 660),
    }
    # junk.wav is never written: a missing file is the failure (a file of junk bytes makes the
    # decoder library log its own ERROR line, which this script must not hide; core tests cover it).
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"), RUST_BACKTRACE="1",
               WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token_file)
    err = open(p("stderr.log"), "w")
    proc = subprocess.Popen([args.exe], env=env, stdout=err, stderr=err)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"), flush=True)

    try:
        for _ in range(120):
            token = open(token_file).read().strip() if os.path.exists(token_file) else ""
            try:
                s = socket.create_connection(("127.0.0.1", args.port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        f = s.makefile("rw", encoding="utf-8")

        def ask(line):
            f.write(line + "\n")
            f.flush()
            return f.readline().strip()

        if ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("token refused")

        def state():
            time.sleep(0.1)
            for _ in range(300):
                head, _, labels = ask("STATE").partition(" undo=")
                d = dict(kv.split("=", 1) for kv in head.split()[1:])
                d["undo"], _, d["redo"] = labels.partition(" redo=")
                if d["busy"] == "0":
                    return d
                time.sleep(0.2)
            raise RuntimeError("still busy")

        def docs():
            head, *rows = [x.strip() for x in ask("DOCS").split(" | ")]
            d = dict(kv.split("=", 1) for kv in head.split()[1:])
            return d, rows

        def read_wav(path):
            with wave.open(path, "rb") as w:
                return w.getnchannels(), w.getframerate(), array.array("h", w.readframes(w.getnframes()))

        # 1. Open many: three good files and one that is not audio, one after the other.
        r = ask("OPEN_MANY " + " ".join(p(n) for n in ("a.wav", "junk.wav", "b.wav", "c.wav")))
        check("OPEN_MANY queues four paths", r == "OK queued=4", r)
        st = state()
        d, rows = docs()
        check("three documents are open (the failure did not stop the others)", d["count"] == "3", (d, rows))
        check("they are in the order given", [r.split()[-1] for r in rows] == ["a.wav", "b.wav", "c.wav"], rows)
        check("each has its own length", [int(r.split("frames=")[1].split()[0]) for r in rows] == [lens["a"], lens["b"], lens["c"]], rows)
        err_line = ask("LASTERROR")
        check("the failure is reported, naming the file", "junk.wav" in err_line, err_line)

        # 2. Append two files to the current document (c, the last opened).
        ask("SWITCH 0")
        st0 = state()
        check("document 0 is a", int(st0["frames"]) == lens["a"] and st0["steps"] == "1", st0)
        r = ask(f"APPEND current {p('b.wav')} {p('c.wav')}")
        check("APPEND current starts", r == "OK", r)
        st = state()
        want = lens["a"] + lens["b"] + lens["c"]
        check("the length is the sum", int(st["frames"]) == want, (st["frames"], want))
        check("it is one history step, labelled", st["steps"] == "2" and st["undo"].startswith("Append"), st)
        check("no new document", docs()[0]["count"] == "3")
        # The audio: save a copy and read it back; b starts exactly where a ends.
        out = p("appended.wav")
        r = ask(f"SAVE {out}")
        st = state()
        ch, rate, pcm = read_wav(out)
        ok = (ch, rate, len(pcm) // 2) == (2, 48000, want)
        _, _, a_pcm = read_wav(p("a.wav"))
        _, _, b_pcm = read_wav(p("b.wav"))
        _, _, c_pcm = read_wav(p("c.wav"))
        check("the saved file is a, b, c back to back", ok and pcm[: len(a_pcm)] == a_pcm[: len(a_pcm)] and
              pcm[len(a_pcm): len(a_pcm) + len(b_pcm)] == b_pcm and pcm[len(a_pcm) + len(b_pcm):] == c_pcm, (ch, rate, len(pcm)))
        # Undo / redo.
        ask("ACTION edit.undo")
        st = state()
        check("one undo gives the old length back", int(st["frames"]) == lens["a"], st)
        ask("ACTION edit.redo")
        st = state()
        check("redo brings it all back", int(st["frames"]) == want, st)

        # 3. Refusals: another sample rate; a mono file is fine (duplicated).
        steps = st["steps"]
        r = ask(f"APPEND current {p('d.wav')}")
        st = state()
        err_line = ask("LASTERROR")
        check("a 44.1 kHz file is refused with a message", "44100" in err_line and "48000" in err_line, err_line)
        check("the refusal adds no step", st["steps"] == steps and int(st["frames"]) == want, st)
        r = ask(f"APPEND current {p('e.wav')} {p('d.wav')}")
        st = state()
        check("a mono file is added to stereo, the refused one skipped", int(st["frames"]) == want + lens["e"], (st["frames"], want + lens["e"]))
        check("that is one more step", int(st["steps"]) == int(steps) + 1, st)

        # 4. Append to a new document.
        r = ask(f"APPEND new {p('c.wav')} {p('d.wav')} {p('b.wav')} {p('e.wav')}")
        check("APPEND new starts", r == "OK", r)
        st = state()
        d, rows = docs()
        check("a fourth document was made and is current", d["count"] == "4" and d["current"] == "3", d)
        new_want = lens["c"] + lens["b"] + lens["e"]
        check("it holds c, b, e (the 44.1 kHz file refused)", int(st["frames"]) == new_want and st["channels"] == "2", (st["frames"], new_want))
        check("it is untitled and unsaved", "unsaved=1" in rows[3], rows)
        err_line = ask("LASTERROR")
        check("the refusal is reported", "d.wav" in err_line, err_line)
        r = ask("APPEND current")
        check("APPEND with no files is an error", r.startswith("ERR"), r)
        r = ask("APPEND sideways x.wav")
        check("APPEND with a bad target is an error", r.startswith("ERR"), r)

        for _ in range(4):
            ask("CLOSE discard")
        r = ask("QUIT")
        check("QUIT", r == "OK", r)
        proc.wait(timeout=30)
    finally:
        if proc.poll() is None:
            proc.kill()
    log_files = [os.path.join(d, n) for d, _, fs in os.walk(logs) for n in fs if n == "wavcraft.log"]
    errors = [l.strip()[:200] for p_ in log_files for l in open(p_, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR line", bool(log_files) and not errors, errors[:3] or "no log file")
    if not all(ok for _, ok, _ in checks) and log_files:
        print("--- log tail ---\n" + "".join(open(log_files[0], errors="replace").readlines()[-15:]))
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    bad = [c for c in checks if not c[1]]
    print(f"{len(checks) - len(bad)}/{len(checks)} checks passed")
    sys.exit(1 if bad else 0)


main()
