#!/usr/bin/env python3
"""K1.6 Episode Check wiring check; run against a release wavcraft binary.

    python3 tools/alpha/k1.py --exe <release build>
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


def make_wav(path, seconds, clipped=False):
    with wave.open(path, "wb") as out:
        out.setnchannels(2)
        out.setsampwidth(2)
        out.setframerate(RATE)
        chunk_frames = 16_384
        for base in range(0, seconds * RATE, chunk_frames):
            count = min(chunk_frames, seconds * RATE - base)
            frames = bytearray()
            for i in range(base, base + count):
                value = 32767 if clipped and 144_000 <= i < 144_960 else int(5000 * ((i % 97) / 48 - 1))
                frames.extend(struct.pack("<hh", value, value))
            out.writeframesraw(frames)


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

    def check_state(self):
        answer = self.ask("CHECK state")
        return dict(field.split("=", 1) for field in answer.split()[1:])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", required=True)
    exe = os.path.abspath(parser.parse_args().exe)
    work = tempfile.mkdtemp(prefix="k1.", dir=os.getcwd())
    proc = None
    try:
        good = os.path.join(work, "clipped.wav")
        long_file = os.path.join(work, "long.wav")
        make_wav(good, 20, clipped=True)
        make_wav(long_file, 30 * 60)
        home = os.path.join(work, "home")
        os.makedirs(home)
        token_path = os.path.join(work, "token")
        env = dict(os.environ, HOME=home, WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token_path,
                   WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_NO_AUDIO="1")
        proc = subprocess.Popen([exe, good], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.monotonic() + 60
        while not os.path.isfile(token_path) and time.monotonic() < deadline:
            time.sleep(.1)
        c = Control(open(token_path, encoding="utf-8").read().strip())
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            state = c.ask("STATE")
            if "frames=960000 " in state and "busy=0" in state:
                break
            time.sleep(.1)
        c.ask("ACTION check.run")
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            state = c.check_state()
            if state.get("running") == "0" and state.get("progress") == "100":
                break
            time.sleep(.05)
        assert state.get("findings", "0").isdigit() and int(state["findings"]) >= 1, state
        assert state.get("stale") == "0", state
        print("PASS clipped 20 s file completes with findings and stale=0", state)
        c.ask("SELECT 0 100")
        c.ask("ACTION edit.delete")
        assert c.check_state().get("stale") == "1", c.check_state()
        print("PASS report becomes stale after a history edit")

        c.ask("OPEN " + long_file)
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            status = c.ask("STATE")
            if "frames=86400000 " in status and "busy=0" in status:
                break
            time.sleep(.1)
        c.ask("ACTION check.run")
        cancelled_at = time.monotonic()
        c.ask("CHECK cancel")
        deadline = cancelled_at + 1
        while time.monotonic() < deadline:
            state = c.check_state()
            if state.get("running") == "0" and state.get("findings") == "0":
                break
            time.sleep(.02)
        assert state.get("running") == "0" and state.get("findings") == "0", state
        print("PASS 30-minute scan cancels within 1 s with no findings", state)
        c.ask("QUIT discard")
        proc.wait(timeout=30)
    finally:
        if proc is not None and proc.poll() is None:
            proc.kill()
            proc.wait()
        shutil.rmtree(work, ignore_errors=True)


main()
