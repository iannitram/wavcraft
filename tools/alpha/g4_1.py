#!/usr/bin/env python3
"""G4.1: STATS and LOUDNESS through the control channel, compared with ffmpeg on the kit files.

    python3 tools/alpha/g4_1.py --exe <release wavcraft> --kit <folder> [--long] [--port N] [--keep]

The kit comes from `wavcraft-testkit <folder>` (tone and pink); `--long` also needs `--long` there (about 2 GB).
For each file the app is started with the file as its argument, STATS and LOUDNESS are asked, and ffmpeg
measures the same file: `astats` for peak, RMS and DC per channel, `ebur128=peak=true` for integrated
loudness and true peak. The clipped count is asserted to be 0 (it is not compared with ffmpeg; the clipped
rule is covered by crates/core/tests/stats.rs). The app's log must have no ERROR line. Standard library and
the `ffmpeg` command only. Scratch folder is removed unless --keep. Exit 0 on pass, 1 otherwise.
"""
import argparse, os, re, shutil, socket, subprocess, sys, tempfile, time

# Tolerances. Each one is the agreement we expect between our numbers and ffmpeg's; do not widen them to pass.
PEAK_TOL_DB = 0.01        # sample peak: STATS peak_db vs astats "Peak level dB" (STATS prints 2 decimals)
RMS_TOL_DB = 0.05         # RMS: STATS rms_total_db vs astats "RMS level dB"
DC_TOL = 0.0001           # DC offset: STATS dc_pct / 100 vs astats "DC offset" (both as fraction of full scale)
TRUE_PEAK_TOL_DB = 0.3    # true peak: LOUDNESS true_peak vs ebur128 "True peak" (ffmpeg prints 1 decimal)
INTEGRATED_TOL_LU = 0.15  # integrated loudness: LOUDNESS integrated vs ebur128 "I" (ffmpeg prints 1 decimal)

OPEN_WAIT_S = 1800        # the 3-hour file takes a long time to open
ANSWER_TIMEOUT_S = 1800   # STATS and LOUDNESS on the 3-hour file

TONE = "tone-1k-m20-48k-stereo-16.wav"
PINK = "pink-48k-mono-24.wav"
LONG = "long-3h-48k-stereo-16.wav"


class Control:
    def __init__(self, port, token, timeout):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=timeout)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def wait_open(self):
        for _ in range(OPEN_WAIT_S * 4):
            st = dict(kv.split("=", 1) for kv in self.ask("STATE").split()[1:] if "=" in kv)
            if st.get("busy") == "0" and int(st.get("frames", 0)) > 0:
                return st
            time.sleep(0.25)
        raise RuntimeError("the file did not open")


def parse_stats(answer):
    """'OK range=.. | ch0 k=v ... | ch1 ...' -> (header dict, [channel dicts])"""
    if not answer.startswith("OK"):
        raise RuntimeError(f"STATS said: {answer}")
    parts = [p.strip() for p in answer.split("|")]
    kv = lambda s: dict(x.split("=", 1) for x in s.split() if "=" in x)
    return kv(parts[0]), [kv(p) for p in parts[1:]]


def parse_loudness(answer):
    if not answer.startswith("OK"):
        raise RuntimeError(f"LOUDNESS said: {answer}")
    return dict(x.split("=", 1) for x in answer.split()[1:] if "=" in x)


def ffmpeg_lines(path, af):
    r = subprocess.run(["ffmpeg", "-hide_banner", "-nostats", "-i", path, "-af", af, "-f", "null", "-"],
                       capture_output=True, text=True, errors="replace")
    if r.returncode != 0:
        raise RuntimeError(f"ffmpeg failed on {os.path.basename(path)}")
    out = []
    for line in r.stderr.splitlines():
        m = re.search(r"\] (.*)$", line)  # ffmpeg prefixes each message with "[Parsed_... @ ...] "
        out.append(m.group(1).strip() if m else line.strip())
    return out


def ffmpeg_astats(path):
    """{channel index (0-based): {'Peak level dB', 'RMS level dB', 'DC offset'}} from the per-channel blocks."""
    chans, cur = {}, None
    for msg in ffmpeg_lines(path, "astats=metadata=0"):
        if msg == "Overall":
            break
        m = re.match(r"Channel: (\d+)$", msg)
        if m:
            cur = int(m.group(1)) - 1
            chans[cur] = {}
            continue
        if cur is None:
            continue
        for key in ("Peak level dB", "RMS level dB", "DC offset"):
            m = re.match(rf"{key}: (\S+)$", msg)
            if m:
                chans[cur][key] = float(m.group(1))
    return chans


def ffmpeg_ebur128(path):
    """{'I': integrated LUFS, 'TP': true peak dBFS} from the Summary block at the end."""
    msgs = ffmpeg_lines(path, "ebur128=peak=true")
    start = next(i for i, m in enumerate(msgs) if m == "Summary:")
    res, section = {}, None
    for m in msgs[start + 1:]:
        if m.endswith(":") and not m.startswith(("I:", "Peak:")):
            section = m
        mi = re.match(r"I:\s+(\S+) LUFS$", m)
        if mi and section == "Integrated loudness:":
            res["I"] = float(mi.group(1))
        mp = re.match(r"Peak:\s+(\S+) dBFS$", m)
        if mp and section == "True peak:":
            res["TP"] = float(mp.group(1))
    if "I" not in res or "TP" not in res:
        raise RuntimeError("could not read the ebur128 Summary")
    return res


def near(ours, theirs, tol):
    if ours is None or theirs is None:
        return False
    return ours == theirs or abs(ours - theirs) <= tol


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def run_app(exe, wav, work, tag, port):
    """Start the app on `wav`, ask STATS and LOUDNESS, quit. Returns (stats answer, loudness answer, logs dir)."""
    d = os.path.join(work, tag)
    home, sessions, logs = (os.path.join(d, x) for x in ("home", "sessions", "logs"))
    os.makedirs(home)
    token = os.path.join(d, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token, WAVCRAFT_NO_AUDIO="1",
               RUST_BACKTRACE="1")
    with open(os.path.join(d, "stderr.log"), "w") as err:
        proc = subprocess.Popen([exe, wav], env=env, stdout=err, stderr=err)
        try:
            for _ in range(120):
                if os.path.exists(token) and os.path.getsize(token) >= 64:
                    break
                time.sleep(0.5)
            c = Control(port, open(token).read().strip(), ANSWER_TIMEOUT_S)
            c.wait_open()
            stats = c.ask("STATS")
            loud = c.ask("LOUDNESS")
            c.ask("QUIT discard")
            try:
                proc.wait(20)
            except subprocess.TimeoutExpired:
                proc.kill()
            return stats, loud, logs
        finally:
            if proc.poll() is None:
                proc.kill()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--kit", required=True, help="folder with the kit files (wavcraft-testkit OUT_DIR)")
    ap.add_argument("--long", action="store_true", help="also check the 3-hour file")
    ap.add_argument("--port", type=int, default=0, help="control port (default: a free port per run)")
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    exe = os.path.abspath(args.exe)

    files = [("tone", TONE, 2), ("pink", PINK, 1)]
    if args.long:
        files.append(("long", LONG, 2))
    else:
        print("SKIP long (no --long)")
    for _, name, _ in files:
        if not os.path.isfile(os.path.join(args.kit, name)):
            print(f"FAIL {name} missing")
            sys.exit(1)
    if shutil.which("ffmpeg") is None:
        print("FAIL ffmpeg is not installed")
        sys.exit(1)

    work = tempfile.mkdtemp(prefix="g4_1.")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok)))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"), flush=True)

    try:
        for tag, name, nch in files:
            wav = os.path.abspath(os.path.join(args.kit, name))
            port = args.port or free_port()
            try:
                stats_ans, loud_ans, logs = run_app(exe, wav, work, tag, port)
                hdr, chans = parse_stats(stats_ans)
                loud = parse_loudness(loud_ans)
            except Exception as e:  # the run itself failed: one failing check, then the next file
                check(f"{tag}: app run", False, f"{type(e).__name__}: {e}")
                continue
            ff_astats = ffmpeg_astats(wav)
            ff_ebu = ffmpeg_ebur128(wav)

            for k in range(nch):
                ours = chans[k] if k < len(chans) else {}
                ff = ff_astats.get(k, {})
                pk, pk_ff = (float(ours["peak_db"]) if "peak_db" in ours else None), ff.get("Peak level dB")
                check(f"{tag}: ch{k} peak", near(pk, pk_ff, PEAK_TOL_DB), f"{pk} vs {pk_ff}")
            for k in range(nch):
                ours = chans[k] if k < len(chans) else {}
                ff = ff_astats.get(k, {})
                rms = float(ours["rms_total_db"]) if "rms_total_db" in ours else None
                rms_ff = ff.get("RMS level dB")
                check(f"{tag}: ch{k} rms", near(rms, rms_ff, RMS_TOL_DB), f"{rms} vs {rms_ff}")
            for k in range(nch):
                ours = chans[k] if k < len(chans) else {}
                ff = ff_astats.get(k, {})
                dc = float(ours["dc_pct"]) / 100 if "dc_pct" in ours else None
                dc_ff = ff.get("DC offset")
                check(f"{tag}: ch{k} dc", near(dc, dc_ff, DC_TOL), f"{dc} vs {dc_ff}")
            for k in range(nch):
                ours = chans[k] if k < len(chans) else {}
                clipped = ours.get("clip_samples")
                check(f"{tag}: clipped ch{k} is 0", clipped == "0", f"clip_samples={clipped}")
            tp = float(loud["true_peak"]) if "true_peak" in loud else None
            check(f"{tag}: true peak", near(tp, ff_ebu["TP"], TRUE_PEAK_TOL_DB), f"{tp} vs {ff_ebu['TP']}")
            integ = float(loud["integrated"]) if "integrated" in loud else None
            check(f"{tag}: integrated", near(integ, ff_ebu["I"], INTEGRATED_TOL_LU), f"{integ} vs {ff_ebu['I']}")

            log_files = [os.path.join(dd, f) for dd, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
            errors = [ln.strip()[:200] for p in log_files for ln in open(p, errors="replace") if " ERROR " in ln]
            check(f"{tag}: app log has no ERROR", bool(log_files) and not errors, errors[:3] or "no log")
    finally:
        if args.keep:
            print("kept scratch folder:", work)
        else:
            shutil.rmtree(work, ignore_errors=True)

    passed = sum(1 for _, ok in checks if ok)
    ok_all = bool(checks) and passed == len(checks)
    print("RESULT", "PASS" if ok_all else "FAIL", f"{passed}/{len(checks)} checks")
    sys.exit(0 if ok_all else 1)


if __name__ == "__main__":
    main()
