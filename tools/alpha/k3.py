#!/usr/bin/env python3
"""K3.3 Episode Check finding selection and playback; run against a release wavcraft binary.

    python3 tools/alpha/k3.py --exe <release build>
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


def make_wav(path):
    with wave.open(path, "wb") as out:
        out.setnchannels(2)
        out.setsampwidth(2)
        out.setframerate(RATE)
        chunk_frames = 16_384
        for base in range(0, 20 * RATE, chunk_frames):
            count = min(chunk_frames, 20 * RATE - base)
            frames = bytearray()
            for i in range(base, base + count):
                left = 32767 if 144_000 <= i < 144_480 else int(5000 * ((i % 97) / 48 - 1))
                right = int(5000 * ((i % 97) / 48 - 1))
                frames.extend(struct.pack("<hh", left, right))
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

    def ask_raw(self, line):
        self.file.write(line + "\n")
        self.file.flush()
        return self.file.readline().strip()

    def ask(self, line):
        answer = self.ask_raw(line)
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
    work = tempfile.mkdtemp(prefix="k3.")  # system temp; removed in finally
    proc = None
    try:
        audio = os.path.join(work, "clipped.wav")
        make_wav(audio)
        home = os.path.join(work, "home")
        os.makedirs(home)
        token_path = os.path.join(work, "token")
        env = dict(os.environ, HOME=home, WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token_path,
                   WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_NO_AUDIO="1")
        proc = subprocess.Popen([exe, audio], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.monotonic() + 60
        while not os.path.isfile(token_path) and time.monotonic() < deadline:
            time.sleep(.1)
        with open(token_path, encoding="utf-8") as token_file:
            c = Control(token_file.read().strip())

        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            state = c.ask("STATE")
            if "frames=960000 " in state and "busy=0" in state:
                break
            time.sleep(.1)

        assert c.ask_raw("CHECK list").startswith("ERR"), "list should need a finished check"
        assert c.ask_raw("CHECK click 0").startswith("ERR"), "click should need a finished check"
        print("PASS list and click need a finished check")

        c.ask("ACTION check.run")
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            state = c.check_state()
            if state.get("running") == "0" and state.get("progress") == "100":
                break
            time.sleep(.05)
        assert state.get("findings", "0").isdigit() and int(state["findings"]) >= 1, state
        answer = c.ask("CHECK list")
        assert answer.startswith("OK rows="), answer
        fields = answer.split(" ", 2)
        row_count = int(fields[1].split("=", 1)[1])
        rows = fields[2].split(";") if len(fields) == 3 else []
        assert row_count == int(state["findings"]) == len(rows), (row_count, state, rows)
        clipping = []
        for index, row in enumerate(rows):
            parts = row.split(",")
            assert len(parts) == 4, row
            row_index, kind, start, end = parts
            assert int(row_index) == index and int(start) < int(end), row
            if kind == "Clipping":
                clipping.append((index, start, end))
        assert len(clipping) == 1 and clipping[0][1:] == ("144000", "144480"), clipping
        click_index = clipping[0][0]
        print("PASS CHECK list reports every finding kind and range", answer)

        clicked = c.ask(f"CHECK click {click_index}")
        assert clicked.startswith("OK") and "playing=1" in clicked, clicked
        deadline = time.monotonic() + 2
        state = ""
        while time.monotonic() < deadline:
            state = c.ask("STATE")
            if "playing=1" in state:
                break
            time.sleep(.02)
        assert "selection=144000-144480" in state and "playing=1" in state, state
        c.ask("PLAYBACK STOP")
        print("PASS CHECK click selects and plays 144000-144480", state)
        c.ask("QUIT discard")
        proc.wait(timeout=30)
    finally:
        if proc is not None and proc.poll() is None:
            proc.kill()
            proc.wait()
        shutil.rmtree(work, ignore_errors=True)


main()
