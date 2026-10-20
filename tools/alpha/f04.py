#!/usr/bin/env python3
"""f04 scripted run: several open documents through the control channel.

    python3 tools/alpha/f04.py --exe <wavcraft> [--port 47104] [--scratch DIR]

Opens three synthetic files in one window, edits two of them, switches between them and checks
that each keeps its own history, selection and unsaved mark; that Close and Quit refuse while any
document has unsaved changes (also one that is not the current document); that Close picks a
neighbour; then QUITs and fails on an ERROR line in the app's log. HOME, the session folder, the
log folder and the token file are all inside a fresh scratch folder, removed at the end.
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time

RATE = 48000


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47104)
    ap.add_argument("--scratch", default=os.path.expanduser("~/dev/craft/_wt/scratch"))
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(args.scratch, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f04.", dir=args.scratch)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    token_file = os.path.join(work, "control.token")
    files = {}
    for name, freq, secs, ch in (("a", 220, 3, 2), ("b", 440, 2, 1), ("c", 880, 1, 2)):
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

        st = state()
        A, B, C = 3 * RATE, 2 * RATE, 1 * RATE
        check("A opens from the command line", int(st["frames"]) == A and st["channels"] == "2", st)
        check("DOCS lists one document", docs()[0] == {"count": "1", "current": "0"}, docs())
        ask(f"OPEN {files['b']}")
        st = state()
        d, rows = docs()
        check("File > Open adds a document (does not replace)", d == {"count": "2", "current": "1"} and len(rows) == 2, (d, rows))
        check("the new document is current (B: mono, 2 s)", int(st["frames"]) == B and st["channels"] == "1", st)
        check("DOCS names and sizes both", rows[0].endswith("a.wav") and f"frames={A}" in rows[0] and rows[1].endswith("b.wav") and f"frames={B}" in rows[1], rows)

        # Edit B, then A: each keeps its own history.
        ask("SELECT 0 24000")
        a = ask("ACTION edit.delete")
        st = state()
        check("Delete on B", a.endswith("changed=1") and int(st["frames"]) == B - 24000 and st["steps"] == "2" and st["unsaved"] == "1", (a, st))
        check("DOCS marks only B unsaved", "unsaved=0" in docs()[1][0] and "unsaved=1" in docs()[1][1], docs())
        check("SWITCH 0", ask("SWITCH 0") == "OK current=0")
        st = state()
        check("A has its own untouched history", int(st["frames"]) == A and st["steps"] == "1" and st["unsaved"] == "0" and st["selection"] == "0-0", st)
        ask("SELECT 1000 2000")
        ask("ACTION edit.silence")
        st = state()
        check("Silence on A", st["steps"] == "2" and st["undo"] == "Silence" and st["unsaved"] == "1" and int(st["frames"]) == A, st)
        ask("SWITCH 1")
        st = state()
        check("back on B: its length, steps and undo label", int(st["frames"]) == B - 24000 and st["steps"] == "2" and st["undo"] == "Delete", st)
        ask("SWITCH 0")
        st = state()
        check("A keeps its selection and history after the round trip", st["selection"] == "1000-2000" and st["steps"] == "2" and st["undo"] == "Silence", st)

        # Unsaved handling.
        check("CLOSE refuses with unsaved changes", ask("CLOSE").startswith("ERR unsaved"))
        check("QUIT refuses with unsaved changes", ask("QUIT").startswith("ERR unsaved"))
        ask("SWITCH 1")
        ask("ACTION edit.undo")
        st = state()
        check("B undone back to its saved step is not unsaved", st["unsaved"] == "0" and int(st["frames"]) == B, st)
        check("QUIT still refuses: A (not current) is unsaved", ask("QUIT").startswith("ERR unsaved"))
        check("CLOSE closes B without asking (saved); A takes its place", ask("CLOSE") == "OK count=1")
        st = state()
        check("A is current again with its edits", int(st["frames"]) == A and st["unsaved"] == "1" and st["steps"] == "2", st)
        check("CLOSE refuses on A", ask("CLOSE").startswith("ERR unsaved"))
        saved = os.path.join(work, "a-saved.wav")
        check("SAVE A", ask(f"SAVE {saved}") == "OK")
        st = state()
        check("saved: unsaved mark off", st["unsaved"] == "0" and os.path.exists(saved), st)

        # A third document, closed with discard; ordering and bad indexes.
        ask(f"OPEN {files['c']}")
        st = state()
        d, rows = docs()
        check("C opens as the second document", d == {"count": "2", "current": "1"} and int(st["frames"]) == C, (d, st))
        check("SWITCH out of range is an error", ask("SWITCH 5").startswith("ERR") and ask("SWITCH x").startswith("ERR") and docs()[0]["current"] == "1")
        ask("SELECT 0 100")
        ask("ACTION edit.delete")
        check("CLOSE refuses on C", ask("CLOSE").startswith("ERR unsaved"))
        check("CLOSE discard closes C", ask("CLOSE discard") == "OK count=1")
        st = state()
        check("A is current, saved, untouched by C", int(st["frames"]) == A and st["unsaved"] == "0" and st["steps"] == "2", st)
        check("CLOSE the last document", ask("CLOSE") == "OK count=0")
        st = state()
        check("nothing open", st["frames"] == "0" and docs()[0] == {"count": "0", "current": "0"}, st)
        check("CLOSE with nothing open is an error", ask("CLOSE").startswith("ERR"))
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
