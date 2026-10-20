#!/usr/bin/env python3
"""f30 scripted run: Close All, Save All, Copy to New, Paste to New and New Audio File.

    python3 tools/alpha/f30.py --exe <wavcraft> [--port 47130] [--scratch DIR]

Opens one synthetic stereo 16-bit file, makes untitled documents with NEWDOC, Copy to New and
Paste to New, checks their format and audio (by saving them and reading the files), that untitled
documents are never discarded silently (CLOSE, Close All and QUIT refuse), that Save All saves
the documents that have a file and skips the untitled ones, then closes everything and QUITs.
Fails on an ERROR line in the app's log. HOME, the session folder, the log folder and the token
file are all inside a fresh scratch folder, removed at the end.
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time

RATE = 48000


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47130)
    ap.add_argument("--scratch", default=os.path.expanduser("~/dev/craft/_wt/scratch"))
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(args.scratch, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f30.", dir=args.scratch)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    token_file = os.path.join(work, "control.token")
    files = {}
    for name, freq, secs, ch in (("a", 220, 3, 2),):
        files[name] = os.path.join(work, f"{name}.wav")
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", f"sine=frequency={freq}:sample_rate={RATE}:duration={secs}",
                        "-ac", str(ch), "-c:a", "pcm_s16le", files[name]], check=True)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"), RUST_BACKTRACE="1",
               WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token_file)
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([args.exe, files["a"]], env=env, stdout=err, stderr=err)
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
            while True:
                head, _, labels = ask("STATE").partition(" undo=")
                d = dict(kv.split("=", 1) for kv in head.split()[1:])
                d["undo"], _, d["redo"] = labels.partition(" redo=")
                if d["busy"] == "0":
                    return d
                time.sleep(0.2)

        def docs():
            head, *rows = [x.strip() for x in ask("DOCS").split(" | ")]
            d = dict(kv.split("=", 1) for kv in head.split()[1:])
            return d, rows

        import wave, array

        def read_wav(path):
            with wave.open(path, "rb") as w:
                return w.getnchannels(), w.getsampwidth(), w.getframerate(), array.array("h", w.readframes(w.getnframes()))

        def wait_docs(pred, what):
            for _ in range(100):
                d, rows = docs()
                st = state()
                if pred(d, rows):
                    return d, rows
                time.sleep(0.2)
            raise RuntimeError("timed out: " + what)

        A = 3 * RATE
        st = state()
        check("the file opens", int(st["frames"]) == A and st["channels"] == "2", st)
        check("Paste to New with an empty clipboard is an error", ask("ACTION edit.paste_to_new").startswith("ERR"))
        check("Save All with nothing unsaved is fine", ask("ACTION file.save_all").startswith("OK"))

        # New > Audio File.
        r = ask("NEWDOC rate=44100 ch=1 bits=16")
        check("NEWDOC makes a second document", r == "OK count=2 current=1", r)
        st = state()
        check("it is empty, mono, 44100, unsaved", (st["frames"], st["channels"], st["rate"], st["unsaved"]) == ("0", "1", "44100", "1"), st)
        r = ask("NEWDOC")
        st = state()
        check("NEWDOC defaults to 48 kHz stereo", r == "OK count=3 current=2" and st["rate"] == "48000" and st["channels"] == "2", (r, st))
        check("NEWDOC refuses a bad rate, channels, bit depth and key",
              all(ask(c).startswith("ERR") for c in ("NEWDOC rate=0", "NEWDOC ch=0", "NEWDOC bits=20", "NEWDOC colour=red")) and docs()[0]["count"] == "3")
        ask("SWITCH 2")
        check("Copy to New on an empty document is an error", ask("ACTION edit.copy_to_new").startswith("ERR"))

        # Copy to New: a selection, then the whole file.
        ask("SWITCH 0")
        ask("SELECT 1000 49000")
        r = ask("ACTION edit.copy_to_new")
        check("Copy to New is accepted", r.startswith("OK"), r)
        d, rows = wait_docs(lambda d, rows: d["count"] == "4", "copy to new")
        st = state()
        check("the copy is a new current document with the selection's length", d["current"] == "3" and int(st["frames"]) == 48000 and st["channels"] == "2" and st["rate"] == str(RATE), (d, st))
        check("it is unsaved and untitled", st["unsaved"] == "1" and rows[3].endswith("Untitled"), rows)
        check("the original is untouched", rows[0].split()[1] == "unsaved=0" and f"frames={A}" in rows[0], rows)
        p_sel = os.path.join(work, "copy-selection.wav")
        ask(f"SAVE {p_sel}")
        state()
        ch, width, rate, data = read_wav(p_sel)
        ch0, w0, _, orig = read_wav(files["a"])
        want = orig[1000 * 2:49000 * 2]
        check("saved copy: 16-bit stereo like the source, identical samples", (ch, width, rate) == (2, 2, RATE) and data == want, (ch, width, rate, len(data), len(want)))
        check("saving gave it its name", docs()[1][3].endswith("copy-selection.wav") and docs()[1][3].split()[1] == "unsaved=0", docs()[1])

        ask("SWITCH 0")
        ask("SELECT 5 5")
        ask("ACTION edit.copy_to_new")
        wait_docs(lambda d, rows: d["count"] == "5", "whole-file copy")
        st = state()
        check("Copy to New with no selection takes the whole file", int(st["frames"]) == A, st)

        # One channel only.
        ask("SWITCH 0")
        ask("ACTION edit.leftChannel")
        ask("SELECT 0 24000")
        ask("ACTION edit.copy_to_new")
        wait_docs(lambda d, rows: d["count"] == "6", "mono copy")
        st = state()
        check("with one channel active the copy is mono", st["channels"] == "1" and int(st["frames"]) == 24000, st)
        ask("SWITCH 0")
        ask("ACTION edit.leftChannel")

        # Paste to New.
        ask("SELECT 2000 12000")
        ask("ACTION edit.copy")
        r = ask("ACTION edit.paste_to_new")
        check("Paste to New is accepted", r.startswith("OK"), r)
        wait_docs(lambda d, rows: d["count"] == "7", "paste to new")
        st = state()
        check("the clipboard became a new document of its length", int(st["frames"]) == 10000 and st["unsaved"] == "1" and st["channels"] == "2", st)
        p_paste = os.path.join(work, "paste.wav")
        ask(f"SAVE {p_paste}")
        state()
        ch, width, rate, data = read_wav(p_paste)
        check("pasted document holds the copied samples", data == orig[2000 * 2:12000 * 2], (len(data),))
        ask("SWITCH 0")
        check("the clipboard is still there after switching", ask("ACTION edit.paste_to_new").startswith("OK"))
        wait_docs(lambda d, rows: d["count"] == "8", "second paste to new")

        # Nothing untitled is discarded silently.
        d, rows = docs()
        check("the untitled documents that were not saved are unsaved", [r_.split()[1] for r_ in rows].count("unsaved=1") == 5, rows)
        ask("SWITCH 2")
        check("CLOSE refuses on an untitled document", ask("CLOSE").startswith("ERR unsaved"))
        r = ask("ACTION file.close_all")
        check("Close All refuses while documents are unsaved", r.startswith("ERR") and docs()[0]["count"] == "8", r)
        check("QUIT refuses while an untitled document is open", ask("QUIT").startswith("ERR unsaved"))

        # Save All: the document with a file is saved, the untitled ones are skipped.
        ask("SWITCH 0")
        ask("SELECT 0 1000")
        ask("ACTION edit.delete")
        check("an edit makes the first document unsaved", docs()[1][0].split()[1] == "unsaved=1", docs()[1][0])
        ask("SWITCH 3")
        r = ask("ACTION file.save_all")
        check("Save All is accepted", r.startswith("OK"), r)
        d, rows = wait_docs(lambda d, rows: rows[0].split()[1] == "unsaved=0", "save all")
        state()
        d, rows = docs()
        flags = [r_.split()[1] for r_ in rows]
        check("Save All saved the document with a file and left the untitled ones", flags[0] == "unsaved=0" and flags.count("unsaved=1") == 5, flags)
        check("and returned to the document it started in", d["current"] == "3", d)
        _, _, _, saved_a = read_wav(files["a"])
        check("the file on disk has the edit", len(saved_a) == (A - 1000) * 2, len(saved_a))

        # Save the untitled non-empty ones, keep the empties.
        for i in range(8):
            d, rows = docs()
            if "unsaved=1" in rows[i] and "frames=0" not in rows[i]:
                ask(f"SWITCH {i}")
                ask(f"SAVE {os.path.join(work, f'doc{i}.wav')}")
                state()
        d, rows = docs()
        flags = [(r_.split()[1], r_.split()[2]) for r_ in rows]
        left = [i for i, f_ in enumerate(flags) if f_[0] == "unsaved=1"]
        check("only the two empty documents remain unsaved", left == [1, 2] or len(left) == 2 and all(flags[i][1] == "frames=0" for i in left), flags)
        ask(f"SWITCH {left[0]}")
        ask(f"SAVE {os.path.join(work, 'empty.wav')}")
        state()
        check("an empty document cannot be saved and stays unsaved", docs()[1][left[0]].split()[1] == "unsaved=1" and not os.path.exists(os.path.join(work, "empty.wav")))
        r = ask("ACTION file.close_all")
        check("Close All still refuses (the empties are unsaved)", r.startswith("ERR") and docs()[0]["count"] == "8", r)
        check("QUIT still refuses", ask("QUIT").startswith("ERR unsaved"))
        for i in reversed(left):
            ask(f"SWITCH {i}")
            check(f"CLOSE discard closes empty document {i}", ask("CLOSE discard").startswith("OK"))
        d, rows = docs()
        check("six saved documents remain", d["count"] == "6" and all("unsaved=0" in r_ for r_ in rows), (d, rows))
        r = ask("ACTION file.close_all")
        check("Close All closes them all", r.startswith("OK") and docs()[0]["count"] == "0", r)
        check("QUIT closes", ask("QUIT") == "OK")
        try:
            code = proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            code = None
        check("the app exits", code == 0, code)
        left = [d for d in os.listdir(sessions)] if os.path.isdir(sessions) else []
        check("the session folders are removed", not left, left)
    except Exception as e:  # noqa: BLE001
        check("the run completed", False, repr(e))
    finally:
        if proc.poll() is None:
            proc.kill()
    log_files = [os.path.join(d, n) for d, _, fs in os.walk(logs) for n in fs if n == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR line", bool(log_files) and not errors, errors[:3] or "no log file")
    if not all(ok for _, ok, _ in checks) and log_files:
        print("--- log tail ---\n" + "".join(open(log_files[0], errors="replace").readlines()[-15:]))
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    bad = [c for c in checks if not c[1]]
    print(f"{len(checks) - len(bad)}/{len(checks)} checks passed")
    sys.exit(1 if bad else 0)


main()
