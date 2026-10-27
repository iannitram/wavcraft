#!/usr/bin/env python3
"""fix286 scripted run: cancelling the Save As / Export dialog must not change what a later plain Save writes.

    python3 tools/alpha/fix286.py --exe <path to the release wavcraft> [--port 47188] [--keep]

A 32-bit float WAV is opened. `SAVEOPTS dialog wav 16 cancel` is the Save As button with 16-bit chosen and the
file dialog cancelled; a plain `SAVE` must then leave the file 32-bit float (header checked byte for byte).
`SAVEOPTS dialog wav 16 <path>` writes 16-bit to that path only; a later plain `SAVE` still writes float.
"""
import argparse, math, os, shutil, signal, socket, struct, subprocess, sys, tempfile, time

RATE, FRAMES = 48000, 48000


def write_float_wav(path):
    data = b"".join(struct.pack("<ff", 0.5 * math.sin(i * 0.05), 0.25 * math.sin(i * 0.031)) for i in range(FRAMES))
    fmt = struct.pack("<HHIIHH", 3, 2, RATE, RATE * 8, 8, 32)
    body = b"WAVE" + b"fmt " + struct.pack("<I", 16) + fmt + b"data" + struct.pack("<I", len(data)) + data
    open(path, "wb").write(b"RIFF" + struct.pack("<I", len(body)) + body)


def header(path):
    """(format tag, bits per sample) from the fmt chunk, or None."""
    b = open(path, "rb").read(4096)
    i = 12
    while i + 8 <= len(b):
        cid, size = b[i:i + 4], struct.unpack("<I", b[i + 4:i + 8])[0]
        if cid == b"fmt ":
            tag, _, _, _, _, bits = struct.unpack("<HHIIHH", b[i + 8:i + 24])
            if tag == 0xFFFE:  # extensible: the sub-format GUID starts with the real tag
                tag = struct.unpack("<H", b[i + 32:i + 34])[0]
            return tag, bits
        i += 8 + size + (size & 1)
    return None


class Control:
    def __init__(self, port, token):
        for _ in range(80):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.25)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def idle(self, timeout=60):
        t0 = time.time()
        while time.time() - t0 < timeout:
            head = self.ask("STATE").partition(" undo=")[0]
            d = dict(kv.split("=", 1) for kv in head.split()[1:])
            if d.get("busy") == "0":
                return d
            time.sleep(0.1)
        return {}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47188)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="fix286.", dir=base)
    checks = []

    def check(label, ok, detail=""):
        checks.append(bool(ok))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"), flush=True)

    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    src = os.path.join(work, "float.wav")
    write_float_wav(src)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions, XDG_CONFIG_HOME=os.path.join(home, ".config"),
               XDG_CACHE_HOME=os.path.join(home, ".cache"), RUST_BACKTRACE="1", WAVCRAFT_CONTROL_PORT=str(a.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([a.exe, src], env=env, stdout=err, stderr=err)
    try:
        for _ in range(80):
            if os.path.exists(token):
                break
            time.sleep(0.25)
        c = Control(a.port, open(token).read().strip())
        for _ in range(100):
            if int(c.idle().get("frames", 0)) == FRAMES:
                break
            time.sleep(0.2)
        check("opened a 32-bit float WAV", header(src) == (3, 32) and int(c.idle().get("frames", 0)) == FRAMES, header(src))

        check("dialog: 16-bit chosen, file dialog cancelled", c.ask("SAVEOPTS dialog wav 16 cancel") == "OK")
        check("cancelling wrote nothing", header(src) == (3, 32), header(src))
        r = c.ask("SAVE")
        c.idle(120)
        check("plain Save after the cancel", r == "OK", r)
        check("the file is still 32-bit float", header(src) == (3, 32), header(src))

        out = os.path.join(work, "sixteen.wav")
        check("dialog: 16-bit to a path", c.ask(f"SAVEOPTS dialog wav 16 {out}") == "OK")
        c.idle(120)
        check("that export is 16-bit PCM", header(out) == (1, 16), header(out))
        check("the original float file is untouched", header(src) == (3, 32), header(src))
        r = c.ask("SAVE")
        c.idle(120)
        check("plain Save after the export", r == "OK", r)
        check("it does not carry the export's 16-bit", header(out) == (3, 32), header(out))
        check("QUIT", c.ask("QUIT discard") == "OK")
        proc.wait(20)
    except Exception as e:
        check("scripted run", False, f"{type(e).__name__}: {e}")
        proc.kill()
    if proc.poll() is None:
        proc.send_signal(signal.SIGTERM)
    err.close()
    text = ""
    for d, _, files in os.walk(work):
        for n in files:
            if n.endswith(".log"):
                text += open(os.path.join(d, n), errors="replace").read()
    bad = [l for l in text.splitlines() if "ERROR" in l or "panicked" in l]
    check("no ERROR line in the app's log or stderr", not bad, bad[:5])
    ok = all(checks)
    print("RESULT", "PASS" if ok else "FAIL", f"{sum(checks)}/{len(checks)}", work if a.keep or not ok else "")
    if not a.keep and ok:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


main()
