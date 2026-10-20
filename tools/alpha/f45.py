#!/usr/bin/env python3
"""f45 scripted run: the Files panel, through the FILES control verb.

    python3 tools/alpha/f45.py --exe <wavcraft> [--port 47145] [--scratch DIR]

Opens three synthetic files (different lengths, rates, channels and bit depths), checks the
table's rows, sorts it by name, duration and rate in both directions, filters it, selects a row
(which makes that document current), checks the command Reveal would run (it is not run), then
closes one document. Fails on an ERROR line in the app's log. HOME, the session folder, the log
folder and the token file are all inside a fresh scratch folder, removed at the end.
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time

# name, frequency, seconds, rate, channels, sample codec
FILES = [("c_long.wav", 220, 3, 48000, 2, "pcm_s16le"), ("a_short.wav", 330, 1, 44100, 1, "pcm_s24le"), ("b_mid.wav", 440, 2, 48000, 2, "pcm_s16le")]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47145)
    ap.add_argument("--scratch", default=os.path.expanduser("~/dev/craft/_wt/scratch"))
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(args.scratch, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f45.", dir=args.scratch)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    token_file = os.path.join(work, "control.token")
    paths = []
    for name, freq, secs, rate, ch, codec in FILES:
        p = os.path.join(work, name)
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", f"sine=frequency={freq}:sample_rate={rate}:duration={secs}",
                        "-ac", str(ch), "-c:a", codec, p], check=True)
        paths.append(p)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"), RUST_BACKTRACE="1",
               WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token_file)
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([args.exe, paths[0]], env=env, stdout=err, stderr=err)
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

        def idle():
            for _ in range(200):
                if " busy=0" in ask("STATE"):
                    return
                time.sleep(0.2)
            raise RuntimeError("still busy")

        def files():
            head, *rows = [x.strip() for x in ask("FILES").split(" | ")]
            h = dict(kv.split("=", 1) for kv in head.split()[1:] if "=" in kv)
            out = []
            for r in rows:
                pre, _, rest = r.partition(" name=")
                name, _, path = rest.partition(" path=")
                d = dict(kv.split("=", 1) for kv in pre.split()[1:])
                d["name"], d["path"] = name, path
                out.append(d)
            return h, out

        def names():
            return [r["name"] for r in files()[1]]

        idle()
        for p in paths[1:]:
            check(f"OPEN {os.path.basename(p)}", ask(f"OPEN {p}") == "OK")
            idle()

        h, rows = files()
        check("the table lists three rows in the order opened", names() == ["c_long.wav", "a_short.wav", "b_mid.wav"] and h["count"] == "3" and h["total"] == "3", (h, names()))
        r0, r1 = rows[0], rows[1]
        check("a row has frames, rate, channels, bits, format and path",
              (r0["frames"], r0["rate"], r0["ch"], r0["bits"], r0["format"], r0["path"]) == ("144000", "48000", "2", "16", "Wav", paths[0])
              and (r1["frames"], r1["rate"], r1["ch"], r1["bits"]) == ("44100", "44100", "1", "24"), rows)
        check("the last opened file is current and nothing is unsaved", [r["current"] for r in rows] == ["0", "0", "1"] and all(r["unsaved"] == "0" for r in rows), rows)

        check("sort name asc", ask("FILES sort name asc").startswith("OK") and names() == ["a_short.wav", "b_mid.wav", "c_long.wav"], names())
        check("sort name desc", ask("FILES sort name desc").startswith("OK") and names() == ["c_long.wav", "b_mid.wav", "a_short.wav"], names())
        check("sort duration asc", ask("FILES sort duration asc").startswith("OK") and names() == ["a_short.wav", "b_mid.wav", "c_long.wav"], names())
        check("sort rate desc (ties keep the order opened)", ask("FILES sort rate desc").startswith("OK") and names() == ["c_long.wav", "b_mid.wav", "a_short.wav"], names())
        check("sort refuses a bad column or direction", ask("FILES sort colour asc").startswith("ERR") and ask("FILES sort name up").startswith("ERR"))
        check("sort none restores the order opened", ask("FILES sort none").startswith("OK") and names() == ["c_long.wav", "a_short.wav", "b_mid.wav"], names())

        ask("FILES sort name asc")
        h, rows = files()
        check("filter 'MID' shows one row", ask("FILES filter MID").startswith("OK") and names() == ["b_mid.wav"] and files()[0]["total"] == "3", (names(), files()[0]))
        check("filter matches the path too", ask("FILES filter " + os.path.basename(work)).startswith("OK") and len(names()) == 3, names())
        check("a filter with no match shows nothing", ask("FILES filter zzz").startswith("OK") and names() == [])
        check("select refuses a row the table does not show", ask("FILES select 0").startswith("ERR"))
        ask("FILES filter _")
        # Shown: a_short, b_mid, c_long (sorted by name). Row 0 is a_short, the second document.
        r = ask("FILES select 0")
        h, rows = files()
        check("select makes that row's document current", r == "OK current=1" and h["current"] == "1" and rows[0]["name"] == "a_short.wav" and rows[0]["current"] == "1", (r, h, rows))
        st = ask("STATE")
        check("STATE follows the selected document", "frames=44100 " in st and "channels=1 " in st and "rate=44100 " in st, st)

        r = ask("FILES reveal 0")
        want = {"darwin": f"open -R {paths[1]}", "win32": f"explorer /select,{paths[1]}"}.get(sys.platform, f"xdg-open {work}")
        check("Reveal reports the command it would run, without running it", r == f"OK would run: {want}", r)

        ask("FILES filter")
        ask("FILES sort none")
        r = ask("FILES select 2")
        check("select the third document", r == "OK current=2", r)
        check("Close closes it", ask("CLOSE") == "OK count=2")
        h, rows = files()
        check("two rows remain", names() == ["c_long.wav", "a_short.wav"] and h["total"] == "2", (h, names()))
        check("FILES select past the end is an error", ask("FILES select 5").startswith("ERR"))
        check("QUIT closes", ask("QUIT") == "OK")
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
