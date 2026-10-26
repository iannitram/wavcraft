#!/usr/bin/env python3
"""f102 (C322) scripted run: a synthetic stereo WAV is opened in the release app, then
`SCSOURCE test` reads it back as a SoundCraft clip source through the model session. The frames
and the FNV-1a checksum it reports must equal ones computed here from the WAV. QUIT; the app's
log must hold no ERROR line.

    python3 tools/alpha/f102.py --exe <wavcraft> [--scratch-root DIR]
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE, SECONDS = 48000, 10
PORT = 47202


def make_wav(path):
    n = RATE * SECONDS
    ch = [[], []]
    for i in range(n):
        ch[0].append(int(12000 * math.sin(2 * math.pi * 220 * i / RATE)))
        ch[1].append(int(9000 * math.sin(2 * math.pi * 330 * i / RATE)) + (i % 17) * 7)
    with wave.open(path, "wb") as w:
        w.setnchannels(2), w.setsampwidth(2), w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<hh", a, b) for a, b in zip(*ch)))
    return n, ch


def fnv(channels):
    h = 0xcbf29ce484222325
    for c in channels:
        for v in c:
            for b in struct.pack("<f", v / 32768.0):
                h = ((h ^ b) * 0x100000001b3) & 0xFFFFFFFF_FFFFFFFF
    return h


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
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self):
        while True:
            fields = dict(kv.split("=", 1) for kv in self.ask("STATE").split()[1:] if "=" in kv)
            if fields.get("busy") == "0":
                return fields
            time.sleep(0.2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--scratch-root", default=os.path.expanduser("~/dev/craft/_wt/scratch"))
    a = ap.parse_args()
    os.makedirs(a.scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f102.", dir=a.scratch_root)
    ok = False
    try:
        home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
        os.makedirs(home)
        wav = os.path.join(work, "synthetic.wav")
        frames, ch = make_wav(wav)
        want = fnv(ch)
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_CONTROL_PORT=str(PORT),
                   WAVCRAFT_CONTROL_TOKEN_FILE=token, XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"))
        err = open(os.path.join(work, "stderr.log"), "w")
        proc = subprocess.Popen([a.exe], env=env, stdout=err, stderr=err)
        checks = []
        try:
            for _ in range(120):
                if os.path.exists(token):
                    break
                time.sleep(0.25)
            c = Control(PORT, open(token).read().strip())
            checks.append(("no document yet is an error", c.ask("SCSOURCE test").startswith("ERR")))
            checks.append(("OPEN", c.ask(f"OPEN {wav}") == "OK"))
            st = c.state()
            for _ in range(100):
                if int(st["frames"]) == frames:
                    break
                time.sleep(0.2)
                st = c.state()
            checks.append(("opened frames", int(st["frames"]) == frames))
            ans = c.ask("SCSOURCE test")
            print("SCSOURCE test ->", ans)
            f = dict(kv.split("=", 1) for kv in ans.split()[1:]) if ans.startswith("OK") else {}
            checks.append(("frames read", f.get("frames") == str(frames)))
            checks.append(("checksum equals the WAV's", f.get("checksum") == f"{want:016x}"))
            checks.append(("QUIT", c.ask("QUIT") == "OK"))
            proc.wait(20)
        except Exception as e:
            checks.append((f"{type(e).__name__}: {e}", False))
        if proc.poll() is None:
            proc.kill()
        log = ""
        for d, _, fs in os.walk(logs):
            for fn in fs:
                log += open(os.path.join(d, fn), errors="replace").read()
        errs = [l for l in log.splitlines() if "ERROR" in l]
        checks.append(("no ERROR line in the app log", not errs))
        for name, good in checks:
            print(("PASS " if good else "FAIL ") + name)
        for l in errs[:5]:
            print("  ", l)
        ok = all(g for _, g in checks)
        if not ok:
            print("log tail:\n" + "\n".join(log.splitlines()[-15:]))
    finally:
        shutil.rmtree(work, ignore_errors=True)
    print("f102 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


main()
