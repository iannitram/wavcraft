#!/usr/bin/env python3
"""f81 scripted run: pointer readout (HOVER), the status fields and last-operation time (STATE).

    python3 tools/alpha/f81.py --exe <wavcraft> [--port 47181]

A 10 s stereo 16-bit 48 kHz WAV: left is a 440 Hz sine at amplitude 0.5, right is silent. Checked at
known pixel positions: time, channel, amplitude and dBFS under the pointer, the stored sample, the
frequency over the spectrogram (against an independent calculation here), the ruler and outside the
editor; STATE's statusbar list, transport and last operation; no ERROR line in the app's log.
"""
import argparse, math, os, shutil, signal, struct, subprocess, sys, tempfile, time, wave

RATE, SECS = 48000, 10


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
        buf = bytearray()
        for i in range(RATE * SECS):
            buf += struct.pack("<hh", int(round(0.5 * 32767 * math.sin(2 * math.pi * 440 * i / RATE))), 0)
            if len(buf) > 1 << 20:
                w.writeframes(bytes(buf)); buf.clear()
        w.writeframes(bytes(buf))


class Control:
    def __init__(self, port, token):
        import socket
        for _ in range(80):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60); break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        assert self.ask(f"TOKEN {token}") == "OK"

    def ask(self, line):
        self.f.write(line + "\n"); self.f.flush()
        return self.f.readline().strip()


def kv(answer):
    head = answer.split(" undo=")[0]
    return dict(p.split("=", 1) for p in head.split()[1:] if "=" in p)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47181)
    o = ap.parse_args()
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f81.", dir=scratch_root)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + (f"  [{detail}]" if not ok or detail else ""), flush=True)

    proc = None
    try:
        home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
        os.makedirs(home)
        wav = os.path.join(work, "tone.wav")
        make_wav(wav)
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions, XDG_CONFIG_HOME=os.path.join(home, ".config"),
                   XDG_CACHE_HOME=os.path.join(home, ".cache"), WAVCRAFT_CONTROL_PORT=str(o.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
        err = open(os.path.join(work, "stderr.log"), "w")
        proc = subprocess.Popen([o.exe, wav], env=env, stdout=err, stderr=err)
        for _ in range(80):
            if os.path.exists(token): break
            time.sleep(0.25)
        c = Control(o.port, open(token).read().strip())
        st = {}
        for _ in range(240):
            st = kv(c.ask("STATE"))
            if st.get("busy") == "0" and st.get("frames") == str(RATE * SECS): break
            time.sleep(0.25)
        check("the file opened", st.get("frames") == str(RATE * SECS), st)
        # Wait for the first painted frame (the layout exists then).
        lay = {}
        for _ in range(120):
            a = c.ask("HOVER")
            if a.startswith("OK"):
                lay = kv(a); break
            time.sleep(0.25)
        check("the editor reports its layout", lay.get("channels") == "2" and lay.get("wave") == "1" and lay.get("spectrum") == "1", lay)
        W, H, R = float(lay["width"]), float(lay["height"]), float(lay["ruler"])
        start, fpp = float(lay["start"]), float(lay["frames_per_px"])

        # Time: at x the frame is start + fpp * x.
        x = W * 0.31 + 0.5
        want_frame = round(start + fpp * x)
        body, half_h = H - R, (H - R) / 2
        lane_h = half_h / 2  # two channels
        # Waveform, channel 0 (left): a quarter of the way down its lane is amplitude +0.5-ish.
        y_mid0 = R + lane_h / 2
        a = kv(c.ask(f"HOVER {x} {y_mid0}"))
        check("over the left waveform lane: pane, channel, time", a.get("pane") == "wave" and a.get("channel") == "0" and int(a["frame"]) == want_frame, a)
        check("lane centre is amplitude 0", abs(float(a["amp"])) < 0.02, a)
        y_q = R + lane_h / 2 - 0.5 * (lane_h / 2 - 2)
        a = kv(c.ask(f"HOVER {x} {y_q}"))
        check("half way up the lane is amplitude +0.5, -6.02 dBFS", abs(float(a["amp"]) - 0.5) < 0.01 and abs(float(a["dbfs"]) + 6.02) < 0.2, a)
        f = int(a["frame"])
        expect = 0.5 * math.sin(2 * math.pi * 440 * f / RATE)
        check("the stored sample under the pointer is the 440 Hz sine's", abs(float(a["sample"]) - expect) < 1e-3, f"{a['sample']} vs {expect:.6f}")
        # Right channel is silent.
        y_r = R + lane_h + lane_h / 2
        a = kv(c.ask(f"HOVER {x} {y_r}"))
        check("the right lane is channel 1 and its sample is 0", a.get("channel") == "1" and abs(float(a["sample"])) < 1e-6, a)
        # Time text.
        sec = want_frame / RATE
        m, s_ = int(sec // 60), sec - 60 * int(sec // 60)
        a = kv(c.ask(f"HOVER {x} {y_mid0}"))
        check("the time text is m:ss.mmm of that frame", a["time"] == f"{m}:{int(s_):02d}.{int((want_frame % RATE) * 1000 // RATE):03d}", a["time"])
        # Spectrogram: default axis is log, 40 Hz .. 24 kHz.
        sp_top = R + half_h
        lo, hi = 40.0, 24000.0
        for frac in (0.1, 0.5, 0.9):
            ly = frac * lane_h
            y = sp_top + ly
            t = 1 - min(max((ly - 1) / (lane_h - 2), 0), 1)
            want = lo * (hi / lo) ** t
            a = kv(c.ask(f"HOVER {x} {y}"))
            ok = a.get("pane") == "spectrum" and a.get("channel") == "0" and abs(float(a["hz"]) - want) / want < 0.01
            check(f"spectrogram lane 0 at {int(frac * 100)} % down reads {want:.0f} Hz", ok, a)
        a = kv(c.ask(f"HOVER {x} {sp_top + lane_h + lane_h / 2}"))
        check("spectrogram lane 1 is channel 1", a.get("pane") == "spectrum" and a.get("channel") == "1", a)
        check("the ruler has a time but no level", kv(c.ask(f"HOVER {x} 5")).get("pane") == "ruler")
        check("outside the editor says so", c.ask(f"HOVER {W + 50} 100") == "OK pane=outside")
        check("a bad HOVER is refused", c.ask("HOVER a b").startswith("ERR"))

        # Status: the bar's fields are the settings list (statusbar=), the transport is play=, and
        # the last operation's time rides on STATE (main's STATE no longer carries status values).
        st = kv(c.ask("STATE"))
        check("status: the bar shows duration and bit depth by default", "duration" in st.get("statusbar", "") and "bit_depth" in st.get("statusbar", ""), st.get("statusbar"))
        check("status: transport is stopped", st.get("play") == "stopped", st.get("play"))
        check("status: the open's time is recorded", st.get("last_op") == "Open" and int(st.get("last_op_ms", "-1")) >= 0, f"{st.get('last_op')} {st.get('last_op_ms')}")
        c.ask(f"SELECT {RATE} {2 * RATE}")
        c.ask("ACTION edit.delete")
        st = kv(c.ask("STATE"))
        check("status: an edit's time is recorded", st.get("last_op") == "Delete", st.get("last_op"))
        check("quits", c.ask("QUIT discard") == "OK")
        proc.wait(20)
        log_text = "".join(open(os.path.join(logs, n), errors="replace").read() for n in sorted(os.listdir(logs))) if os.path.isdir(logs) else ""
        bad = [l for l in log_text.splitlines() if " ERROR" in l or "panicked" in l]
        check("no ERROR line in the app's log", not bad and log_text != "", bad[:3] if bad else ("log empty" if not log_text else ""))
    except Exception as e:
        check("script", False, f"{type(e).__name__}: {e}")
    finally:
        if proc and proc.poll() is None:
            proc.send_signal(signal.SIGTERM)
            try: proc.wait(10)
            except subprocess.TimeoutExpired: proc.kill()
        shutil.rmtree(work, ignore_errors=True)
    ok = all(c[1] for c in checks)
    print(f"{'PASS' if ok else 'FAIL'}: {sum(c[1] for c in checks)}/{len(checks)} checks")
    sys.exit(0 if ok else 1)


main()
