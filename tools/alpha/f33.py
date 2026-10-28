#!/usr/bin/env python3
"""f33 alpha run: Match Loudness through the control channel.

    python3 tools/alpha/f33.py --exe <release wavcraft binary> [--port 47133] [--keep]

Generates a quiet stereo tone and a peaky tone (16-bit WAV, standard library only) in a scratch folder,
starts the app with HOME, the session folder, the log folder and the token file all inside it, then checks:
- `MATCH target=-16 tp=-1 limit=on` on a quiet tone: `LOUDNESS` reads back -16 +-0.5 LUFS and a true peak at
  or under -0.7 dBTP; one history step "Match Loudness"; undo brings the original loudness back, redo again;
- on a peaky file with limiting on: the loudness lands on -16 +-0.5 and the true peak stays under -0.7;
- the same file with `limit=off`: true peak at or under -1 dBTP, loudness short of the target, reported;
- the broadcast preset reaches -23 +-0.5.
Quits through the channel; any ERROR line in the app's log fails the run; the scratch folder is deleted.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

RATE = 48000


def write_wav(path, left, right):
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        out = bytearray()
        for a, b in zip(left, right):
            out += struct.pack("<hh", max(-32768, min(32767, round(a * 32767))), max(-32768, min(32767, round(b * 32767))))
        w.writeframes(bytes(out))


def sine(secs, hz, peak_db, phase=0.0):
    a = 10 ** (peak_db / 20)
    return [a * math.sin(2 * math.pi * hz * n / RATE + phase) for n in range(int(secs * RATE))]


class Control:
    def __init__(self, port, token):
        for _ in range(80):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=120)
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

    def open(self, path):
        assert self.ask(f"OPEN {path}") == "OK"
        for _ in range(240):
            if "busy=0" in self.ask("STATE") and "frames=0 " not in self.ask("STATE"):
                return
            time.sleep(0.25)
        raise RuntimeError("open timed out")


def fields(answer):
    assert answer.startswith("OK"), answer
    out = {}
    for kv in answer.split()[1:]:
        k, _, v = kv.partition("=")
        out[k] = float("-inf") if v == "-inf" else (float(v) if v.replace(".", "", 1).replace("-", "", 1).isdigit() else v)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47133)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f33.", dir=base)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    token_file = os.path.join(work, "control.token")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + (f"  [{detail}]" if detail else ""), flush=True)

    quiet = os.path.join(work, "quiet.wav")
    s = sine(20, 1000, -34)
    write_wav(quiet, s, s)
    peaky = os.path.join(work, "peaky.wav")
    s = sine(20, 440, -32)
    for i in range(1000, len(s), int(0.37 * RATE)):
        s[i] = 0.5
    write_wav(peaky, s, s)

    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token_file, RUST_BACKTRACE="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([args.exe], env=env, stdout=err, stderr=err)

    def settle(c):
        for _ in range(240):
            if "busy=0" in c.ask("STATE"):
                return
            time.sleep(0.25)
        raise RuntimeError("busy never cleared")

    try:
        for _ in range(80):
            if os.path.exists(token_file):
                break
            time.sleep(0.25)
        c = Control(args.port, open(token_file).read().strip())

        c.open(quiet)
        before = fields(c.ask("LOUDNESS"))
        check("quiet: starts at -34 LUFS", abs(before["integrated"] + 34) < 0.3, before["integrated"])
        r = fields(c.ask("MATCH target=-16 tp=-1 limit=on"))
        settle(c)
        m = fields(c.ask("LOUDNESS"))
        check("quiet: MATCH reports a change and no limiting", r["changed"] == 1 and r["limited"] == 0, r)
        check("quiet: LOUDNESS reads -16 +-0.5 after", abs(m["integrated"] + 16) < 0.5, m["integrated"])
        check("quiet: true peak at or under -0.7 dBTP", m["true_peak"] <= -0.7, m["true_peak"])
        st = c.ask("STATE")
        check("quiet: one history step named Match Loudness", "steps=2" in st and "undo=Match Loudness" in st, st)
        c.ask("ACTION edit.undo")
        settle(c)
        m = fields(c.ask("LOUDNESS"))
        check("quiet: undo brings -34 back", abs(m["integrated"] + 34) < 0.3, m["integrated"])
        c.ask("ACTION edit.redo")
        settle(c)
        m = fields(c.ask("LOUDNESS"))
        check("quiet: redo gives -16 again", abs(m["integrated"] + 16) < 0.5, m["integrated"])

        c.open(peaky)
        r = fields(c.ask("MATCH target=-16 tp=-1 limit=on"))
        settle(c)
        m = fields(c.ask("LOUDNESS"))
        check("peaky: the limiter was used", r["limited"] == 1, r)
        check("peaky: -16 +-0.5 LUFS after", abs(m["integrated"] + 16) < 0.5, m["integrated"])
        check("peaky: true peak at or under -0.7 dBTP", m["true_peak"] <= -0.7, m["true_peak"])

        c.open(peaky)
        r = fields(c.ask("MATCH target=-16 tp=-1 limit=off"))
        settle(c)
        m = fields(c.ask("LOUDNESS"))
        check("peaky, limiting off: true peak at or under -1 dBTP", m["true_peak"] <= -0.95, m["true_peak"])
        check("peaky, limiting off: loudness short of the target, and reported", m["integrated"] < -16.5 and abs(r["after"] - m["integrated"]) < 0.05, (m["integrated"], r))

        c.open(quiet)
        r = fields(c.ask("MATCH preset=broadcast"))
        settle(c)
        m = fields(c.ask("LOUDNESS"))
        check("broadcast preset: -23 +-0.5 LUFS", abs(m["integrated"] + 23) < 0.5, m["integrated"])

        check("quit", c.ask("QUIT discard") == "OK")
        proc.wait(20)
    except Exception as e:
        check("script ran to the end", False, f"{type(e).__name__}: {e}")
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()
        err.close()
    log_lines = []
    if os.path.isdir(logs):
        for name in os.listdir(logs):
            log_lines += open(os.path.join(logs, name), errors="replace").read().splitlines()
    errors = [l[:200] for l in log_lines if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_lines) and not errors, errors[:3] or f"{len(log_lines)} lines")
    if errors or not all(ok for _, ok, _ in checks):
        print("--- log tail ---")
        print("\n".join(log_lines[-15:]))
        print(open(os.path.join(work, "stderr.log"), errors="replace").read()[-1500:])
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    failed = [l for l, ok, _ in checks if not ok]
    print(f"RESULT {'PASS' if not failed else 'FAIL'}: {len(checks) - len(failed)}/{len(checks)} checks")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
