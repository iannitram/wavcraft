#!/usr/bin/env python3
"""f95 scripted run: the Files panel channel expander (FILES expand / collapse / select <row>.<ch>).

    python3 tools/alpha/f95.py --exe <wavcraft> [--port 47195] [--scratch DIR]

Opens a stereo and a mono synthetic file, expands and collapses rows, checks the channel lines and
their labels, selects a channel (which makes that file current and edits only that channel), checks
the refusals. HOME, session, log and token file are all inside a fresh scratch folder, removed at the end.
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time

# name, frequency, seconds, rate, channels, sample codec
FILES = [("st.wav", 220, 1, 48000, 2, "pcm_s16le"), ("mono.wav", 330, 1, 44100, 1, "pcm_s16le")]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47195)
    ap.add_argument("--scratch", default=os.path.expanduser("~/dev/craft/_wt/scratch"))
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(args.scratch, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f95.", dir=args.scratch)
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
        check("OPEN mono.wav", ask(f"OPEN {paths[1]}") == "OK")
        idle()

        def lines():
            return [x.strip() for x in ask("FILES").split(" | ")][1:]

        L = lines()
        check("two file rows, none expanded", len(L) == 2 and all("expanded=0" in l for l in L), L)
        check("expand the stereo file", ask("FILES expand 0") == "OK expanded=1")
        L = lines()
        check("the stereo file shows Left and Right under it",
              len(L) == 4 and L[1].startswith("0.0 channel label=Left") and L[2].startswith("0.1 channel label=Right") and "expanded=1" in L[0], L)
        check("expand the mono file", ask("FILES expand 1") == "OK expanded=1")
        L = lines()
        check("the mono file shows one Mono line", len(L) == 5 and L[4].startswith("1.0 channel label=Mono"), L)
        check("both stereo channels start active", "active=1" in L[1] and "active=1" in L[2], L)
        r = ask("FILES select 0.1")
        check("select the right channel makes the file current", r == "OK current=0 channel=1", r)
        L = lines()
        check("only the right channel is active", "active=0" in L[1] and "active=1" in L[2], L)
        st = ask("STATE")
        check("STATE follows the selected file", "channels=2 " in st, st)
        check("select a channel the file lacks is refused", ask("FILES select 0.2").startswith("ERR"))
        check("select a file that is not there is refused", ask("FILES select 7.0").startswith("ERR") and ask("FILES expand 9").startswith("ERR"))
        check("collapse the stereo file", ask("FILES collapse 0") == "OK expanded=0" and len(lines()) == 3, lines())
        check("the plain select still works", ask("FILES select 1") == "OK current=1")
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
