#!/usr/bin/env python3
"""f56 alpha run: the input meter toggle, the arm/monitor mode and the mic check, on a FAKE input only.

    python3 tools/alpha/f56.py --exe <release wavcraft binary> [--port 47156] [--keep]

Starts the app three times with HOME, the session folder, the log folder and the token file in a scratch
folder and WAVCRAFT_FAKE_INPUT set (a synthetic source: no input device is ever opened, so no microphone
permission prompt):
- sine:440:-12 -> toggle the input meter on: the bars read the input (peak -12, no clip), the stream is open
  only while the toggle is on; MICCHECK 1 reports peak -12, average 3 dB under it, verdict ok, and the
  control channel keeps answering while it listens; a mic check with the meter off leaves the input closed;
- sine:440:-50 -> verdict too-quiet;
- sine:440:6 (hard-clipped) -> the clip light lights and the verdict is clipping.
Quits through the channel; any ERROR line in the app's log fails the run; the scratch folder is deleted.
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time


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

    def send(self, line):
        self.f.write(line + "\n")
        self.f.flush()

    def read(self):
        return self.f.readline().strip()

    def ask(self, line):
        self.send(line)
        return self.read()


def fields(answer):
    assert answer.startswith("OK"), answer
    out = {}
    for kv in answer.split()[1:]:
        k, _, v = kv.partition("=")
        try:
            out[k] = float(v)
        except ValueError:
            out[k] = v
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47156)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f56.", dir=base)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + (f"  [{detail}]" if detail else ""), flush=True)

    def run_app(tag, fake, script):
        token_file = os.path.join(work, f"control-{tag}.token")
        env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
                   WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token_file,
                   WAVCRAFT_FAKE_INPUT=fake, RUST_BACKTRACE="1")
        err = open(os.path.join(work, f"stderr-{tag}.log"), "w")
        proc = subprocess.Popen([args.exe], env=env, stdout=err, stderr=err)
        try:
            for _ in range(80):
                if os.path.exists(token_file):
                    break
                time.sleep(0.25)
            c = Control(args.port, open(token_file).read().strip())
            script(c, token_file)
            check(f"{tag}: quit", c.ask("QUIT") == "OK")
            proc.wait(20)
        except Exception as e:
            check(f"{tag}: script ran to the end", False, f"{type(e).__name__}: {e}")
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait()
            err.close()

    def settle(c, tries=20):
        m = {}
        for _ in range(tries):
            m = fields(c.ask("METERS"))
            time.sleep(0.1)
        return m

    def ok_script(c, token_file):
        m = fields(c.ask("METERS"))
        check("idle: bars show the output, input stream closed, not armed", m["view"] == "output" and m["armed"] == 0 and m["input_open"] == 0 and m["l"] < -100, m)
        check("toggle on", c.ask("ACTION meter.input.toggle").startswith("OK"))
        m = settle(c)
        check("armed: the stream is open and the bars show the input", m["armed"] == 1 and m["input_open"] == 1 and m["view"] == "input", m)
        check("armed: peak -12 dBFS +-0.3 on both channels, no clip", abs(m["l_hold"] + 12) < 0.3 and abs(m["r_hold"] + 12) < 0.3 and m["l_clip"] == 0 and m["r_clip"] == 0, m)
        check("toggle off", c.ask("ACTION meter.input.toggle").startswith("OK"))
        m = fields(c.ask("METERS"))
        check("off: the stream is closed again and the bars show the output", m["armed"] == 0 and m["input_open"] == 0 and m["view"] == "output", m)
        check("arm turns it on (and stays on)", c.ask("ACTION meter.input.arm").startswith("OK") and c.ask("ACTION meter.input.arm").startswith("OK")
              and fields(c.ask("METERS"))["armed"] == 1)
        c.ask("ACTION meter.input.toggle")
        # Mic check with the meter off: it opens the input for the check only, and the channel stays alive.
        c2 = Control(args.port, open(token_file).read().strip())  # a connection serves one line at a time
        c.send("MICCHECK 1")
        t0 = time.time()
        during = c2.ask("STATE")
        check("mic check: the control channel answers while it listens", during.startswith("OK") and time.time() - t0 < 0.9, during[:40])
        m = fields(c2.ask("METERS"))
        check("mic check: the input is open while listening", m["input_open"] == 1, m)
        r = fields(c.read())
        check("mic check: waited about a second", 0.9 < time.time() - t0 < 4.0, f"{time.time() - t0:.2f}s")
        check("mic check: peak -12 dBFS +-0.1", abs(r["peak"] + 12) < 0.1, r)
        check("mic check: average 3 dB under the peak +-0.15", abs(r["average"] - (r["peak"] - 3.01)) < 0.15, r)
        check("mic check: verdict ok, no clips", r["verdict"] == "ok" and r["clips"] == 0, r)
        m = fields(c.ask("METERS"))
        check("mic check: the input is closed again afterwards", m["input_open"] == 0, m)
        bad = c.ask("MICCHECK 0")
        check("mic check: a bad length is refused", bad.startswith("ERR"), bad)

    def quiet_script(c, token_file):
        r = fields(c.ask("MICCHECK 1"))
        check("quiet source (-50 dBFS): too-quiet", r["verdict"] == "too-quiet" and abs(r["peak"] + 50) < 0.2, r)

    def clip_script(c, token_file):
        c.ask("ACTION meter.input.toggle")
        m = settle(c, 10)
        check("hot source: the clip light is lit on the meter", m["l_clip"] == 1 and m["r_clip"] == 1, m)
        r = fields(c.ask("MICCHECK 1"))
        check("hot source: verdict clipping", r["verdict"] == "clipping" and r["clips"] > 0 and abs(r["peak"]) < 0.01, r)
        m = fields(c.ask("METERS RESET"))
        check("clicking the meter clears the light", m["l_clip"] == 0, m)

    run_app("ok", "sine:440:-12", ok_script)
    run_app("quiet", "sine:440:-50", quiet_script)
    run_app("clip", "sine:440:6", clip_script)

    log_lines = []
    if os.path.isdir(logs):
        for name in os.listdir(logs):
            log_lines += open(os.path.join(logs, name), errors="replace").read().splitlines()
    errors = [l[:200] for l in log_lines if " ERROR " in l]
    check("the app's own log has no ERROR", bool(log_lines) and not errors, errors[:3] or f"{len(log_lines)} lines")
    if errors or not all(ok for _, ok, _ in checks):
        print("--- log tail ---")
        print("\n".join(log_lines[-15:]))
        for name in sorted(os.listdir(work)):
            if name.startswith("stderr"):
                print(name, open(os.path.join(work, name), errors="replace").read()[-800:])
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    failed = [l for l, ok, _ in checks if not ok]
    print(f"RESULT {'PASS' if not failed else 'FAIL'}: {len(checks) - len(failed)}/{len(checks)} checks")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
