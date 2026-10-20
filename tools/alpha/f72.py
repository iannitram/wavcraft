#!/usr/bin/env python3
"""f72 scripted run: synchronise selection, zoom and playhead across open documents.

    python3 tools/alpha/f72.py --exe <wavcraft> [--port 47172] [--scratch DIR]

Opens two synthetic files at different sample rates (48 kHz, 3 s; 24 kHz, 2 s), sets a selection and
a view on the first, switches, and checks STATE: with each option off nothing is carried; with each
on, the selection / view / cursor arrive by time in seconds (converted between rates, clamped to
the shorter file). Then QUITs and fails on an ERROR line in the app's log. HOME, the session folder,
the log folder and the token file are all inside a fresh scratch folder, removed at the end.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave



def write_sine(path, freq, secs, rate):
    n = int(rate * secs)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * freq * i / rate))) for i in range(n)))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47172)
    ap.add_argument("--scratch", default=os.path.expanduser("~/dev/craft/_wt/scratch"))
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(args.scratch, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f72.", dir=args.scratch)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    token_file = os.path.join(work, "control.token")
    files = {}
    for name, freq, secs, rate in (("a", 220, 3, 48000), ("b", 440, 2, 24000)):
        files[name] = os.path.join(work, f"{name}.wav")
        write_sine(files[name], freq, secs, rate)
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
        check("A opens (48 kHz, 3 s)", st["rate"] == "48000" and int(st["frames"]) == 144000 and st["sync"] == "-", st)
        ask(f"OPEN {files['b']}")
        st = state()
        check("B opens (24 kHz, 2 s) as the second document", st["rate"] == "24000" and int(st["frames"]) == 48000 and docs()[0]["current"] == "1", st)

        def view(st):
            start, _, fpp = st["view"].partition("+")
            return float(start), float(fpp)

        # Put B at a known place, then A at a different one, with every option off.
        ask("SELECT 100 200")
        check("SWITCH 0", ask("SWITCH 0") == "OK current=0")
        check("VIEW on A", ask("VIEW 48000 20").startswith("OK view=48000.000+20."))
        check("SELECT on A (1.0 s .. 2.0 s)", ask("SELECT 48000 96000").endswith("selection=48000-96000"))
        ask("SWITCH 1")
        st = state()
        check("off: B keeps its own selection", st["selection"] == "100-200", st)
        ask("SWITCH 0")
        st = state()
        check("off: A keeps its selection and view", st["selection"] == "48000-96000" and view(st) == (48000.0, 20.0), st)

        # Selection sync.
        r = ask("ACTION view.sync.selection")
        check("view.sync.selection toggles on", r.startswith("OK") and state()["sync"] == "selection", r)
        ask("SWITCH 1")
        st = state()
        check("selection: 1..2 s at 48 kHz becomes 24000-48000 at 24 kHz (clamped to B's 2 s)", st["selection"] == "24000-48000", st)
        ask("SELECT 6000 9000")
        ask("SWITCH 0")
        st = state()
        check("selection: B's 6000-9000 (0.25..0.375 s) arrives on A as 12000-18000", st["selection"] == "12000-18000", st)

        # Selection beyond the other file's length is clamped.
        ask("SELECT 100000 140000")  # 2.083 s .. 2.917 s on A; B is 2 s long
        ask("SWITCH 1")
        st = state()
        check("clamped to the shorter file (B ends at 48000)", st["selection"] == "48000-48000", st)
        ask("ACTION view.sync.selection")

        # Zoom sync.
        r = ask("ACTION view.sync.zoom")
        check("view.sync.zoom toggles on (selection off)", r.startswith("OK") and state()["sync"] == "zoom", r)
        ask("SWITCH 0")
        ask("VIEW 48000 20")  # 1.0 s .. 1.0 + 20*width/48000 s
        ask("SWITCH 1")
        st = state()
        s, fpp = view(st)
        check("zoom: A's 1.0 s, 20 frames/px (48 kHz) -> B's 24000, 10 frames/px (24 kHz)", abs(s - 24000.0) < 1e-3 and abs(fpp - 10.0) < 1e-3, st)
        ask("VIEW 0 10")
        ask("SWITCH 0")
        st = state()
        s, fpp = view(st)
        check("zoom: B at 0 s, 10 frames/px (24 kHz) -> A at 0 s, 20 frames/px (48 kHz)", abs(s) < 1e-6 and abs(fpp - 20.0) < 1e-3, st)
        ask("VIEW 24000 40")
        ask("SWITCH 1")
        st = state()
        s, fpp = view(st)
        check("zoom: A's 0.5 s, 40 frames/px -> B's 12000, 20 frames/px", abs(fpp - 20.0) < 1e-3 and abs(s - 12000.0) < 1e-3, st)
        ask("ACTION view.sync.zoom")

        # Playhead sync: the cursor moves, the range does not follow.
        r = ask("ACTION view.sync.playhead")
        check("view.sync.playhead toggles on", r.startswith("OK") and state()["sync"] == "playhead", r)
        ask("SWITCH 0")
        ask("SELECT 72000 72000")  # cursor at 1.5 s on A
        ask("SWITCH 1")
        st = state()
        check("playhead: the cursor 1.5 s arrives on B as 36000", st["selection"] == "36000-36000", st)
        ask("SELECT 1000 2000")
        ask("SWITCH 0")
        st = state()
        check("playhead: B's range start (1000 -> 2000 on A at 48 kHz) becomes the cursor", st["selection"] == "2000-2000", st)

        # All three, then off again.
        ask("ACTION view.sync.selection")
        ask("ACTION view.sync.zoom")
        check("all three on", state()["sync"] == "selection,zoom,playhead")
        ask("SELECT 48000 72000")
        ask("VIEW 0 30")
        ask("SWITCH 1")
        st = state()
        check("all: selection 24000-36000, view 15 frames/px", st["selection"] == "24000-36000" and abs(view(st)[1] - 15.0) < 1e-3, st)
        for n in ("selection", "zoom", "playhead"):
            ask(f"ACTION view.sync.{n}")
        check("all off again", state()["sync"] == "-")
        check("a bad option name is an error", ask("ACTION view.sync.nothing").startswith("ERR"))
        settings = [os.path.join(d, n) for d, _, fs in os.walk(home) for n in fs if n == "settings.txt"]
        check("the options are kept in the settings file", settings and "sync_zoom = false" in open(settings[0]).read(), settings)
        check("QUIT closes", ask("QUIT discard") == "OK")
        try:
            code = proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            code = None
        check("the app exits", code == 0, code)
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
