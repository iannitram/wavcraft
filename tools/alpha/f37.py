#!/usr/bin/env python3
"""f37 scripted run: Amplitude Statistics through the control channel on a synthetic file.

    python3 tools/alpha/f37.py --exe <path to the wavcraft binary>

A 4 s stereo 16-bit 48 kHz file is generated: left has a DC offset, a sine and two full-scale
runs (20 samples at frame 100000, 5 at 150000); right is a quiet sine with no DC. The expected
numbers are computed here from the same integers. Checks STATS (whole file, a range, the
selection), then ACTION stats.goto_peak / next_clip / prev_clip move the cursor, and the app's
log has no ERROR line. Scratch folder is removed at the end. Exit 0 on pass.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, FRAMES = 48000, 192000
RUN1, LEN1, RUN2, LEN2 = 100000, 20, 150000, 5


def make_samples():
    left, right = [], []
    for n in range(FRAMES):
        left.append(round(3277 + 13107 * math.sin(2 * math.pi * n / 480)))
        right.append(round(6554 * math.sin(2 * math.pi * n / 300)))
    for s, ln in ((RUN1, LEN1), (RUN2, LEN2)):
        for i in range(ln):
            left[s + i] = 32767
    return left, right


def write_wav(path, left, right):
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<hh", a, b) for a, b in zip(left, right)))


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

    def ask(self, line):
        self.f.write(line + "\n"); self.f.flush()
        return self.f.readline().strip()

    def wait_open(self):
        for _ in range(240):
            st = dict(kv.split("=", 1) for kv in self.ask("STATE").split()[1:] if "=" in kv)
            if st.get("busy") == "0" and int(st.get("frames", 0)) > 0:
                return st
            time.sleep(0.25)
        raise RuntimeError("the file did not open")


def parse(answer):
    """'OK range=.. | ch0 k=v ... | ch1 ...' -> (header dict, [channel dicts])"""
    parts = [p.strip() for p in answer.split("|")]
    kv = lambda s: dict(x.split("=", 1) for x in s.split() if "=" in x)
    return kv(parts[0]), [kv(p) for p in parts[1:]]


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--exe", required=True); ap.add_argument("--keep", action="store_true")
    exe = ap.parse_args().exe
    scratch_root = os.environ.get("WAVCRAFT_SCRATCH") or tempfile.gettempdir()
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f37.", dir=scratch_root)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"))

    proc, logs = None, os.path.join(work, "logs")
    try:
        home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
        os.makedirs(home)
        left, right = make_samples()
        wav = os.path.join(work, "stats-kit.wav")
        write_wav(wav, left, right)
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0)); port = s.getsockname()[1]
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
                   WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
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
    try:
        st = c.wait_open()
        check("the synthetic file opens", int(st["frames"]) == FRAMES, st)

        hdr, ch = parse(c.ask("STATS"))
        check("STATS answers for both channels", len(ch) == 2, ch)
        L, R = ch[0], ch[1]
        check("range is the whole file", hdr.get("range") == f"0-{FRAMES}", hdr)
        check("left peak is 32767/32768 at the first clipped frame", abs(float(L["peak"]) - 32767 / 32768) < 1e-6 and int(L["peak_frame"]) == RUN1, L)
        check("left peak dBFS ~ -0.00027", abs(float(L["peak_db"]) + 0.000265) < 1e-3, L["peak_db"])
        check("left max/min", abs(float(L["max"]) - 32767 / 32768) < 1e-6 and abs(float(L["min"]) - min(left) / 32768) < 1e-6, L)
        dc = sum(left) / FRAMES / 32768 * 100
        check("left DC offset %", abs(float(L["dc_pct"]) - dc) < 1e-3, f"{L['dc_pct']} vs {dc:.4f}")
        check("right DC offset ~ 0", abs(float(R["dc_pct"])) < 0.01, R["dc_pct"])
        check("left clipped runs = 2, samples = 25", L["clip_runs"] == "2" and L["clip_samples"] == str(LEN1 + LEN2), L)
        check("right has no clipped runs", R["clip_runs"] == "0", R)
        tot = math.sqrt(sum((x / 32768) ** 2 for x in left) / FRAMES)
        check("left total RMS dB", abs(float(L["rms_total_db"]) - 20 * math.log10(tot)) < 6e-3, f"{L['rms_total_db']} vs {20 * math.log10(tot):.4f}")
        rtot = math.sqrt(sum((x / 32768) ** 2 for x in right) / FRAMES)
        check("right total RMS dB", abs(float(R["rms_total_db"]) - 20 * math.log10(rtot)) < 6e-3, R["rms_total_db"])
        check("right peak", abs(float(R["peak"]) - max(abs(x) for x in right) / 32768) < 1e-6, R["peak"])
        want = lambda xs: 16 - ((lambda o: (o & -o).bit_length() - 1)(__import__("functools").reduce(lambda a, b: a | abs(b), xs, 0)))
        check("detected bit depth is the smallest step present", L["bits"] == str(want(left)) and R["bits"] == str(want(right)), (L["bits"], R["bits"], want(left), want(right)))

        # Navigation.
        c.ask("SELECT 0 0")
        a = c.ask("ACTION stats.goto_peak")
        check("goto_peak puts the cursor on the peak", a.startswith("OK") and f"selection={RUN1}-{RUN1}" in a, a)
        a = c.ask("ACTION stats.next_clip")
        check("next_clip moves to the second run", f"selection={RUN2}-{RUN2}" in a, a)
        a = c.ask("ACTION stats.next_clip")
        check("next_clip at the last run answers ERR and stays", a.startswith("ERR") and f"selection={RUN2}" in c.ask("STATE"), a)
        a = c.ask("ACTION stats.prev_clip")
        check("prev_clip goes back to the first run", f"selection={RUN1}-{RUN1}" in a, a)
        a = c.ask("ACTION stats.prev_clip")
        check("prev_clip before the first run answers ERR", a.startswith("ERR"), a)

        # A range with no clipping, and the selection.
        hdr, ch = parse(c.ask(f"STATS 0 {RUN1 - 1}"))
        check("a range before the run has no clipped runs and a lower peak", ch[0]["clip_runs"] == "0" and float(ch[0]["peak"]) < 0.6 and hdr["range"] == f"0-{RUN1 - 1}", (hdr, ch[0]))
        c.ask(f"SELECT {RUN1 - 10} {RUN1 + 10}")
        hdr, ch = parse(c.ask("STATS"))
        check("STATS without numbers measures the selection and finds the run start", hdr["range"] == f"{RUN1 - 10}-{RUN1 + 10}" and ch[0]["clip_runs"] == "1" and int(ch[0]["peak_frame"]) == RUN1, (hdr, ch[0]))
        check("a bad STATS is refused", c.ask("STATS 5").startswith("ERR"))
        c.ask(f"SELECT 0 {FRAMES}")
        c.ask("ACTION edit.silence")
        a = c.ask("ACTION stats.goto_peak")
        check("after an edit the old scan is refused (scan again)", a.startswith("ERR") and "scan" in a, a)
        hdr, ch = parse(c.ask("STATS"))
        check("the new scan sees silence", float(ch[0]["peak"]) == 0.0 and ch[0]["bits"] == "0", ch[0])
        check("QUIT discard", c.ask("QUIT discard") == "OK")
        proc.wait(20)
    except Exception as e:
        check("scripted run completed", False, f"{type(e).__name__}: {e}")
        if proc and proc.poll() is None:
            proc.kill()
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR", bool(log_files) and not errors, errors[:3] or "no log")
    ok = all(x[1] for x in checks)
    print("RESULT", "PASS" if ok else "FAIL", f"{sum(x[1] for x in checks)}/{len(checks)}")
    if not ok:
        print("logs:", [open(p, errors="replace").read()[-1500:] for p in log_files])
    if not ap_keep(): shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


def ap_keep():
    return "--keep" in sys.argv


if __name__ == "__main__":
    main()
