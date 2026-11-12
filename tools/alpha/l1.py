#!/usr/bin/env python3
"""L1.11 transcription runner check; run against a release wavcraft binary.

    python3 tools/alpha/l1.py --exe <release build>
"""
import argparse
import os
import shutil
import socket
import struct
import subprocess
import tempfile
import time
import wave

PORT = 47196
RATE = 48_000


def make_wav(path, seconds):
    # One 16 384-frame chunk of a repeating ramp, written as many times as the length needs (a 30-minute
    # file is 86.4 million frames, too many to pack one sample at a time).
    chunk_frames = 16_384
    chunk = bytearray()
    for i in range(chunk_frames):
        value = int(5000 * ((i % 97) / 48 - 1))
        chunk.extend(struct.pack("<hh", value, value))
    total = seconds * RATE
    full, rest = divmod(total, chunk_frames)
    with wave.open(path, "wb") as out:
        out.setnchannels(2)
        out.setsampwidth(2)
        out.setframerate(RATE)
        for _ in range(full):
            out.writeframesraw(chunk)
        if rest:
            out.writeframesraw(bytes(chunk[: rest * 4]))


class Control:
    def __init__(self, token):
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            try:
                self.sock = socket.create_connection(("127.0.0.1", PORT), timeout=10)
                break
            except OSError:
                time.sleep(.1)
        else:
            raise RuntimeError("control channel did not start")
        self.file = self.sock.makefile("rw", encoding="utf-8")
        assert self.ask("TOKEN " + token) == "OK"

    def ask(self, line):
        self.file.write(line + "\n")
        self.file.flush()
        answer = self.file.readline().strip()
        if answer.startswith("ERR"):
            raise RuntimeError(answer)
        return answer

    def transcript_state(self):
        answer = self.ask("TRANSCRIPT state")
        return dict(field.split("=", 1) for field in answer.split()[1:])

    def steps(self):
        state = self.ask("STATE")
        fields = dict(field.split("=", 1) for field in state.split()[1:] if "=" in field)
        return int(fields["steps"])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", required=True)
    exe = os.path.abspath(parser.parse_args().exe)
    work = tempfile.mkdtemp(prefix="l1.")  # system temp, never the repo root; removed in finally
    proc = None
    try:
        good = os.path.join(work, "speech65.wav")
        long_file = os.path.join(work, "long.wav")
        make_wav(good, 65)
        make_wav(long_file, 30 * 60)
        home = os.path.join(work, "home")
        os.makedirs(home)
        token_path = os.path.join(work, "token")
        env = dict(
            os.environ,
            HOME=home,
            WAVCRAFT_CONTROL_PORT=str(PORT),
            WAVCRAFT_CONTROL_TOKEN_FILE=token_path,
            WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"),
            WAVCRAFT_NO_AUDIO="1",
            WAVCRAFT_FAKE_SPEECH="1",
        )
        proc = subprocess.Popen([exe, good], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.monotonic() + 60
        while not os.path.isfile(token_path) and time.monotonic() < deadline:
            time.sleep(.1)
        c = Control(open(token_path, encoding="utf-8").read().strip())
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            state = c.ask("STATE")
            if "frames=3120000 " in state and "busy=0" in state:
                break
            time.sleep(.1)

        initial_steps = c.steps()
        c.ask("ACTION transcript.run")
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            state = c.transcript_state()
            if state.get("running") == "0" and state.get("words") == "3" and state.get("first") == "48000-72000":
                break
            time.sleep(.05)
        assert state.get("running") == "0" and state.get("words") == "3" and state.get("first") == "48000-72000", state
        steps_after = c.steps()
        assert steps_after == initial_steps + 1, f"expected steps={initial_steps + 1}, got {steps_after}"
        print("PASS 65 s file transcribes to 3 words and steps up by 1", state)

        c.ask("OPEN " + long_file)
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            status = c.ask("STATE")
            if "frames=86400000 " in status and "busy=0" in status:
                break
            time.sleep(.1)

        initial_steps = c.steps()
        c.ask("ACTION transcript.run")
        cancelled_at = time.monotonic()
        c.ask("TRANSCRIPT cancel")
        deadline = cancelled_at + 1.0
        while time.monotonic() < deadline:
            state = c.transcript_state()
            if state.get("running") == "0" and state.get("words") == "0":
                break
            time.sleep(.02)
        assert state.get("running") == "0" and state.get("words") == "0", state
        assert time.monotonic() <= deadline, "cancel did not end within 1 s"
        steps_after = c.steps()
        assert steps_after == initial_steps, f"expected steps={initial_steps}, got {steps_after}"
        print("PASS 30-minute scan cancels within 1 s with words=0 and steps unchanged", state)

        c.ask("QUIT discard")
        proc.wait(timeout=30)
    finally:
        if proc is not None and proc.poll() is None:
            proc.kill()
            proc.wait()
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
