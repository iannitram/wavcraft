#!/usr/bin/env python3
"""B2.6 edit-to-picture refresh run: a long 48 kHz stereo file, three edits, and SPECTRAL's timer.

    python3 tools/alpha/b2-refresh.py --exe <release wavcraft binary> [--port 47202] [--seconds 10800] [--keep]

Makes a 16-bit 48 kHz stereo WAV in a scratch folder (WAVCRAFT_SCRATCH_ROOT, else the system temp
folder), starts the app on it with WAVCRAFT_NO_AUDIO=1 in a scratch home, shows the spectral
display and deletes one minute from the middle of the file. After the delete, the undo and the
redo it polls SPECTRAL until edit_to_picture_ms is a whole number and prints one
`edit_to_picture_ms=<n>` line for each. It prints PASS when all three numbers are at most 500 ms,
else FAIL with a short reason; the exit status matches. The scratch folder is removed when every
check passes, unless --keep is given.
"""
import argparse
import array
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
LIMIT_MS = 500
DEFAULT_PORT = 47202
DEFAULT_SECONDS = 3 * 60 * 60
TOKEN_TIMEOUT = 60
LOAD_TIMEOUT = 600
PICTURE_TIMEOUT = 120
QUIT_TIMEOUT = 30
FORBIDDEN_PORTS = (47119, 47201)


class RunError(Exception):
    """A step failed in a way that stops the run; it is reported as a FAIL line."""


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


def parse_state(line):
    """STATE's `key=value` fields (the part before ` undo=`), tolerant of values with spaces."""
    fields = {}
    for part in line.partition(" undo=")[0].split()[1:]:
        key, sep, value = part.partition("=")
        if sep:
            fields[key] = value
    return fields


def read_picture(answer):
    """`SPECTRAL`'s last field: `("number", n)`, `("pending", None)`, `("none", None)` or `("bad", None)`."""
    parts = answer.split()
    if not parts or not answer.startswith("OK "):
        return "bad", None
    key, sep, value = parts[-1].partition("=")
    if key != "edit_to_picture_ms" or not sep:
        return "bad", None
    if value == "pending":
        return "pending", None
    if value == "none":
        return "none", None
    try:
        number = int(value)
    except ValueError:
        return "bad", None
    if number < 0:
        return "bad", None
    return "number", number


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


def wait_for_number(control):
    """Poll SPECTRAL until edit_to_picture_ms is a whole number, within PICTURE_TIMEOUT."""
    deadline = time.monotonic() + PICTURE_TIMEOUT
    last = ""
    while time.monotonic() <= deadline:
        answer = control.ask("SPECTRAL")
        kind, number = read_picture(answer)
        if kind == "number":
            return number
        if kind == "none":
            raise RunError(f"SPECTRAL went back to none: {answer!r}")
        if kind == "bad":
            raise RunError(f"SPECTRAL answered {answer!r}")
        last = answer
        time.sleep(0.2)
    raise RunError(f"edit_to_picture_ms did not become a number within {PICTURE_TIMEOUT} s (last: {last!r})")


def edit(control, command, label, numbers):
    """Run one edit, wait for its picture time and print the number as the issue asks."""
    answer = control.ask(command)
    if not answer.startswith("OK "):
        raise RunError(f"{command} answered {answer!r}")
    number = wait_for_number(control)
    numbers.append((label, number))
    print(f"edit_to_picture_ms={number}", flush=True)


def run(args, exe, work, numbers, state):
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
        if st.get("frames") != str(frames) or st.get("busy") != "0":
            raise RunError(f"the file did not finish loading in {LOAD_TIMEOUT} s "
                           f"(frames={st.get('frames')} busy={st.get('busy')}, expected {frames})")

        answer = control.ask("SPECTRAL show")
        if not answer.startswith("OK "):
            raise RunError(f"SPECTRAL show answered {answer!r}")
        answer = control.ask("SPECTRAL")
        kind, _ = read_picture(answer)
        if kind != "none":
            raise RunError(f"edit_to_picture_ms was not none before any edit: {answer!r}")

        mid = frames // 2
        span = 30 * RATE
        answer = control.ask(f"SELECT {mid - span} {mid + span}")
        if not answer.startswith("OK "):
            raise RunError(f"SELECT answered {answer!r}")
        edit(control, "ACTION edit.delete", "delete", numbers)
        edit(control, "ACTION edit.undo", "undo", numbers)
        edit(control, "ACTION edit.redo", "redo", numbers)


def main():
    ap = argparse.ArgumentParser(description="B2.6 edit-to-picture refresh run (see the module docstring).")
    ap.add_argument("--exe", required=True, help="release wavcraft binary")
    ap.add_argument("--port", type=int, default=DEFAULT_PORT, help="control port (default 47202)")
    ap.add_argument("--seconds", type=int, default=DEFAULT_SECONDS, help="file length in seconds (default 10800)")
    ap.add_argument("--keep", action="store_true", help="keep the scratch folder")
    args = ap.parse_args()

    if not 1024 <= args.port <= 65535 or args.port in FORBIDDEN_PORTS:
        ap.error("--port must be 1024 to 65535, and not 47119 or 47201")
    if args.seconds < 120:
        ap.error("--seconds must be at least 120 (the edit deletes one minute from the middle)")
    exe = os.path.abspath(args.exe)
    if not os.path.isfile(exe):
        ap.error(f"--exe is not a file: {args.exe}")

    numbers = []
    state = {"proc": None, "control": None}
    work = None
    failure = None
    ok = False
    try:
        scratch_root = os.environ.get("WAVCRAFT_SCRATCH_ROOT") or tempfile.gettempdir()
        os.makedirs(scratch_root, exist_ok=True)
        work = tempfile.mkdtemp(prefix="b2-refresh.", dir=scratch_root)
        run(args, exe, work, numbers, state)
        ok = len(numbers) == 3 and all(number <= LIMIT_MS for _, number in numbers)
    except Exception as exc:  # any failure is reported, never hidden
        failure = f"{type(exc).__name__}: {exc}"
    finally:
        control = state["control"]
        if control is not None:
            try:
                control.ask("QUIT discard")
            except OSError:
                pass
            control.close()
        proc = state["proc"]
        if proc is not None:
            try:
                proc.wait(timeout=QUIT_TIMEOUT)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        if work is not None:
            if ok and not args.keep:
                shutil.rmtree(work, ignore_errors=True)
            else:
                print(f"scratch folder kept: {work}", flush=True)

    if failure is not None:
        print(f"FAIL {failure}", flush=True)
        return 1
    if len(numbers) != 3:
        print(f"FAIL only {len(numbers)} of 3 edits answered a number", flush=True)
        return 1
    over = [(label, number) for label, number in numbers if number > LIMIT_MS]
    if over:
        details = ", ".join(f"{label} {number} ms" for label, number in over)
        print(f"FAIL edit_to_picture_ms over {LIMIT_MS} ms: {details}", flush=True)
        return 1
    print("PASS", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
