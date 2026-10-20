#!/usr/bin/env python3
"""f03 alpha run: Reverse, Amplify, Normalize, Fade In and Fade Out through the control channel.

    python3 tools/alpha/f03.py --exe <path to the release wavcraft> [--keep]

A scratch HOME and session folder hold a synthetic 4 s stereo 48 kHz file (left: a straight ramp
from -0.4 to +0.4, right: a constant 0.25). Each effect runs through `ACTION effect.*` (numbers
through `PARAM`), the result is SAVEd to a WAV and read back sample by sample, and STATE's undo
label is checked. The run fails on any check that fails, a non-clean QUIT, or an ERROR line in
the app's own log.
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, SECS = 48000, 4
N = RATE * SECS


class Control:
    def __init__(self, port, token):
        for _ in range(80):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self):
        for _ in range(300):
            head, _, labels = self.ask("STATE").partition(" undo=")
            fields = dict(kv.split("=", 1) for kv in head.split()[1:])
            undo, _, redo = labels.partition(" redo=")
            fields.update(undo=undo, redo=redo)
            if fields.get("busy") == "0":
                return fields
            time.sleep(0.2)
        raise RuntimeError("still busy")


def source(path):
    left = [-0.4 + 0.8 * k / (N - 1) for k in range(N)]
    with wave.open(path, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<hh", round(l * 32767), round(0.25 * 32767)) for l in left))
    return left


def read(path):
    """Both channels of a 16- or 24-bit PCM WAV as floats."""
    with wave.open(path, "rb") as w:
        raw, width = w.readframes(w.getnframes()), w.getsampwidth()
    scale = float(1 << (8 * width - 1))
    vals = [int.from_bytes(raw[i:i + width], "little", signed=True) / scale for i in range(0, len(raw), width)]
    return vals[0::2], vals[1::2]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    exe, keep = ap.parse_args().exe, ap.parse_args().keep
    base = os.environ.get("WAVCRAFT_SCRATCH") or None  # a folder for the run; the system temp folder when unset
    if base:
        os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f03.", dir=base)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    wav = os.path.join(work, "in.wav")
    left = source(wav)
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0)); port = s.getsockname()[1]
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))

    out = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([exe, wav], env=env, stdout=out, stderr=out)
    try:
        for _ in range(80):
            if os.path.exists(token):
                break
            time.sleep(0.25)
        c = Control(port, open(token).read().strip())
        for _ in range(100):
            if c.state().get("frames") == str(N):
                break
            time.sleep(0.2)
        check("the file opened", c.state().get("frames") == str(N), c.state())
        mid = N // 2

        def result(name, sel, label, want_undo):
            c.ask(f"SELECT {sel[0]} {sel[1]}")
            a = c.ask(f"ACTION {name}")
            st = c.state()
            check(f"{name}: answered OK and changed the file", a.endswith("changed=1"), a)
            check(f"{name}: undo step is named {want_undo!r}", st["undo"] == want_undo, st["undo"])
            p = os.path.join(work, label + ".wav")
            check(f"{name}: SAVE works", c.ask(f"SAVE {p}") == "OK")
            c.state()
            return read(p)

        # Reverse the whole file (no selection).
        l, r = result("effect.reverse", (0, 0), "rev", "Reverse")
        tol = 3e-4
        check("Reverse: left runs backwards", abs(l[0] - left[-1]) < tol and abs(l[mid] - left[N - 1 - mid]) < tol and abs(l[-1] - left[0]) < tol, (l[0], l[-1]))
        check("Reverse: right (constant) is unchanged", all(abs(v - 0.25) < tol for v in r[::997]))
        c.ask("ACTION edit.undo")

        # Amplify by -6 dB, whole file.
        check("PARAM amplify.db accepted", c.ask("PARAM amplify.db -6") == "OK")
        check("PARAM with a bad name is refused", c.ask("PARAM volume 3").startswith("ERR"))
        l, r = result("effect.amplify", (0, 0), "amp", "Amplify")
        g = 10 ** (-6 / 20)
        check("Amplify -6 dB: both channels scaled", abs(l[mid] - left[mid] * g) < tol and abs(r[1000] - 0.25 * g) < tol, (l[mid], left[mid] * g, r[1000]))
        c.ask("ACTION edit.undo")

        # Normalize to -2 dBFS.
        check("PARAM normalize.db accepted", c.ask("PARAM normalize.db -2") == "OK")
        l, r = result("effect.normalize", (0, 0), "norm", "Normalize")
        peak = max(max(abs(v) for v in l), max(abs(v) for v in r))
        check("Normalize -2 dBFS: peak reached", abs(peak - 10 ** (-2 / 20)) < 1e-3, peak)
        c.ask("ACTION edit.undo")

        # Fade In linear over the first second.
        l, r = result("effect.fadeIn.linear", (0, RATE), "fi", "Fade In (Linear)")
        check("Fade In (Linear): starts silent, half way at half", abs(l[0]) < tol and abs(l[RATE // 2] - left[RATE // 2] * 0.5) < tol and abs(l[RATE + 10] - left[RATE + 10]) < tol, (l[0], l[RATE // 2]))
        c.ask("ACTION edit.undo")

        # Fade Out raised cosine over the last second.
        l, r = result("effect.fadeOut.cosine", (N - RATE, N), "fo", "Fade Out (Raised Cosine)")
        check("Fade Out (Raised Cosine): ends silent, half way at half", abs(l[-1]) < tol and abs(r[-1]) < tol and abs(r[N - RATE // 2] - 0.125) < 2e-3 and abs(l[N - RATE - 10] - left[N - RATE - 10]) < tol, (l[-1], r[N - RATE // 2]))

        check("QUIT", c.ask("QUIT discard") == "OK")
        proc.wait(20)
    except Exception as e:
        check("the run", False, f"{type(e).__name__}: {e}")
        proc.kill()
    out.close()
    bad = []
    for d, _, files in os.walk(logs):
        for f in files:
            bad += [l for l in open(os.path.join(d, f), errors="replace") if " ERROR " in l or "ERROR" in l.split(" ")[:4]]
    bad += [l for l in open(os.path.join(work, "stderr.log"), errors="replace") if "panicked" in l]
    check("no ERROR line in the app's log, no panic on stderr", not bad, bad[:3])
    check("the app exited 0", proc.returncode == 0, proc.returncode)
    for label, ok, detail in checks:
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"))
    if not keep:
        shutil.rmtree(work, ignore_errors=True)
    failed = [c for c in checks if not c[1]]
    print(f"{len(checks) - len(failed)}/{len(checks)} checks passed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
