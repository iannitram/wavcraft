#!/usr/bin/env python3
"""G3.5 scripted run: Match Loudness on a long kit file, saved and measured with ffmpeg.

    G35 needs ffmpeg on the PATH and a long kit WAV (make it with
    `cargo run --release -p wavcraft-testkit -- <folder> --long`, about 2 GB).

    python3 tools/alpha/g35.py --exe <release wavcraft binary> --wav <long kit WAV> [--keep]

The app opens the file, then the MATCH verb runs with its default (podcast) target:
-16 LUFS, -1 dBTP, limiter on. The control channel answers only when the whole run ends, so
the socket has no read timeout. Then SAVE writes a copy to the scratch folder, and ffmpeg's
ebur128 filter (peak=true) measures that copy. Checks: MATCH answers OK with changed=1; SAVE
answers OK; integrated loudness is in [-16.2, -15.8] LUFS; true peak is at or below -1.0 dBTP;
the app's log has no ERROR line. Nothing is written to the repo. The scratch folder is removed
unless --keep. Exit 0 on pass, 1 on a failed check, 2 on a missing input.
"""
import argparse, os, re, shutil, socket, subprocess, sys, tempfile, time

LUFS_LOW, LUFS_HIGH, TP_LIMIT = -16.2, -15.8, -1.0


class Control:
    def __init__(self, port, token):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        assert self.ask(f"TOKEN {token}") == "OK", "token refused"

    def ask(self, line, timeout=60):
        self.s.settimeout(timeout)
        self.f.write(line + "\n"); self.f.flush()
        return self.f.readline().strip()

    def wait_open(self):
        for _ in range(2400):
            st = dict(kv.split("=", 1) for kv in self.ask("STATE").split()[1:] if "=" in kv)
            if st.get("busy") == "0" and int(st.get("frames", 0)) > 0:
                return st
            time.sleep(0.25)
        raise RuntimeError("the file did not open within 10 minutes")


def measure(wav):
    """ffmpeg ebur128 with peak=true on the whole file; returns (integrated LUFS, true peak dBTP)."""
    run = subprocess.run(["ffmpeg", "-nostats", "-hide_banner", "-i", wav, "-af", "ebur128=peak=true",
                          "-f", "null", "-"], capture_output=True, text=True, errors="replace")
    text = run.stderr
    if "Summary:" not in text:
        raise RuntimeError("ffmpeg printed no ebur128 summary")
    summary = text[text.rfind("Summary:"):]
    i = re.search(r"Integrated loudness:\s*\n\s*I:\s*(-?[\d.]+)\s*LUFS", summary)
    p = re.search(r"True peak:\s*\n\s*Peak:\s*(-?[\d.]+)\s*dBFS", summary)
    if not i or not p:
        raise RuntimeError("could not read the ebur128 summary")
    return float(i.group(1)), float(p.group(1))


def ffmpeg_version():
    out = subprocess.run(["ffmpeg", "-version"], capture_output=True, text=True, errors="replace").stdout
    return out.splitlines()[0] if out else "unknown"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--wav", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    exe, wav = os.path.abspath(args.exe), os.path.abspath(args.wav)
    if not os.path.isfile(wav):
        print("FAIL --wav is not a file:", wav); sys.exit(2)
    if shutil.which("ffmpeg") is None:
        print("FAIL ffmpeg is not on the PATH"); sys.exit(2)

    scratch_root = os.environ.get("WAVCRAFT_SCRATCH") or tempfile.gettempdir()
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="g35.", dir=scratch_root)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"))

    proc = None
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    saved = os.path.join(work, "matched.wav")
    try:
        os.makedirs(home)
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0)); port = s.getsockname()[1]
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
                   WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token,
                   WAVCRAFT_NO_AUDIO="1", RUST_BACKTRACE="1")
        err = open(os.path.join(work, "stderr.log"), "w")
        proc = subprocess.Popen([exe, wav], env=env, stdout=err, stderr=err)
        for _ in range(120):
            if os.path.exists(token) and os.path.getsize(token) >= 64:
                break
            time.sleep(0.5)
        c = Control(port, open(token).read().strip())
    except Exception as e:
        print("FAIL could not start the app:", e)
        if proc and proc.poll() is None:
            proc.kill()
        shutil.rmtree(work, ignore_errors=True)
        sys.exit(1)

    match_answer = ""
    try:
        st = c.wait_open()
        check("the long file opens", int(st["frames"]) > 0, st)
        match_answer = c.ask("MATCH", timeout=None)
        check("MATCH answers OK with changed=1", match_answer.startswith("OK") and "changed=1" in match_answer, match_answer)
        save_answer = c.ask(f"SAVE {saved}", timeout=None)
        check("SAVE to the scratch folder answers OK", save_answer == "OK", save_answer)
        # SAVE answers when the write is queued; wait for the app to report busy=0 before reading the file.
        for _ in range(7200):
            st = dict(kv.split("=", 1) for kv in c.ask("STATE").split()[1:] if "=" in kv)
            if st.get("busy") == "0":
                break
            time.sleep(0.5)
        check("the save finishes (busy=0)", st.get("busy") == "0", st)
        check("QUIT discard", c.ask("QUIT discard") == "OK")
        proc.wait(10)
    except Exception as e:
        check("scripted run completed", False, f"{type(e).__name__}: {e}")
        if proc and proc.poll() is None:
            proc.kill()

    if os.path.isfile(saved):
        try:
            lufs, tp = measure(saved)
            print(f"ffmpeg_version={ffmpeg_version()}")
            print(f"integrated_lufs={lufs}")
            print(f"true_peak_dbtp={tp}")
            check(f"integrated loudness {LUFS_LOW} to {LUFS_HIGH} LUFS", LUFS_LOW <= lufs <= LUFS_HIGH, lufs)
            check(f"true peak at or below {TP_LIMIT} dBTP", tp <= TP_LIMIT, tp)
        except Exception as e:
            check("ffmpeg measured the saved file", False, f"{type(e).__name__}: {e}")
    else:
        check("the saved file exists", False, saved)

    print("MATCH:", match_answer)
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR", bool(log_files) and not errors, errors[:3] or "no log")
    ok = all(x[1] for x in checks)
    print("PASS" if ok else "FAIL", f"{sum(x[1] for x in checks)}/{len(checks)}")
    if args.keep:
        print("kept:", work)
    else:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
