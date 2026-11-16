#!/usr/bin/env python3
"""B1.4 frame-time run: a long 48 kHz stereo file, zoomed to 2 seconds, scrolled with NAV, then FRAMETIMES.

    python3 tools/alpha/b1-frames.py --exe <release wavcraft binary> [--port 47201] [--seconds 10800] [--keep]
    python3 tools/alpha/b1-frames.py --selftest

Makes a 16-bit 48 kHz stereo WAV in a scratch folder (WAVCRAFT_SCRATCH_ROOT, else the system temp folder),
starts the app on it with WAVCRAFT_NO_AUDIO=1 in a scratch home, drives it over the control channel and
prints one PASS or FAIL line per check, then the raw FRAMETIMES answer and one JSON summary line.
Exit status is 0 only if every check passed. The scratch folder is removed unless --keep is given.
"""
import argparse
import array
import json
import math
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import wave

RATE = 48000
BUDGET_MS = 16.7
MIN_COUNT = 300
ZOOM_FRAMES = 2 * RATE
SCROLL_SECONDS = 10
SCROLL_STEP = 1600
FRAME_PERIOD = 0.016
VIEW_TOLERANCE = 960
DEFAULT_PORT = 47201
DEFAULT_SECONDS = 3 * 60 * 60
TOKEN_TIMEOUT = 60
LOAD_TIMEOUT = 600
QUIT_TIMEOUT = 30
COUNT_LABEL = f"count is at least {MIN_COUNT}"
P95_LABEL = f"p95 work time is at most {BUDGET_MS} ms"


class RunError(Exception):
    """A step failed in a way that stops the run; it is reported as a FAIL line."""


def report(checks, label, ok, detail=""):
    ok = bool(ok)
    checks.append((label, ok))
    print(f"PASS {label}" if ok else f"FAIL {label}  -> {detail}", flush=True)


def make_wav(path, seconds):
    """Writes a 16-bit stereo WAV: 220 Hz left, 330 Hz right. One second is built once and repeated."""
    block = array.array("h")
    for i in range(RATE):
        block.append(int(16000 * math.sin(2 * math.pi * 220 * i / RATE)))
        block.append(int(16000 * math.sin(2 * math.pi * 330 * i / RATE)))
    if sys.byteorder == "big":
        block.byteswap()  # WAV samples are little-endian
    data = block.tobytes()
    with wave.open(path, "wb") as out:
        out.setnchannels(2)
        out.setsampwidth(2)
        out.setframerate(RATE)
        for _ in range(seconds):
            out.writeframes(data)


def _finite(value):
    return math.isfinite(value)


def parse_frametimes(line):
    """`OK n=<n> median_ms=<x> p95_ms=<x> worst_ms=<x> over=<n>` → a dict, or None if it is not that."""
    if not line.startswith("OK "):
        return None
    fields = {}
    for part in line.split()[1:]:
        key, sep, value = part.partition("=")
        if sep:
            fields[key] = value
    try:
        summary = {
            "n": int(fields["n"]),
            "median_ms": float(fields["median_ms"]),
            "p95_ms": float(fields["p95_ms"]),
            "worst_ms": float(fields["worst_ms"]),
            "over": int(fields["over"]),
        }
    except (KeyError, ValueError):
        return None
    if summary["n"] < 0 or summary["over"] < 0:
        return None
    if not all(_finite(summary[k]) for k in ("median_ms", "p95_ms", "worst_ms")):
        return None
    return summary


def judge(summary):
    """The two pass checks, as (label, ok, detail). Both limits are inclusive; None fails both."""
    if summary is None:
        return [(COUNT_LABEL, False, "no FRAMETIMES answer to read"),
                (P95_LABEL, False, "no FRAMETIMES answer to read")]
    return [
        (COUNT_LABEL, summary["n"] >= MIN_COUNT, f"n={summary['n']}"),
        (P95_LABEL, summary["p95_ms"] <= BUDGET_MS, f"p95_ms={summary['p95_ms']:.2f}"),
    ]


def parse_state(line):
    """STATE's `key=value` fields (the part before ` undo=`), tolerant of values with spaces."""
    fields = {}
    for part in line.partition(" undo=")[0].split()[1:]:
        key, sep, value = part.partition("=")
        if sep:
            fields[key] = value
    return fields


def parse_view(answer):
    """`OK view=<a>-<b>` → (a, b) as floats, or None."""
    if not answer.startswith("OK view="):
        return None
    head = answer.split()[1][len("view="):]
    first, sep, last = head.partition("-")
    try:
        a, b = float(first), float(last)
    except ValueError:
        return None
    if not sep or not (_finite(a) and _finite(b)) or b <= a:
        return None
    return a, b


class Control:
    def __init__(self, port, token):
        deadline = time.monotonic() + TOKEN_TIMEOUT
        while True:
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                if time.monotonic() > deadline:
                    raise RunError(f"no control channel on port {port}")
                time.sleep(0.5)
        self.file = self.sock.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RunError("the control channel refused the token")

    def ask(self, line):
        self.file.write(line + "\n")
        self.file.flush()
        return self.file.readline().strip()

    def close(self):
        for closer in (self.file.close, self.sock.close):
            try:
                closer()
            except OSError:
                pass


def selftest():
    checks = []

    def case(label, ok, detail=""):
        report(checks, label, ok, detail)

    def summary(n, p95):
        return {"n": n, "median_ms": 1.0, "p95_ms": p95, "worst_ms": 20.0, "over": 0}

    got = parse_frametimes("OK n=600 median_ms=2.10 p95_ms=6.40 worst_ms=18.00 over=3")
    case("parse_frametimes reads an OK line",
         got == {"n": 600, "median_ms": 2.1, "p95_ms": 6.4, "worst_ms": 18.0, "over": 3}, got)
    case("parse_frametimes refuses an ERR line", parse_frametimes("ERR FRAMETIMES [reset]") is None)
    case("parse_frametimes refuses a field that is not a number",
         parse_frametimes("OK n=abc median_ms=1 p95_ms=1 worst_ms=1 over=0") is None)
    case("parse_frametimes refuses a missing field", parse_frametimes("OK n=0") is None)

    at_limit = judge(summary(300, 16.7))
    case("judge passes both checks at the inclusive limits", all(ok for _, ok, _ in at_limit), at_limit)
    short = judge(summary(299, 16.7))
    case("judge fails the count check at n=299", short[0][1] is False and short[1][1] is True, short)
    slow = judge(summary(300, 16.71))
    case("judge fails the p95 check at p95_ms=16.71", slow[0][1] is True and slow[1][1] is False, slow)
    none = judge(None)
    case("judge fails both checks on None", not any(ok for _, ok, _ in none), none)

    passed = sum(1 for _, ok in checks if ok)
    print(json.dumps({"passed": passed, "total": len(checks), "ok": passed == len(checks)}))
    return 0 if passed == len(checks) else 1


def run(args, exe, work, checks, state):
    frames = args.seconds * RATE
    home = os.path.join(work, "home")
    os.makedirs(home)
    wav = os.path.join(work, "long.wav")
    make_wav(wav, args.seconds)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home,
               WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_LOG_DIR=os.path.join(work, "logs"),
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token, WAVCRAFT_NO_AUDIO="1")
    stderr_path = os.path.join(work, "stderr.log")
    with open(stderr_path, "w", encoding="utf-8") as stderr_log:
        proc = subprocess.Popen([exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=stderr_log)
        state["proc"] = proc
        secret = ""
        deadline = time.monotonic() + TOKEN_TIMEOUT
        while time.monotonic() < deadline and proc.poll() is None:
            if os.path.isfile(token):
                with open(token, encoding="utf-8") as f:
                    secret = f.read().strip()
                if secret:
                    break
            time.sleep(0.2)
        if not secret:
            raise RunError(f"the app did not write its control token within {TOKEN_TIMEOUT} s")
        control = Control(args.port, secret)
        state["control"] = control

        deadline = time.monotonic() + LOAD_TIMEOUT
        st = {}
        while True:
            st = parse_state(control.ask("STATE"))
            if st.get("busy") == "0" and st.get("frames") == str(frames):
                break
            if time.monotonic() > deadline:
                break
            time.sleep(0.5)
        opened = st.get("frames") == str(frames)
        report(checks, "the file opened with the expected frame count", opened,
               f"frames={st.get('frames')} busy={st.get('busy')} expected {frames}")
        if not opened or st.get("busy") != "0":
            raise RunError(f"the file did not finish loading in {LOAD_TIMEOUT} s")
        time.sleep(2.0)  # let the window lay out once, so the view has its real width

        mid = frames // 2
        answer = control.ask(f"NAV {mid} {mid + ZOOM_FRAMES}")
        view = parse_view(answer)
        zoomed = view is not None and abs((view[1] - view[0]) - ZOOM_FRAMES) <= VIEW_TOLERANCE
        report(checks, "NAV zoomed to a 2-second view", zoomed, answer)
        time.sleep(1.0)

        reset = control.ask("FRAMETIMES reset")
        report(checks, "FRAMETIMES reset answered OK n=0", reset == "OK n=0", reset)

        span_max = frames - ZOOM_FRAMES
        start = mid
        sent = 0
        bad = None
        end = time.monotonic() + SCROLL_SECONDS
        while time.monotonic() < end:
            t0 = time.monotonic()
            answer = control.ask(f"NAV {start} {start + ZOOM_FRAMES}")
            sent += 1
            if parse_view(answer) is None:
                bad = answer
                break
            start = (start + SCROLL_STEP) % span_max
            time.sleep(max(0.0, FRAME_PERIOD - (time.monotonic() - t0)))
        report(checks, "the scroll sent only OK answers", bad is None,
               f"answer to NAV call {sent}: {bad}")

        raw = control.ask("FRAMETIMES")
        print(raw, flush=True)
        for label, ok, detail in judge(parse_frametimes(raw)):
            report(checks, label, ok, detail)

        try:
            quit_answer = control.ask("QUIT")
        except OSError:
            quit_answer = "no answer"
        try:
            proc.wait(timeout=QUIT_TIMEOUT)
            closed = True
        except subprocess.TimeoutExpired:
            closed = False
        report(checks, "QUIT closed the app", closed,
               f"QUIT answered {quit_answer!r}; the app was still running after {QUIT_TIMEOUT} s")


def main():
    ap = argparse.ArgumentParser(description="B1.4 frame-time run (see the module docstring).")
    ap.add_argument("--exe", help="release wavcraft binary (required unless --selftest)")
    ap.add_argument("--port", type=int, default=DEFAULT_PORT, help="control port (default 47201)")
    ap.add_argument("--seconds", type=int, default=DEFAULT_SECONDS, help="file length in seconds (default 10800)")
    ap.add_argument("--keep", action="store_true", help="keep the scratch folder")
    ap.add_argument("--selftest", action="store_true", help="run the parsing and judging checks, no app")
    args = ap.parse_args()

    if args.selftest:
        return selftest()
    if not args.exe:
        ap.error("--exe <release wavcraft> is required")
    if not 1024 <= args.port <= 65535 or args.port == 47119:
        ap.error("--port must be 1024 to 65535, and not 47119 (the f19 port)")
    if args.seconds < 4:
        ap.error("--seconds must be at least 4 (the zoom is 2 seconds and the scroll needs room)")
    exe = os.path.abspath(args.exe)
    if not os.path.isfile(exe):
        ap.error(f"--exe is not a file: {args.exe}")

    checks = []
    state = {"proc": None, "control": None}
    work = None
    try:
        scratch_root = os.environ.get("WAVCRAFT_SCRATCH_ROOT") or tempfile.gettempdir()
        os.makedirs(scratch_root, exist_ok=True)
        work = tempfile.mkdtemp(prefix="b1-frames.", dir=scratch_root)
        run(args, exe, work, checks, state)
    except Exception as exc:  # any failure is reported, never hidden
        checks.append(("the run finished without an error", False))
        print(f"FAIL the run finished without an error  -> {type(exc).__name__}: {exc}", flush=True)
    finally:
        if state["control"] is not None:
            state["control"].close()
        proc = state["proc"]
        if proc is not None and proc.poll() is None:
            proc.kill()
            proc.wait()
        if work is not None:
            if args.keep:
                print(f"scratch folder kept: {work}", flush=True)
            else:
                shutil.rmtree(work, ignore_errors=True)

    passed = sum(1 for _, ok in checks if ok)
    total = len(checks)
    ok = total > 0 and passed == total
    print(json.dumps({"passed": passed, "total": total, "ok": ok}), flush=True)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
