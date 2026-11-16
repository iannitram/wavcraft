#!/usr/bin/env python3
"""Alpha run for I2.3: the panel layout survives a restart, through the control channel.

    python3 tools/alpha/i2-restart.py --exe <path to the built wavcraft> [--port <n>]

Run 1 (fresh scratch HOME): PANELS shows History; PANEL close history hides it; DIVIDER drag left 40 0
moves the left divider; QUIT closes the app with exit 0.
Run 2 (same HOME, a new process): History is still hidden and size.left is what the drag left;
WORKSPACE reset brings History back and puts size.left back to its first value; QUIT, exit 0.
The app's log must hold no ERROR line. Prints PASS or FAIL per check, then "N/M checks passed".
The scratch folder is deleted when every check passes; otherwise its place is printed.
"""
import argparse, math, os, shutil, socket, subprocess, sys, tempfile, time, wave, struct


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Control:
    def __init__(self, port, token_file):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        for _ in range(40):
            if os.path.exists(token_file):
                break
            time.sleep(0.25)
        if self.ask("TOKEN " + open(token_file).read().strip()) != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()


def panels(c):
    ans = c.ask("PANELS")
    if not ans.startswith("OK "):
        raise RuntimeError(f"PANELS answered {ans!r}")
    return dict(part.split("=", 1) for part in ans.split()[1:] if "=" in part)


def size_left(c):
    return float(panels(c)["size.left"])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=None, help="control port for both runs (default: a free port)")
    args = ap.parse_args()

    work = tempfile.mkdtemp(prefix="i2.", dir=tempfile.gettempdir())
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    token = os.path.join(work, "control.token")
    wav = os.path.join(work, "tone.wav")
    with wave.open(wav, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(22050)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / 22050))) for i in range(22050 * 2)))

    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok)))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"), flush=True)

    def env_for(port):
        return dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
                    XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
                    APPDATA=os.path.join(home, "AppData", "Roaming"), LOCALAPPDATA=os.path.join(home, "AppData", "Local"),
                    WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")

    def run(n, body):
        """Start the app on the scratch HOME, run `body(control)`, then QUIT and wait for the exit."""
        port = args.port or free_port()
        if os.path.exists(token):
            os.remove(token)
        err = open(os.path.join(work, "stderr.log"), "a")
        proc = subprocess.Popen([args.exe, wav], env=env_for(port), stdout=err, stderr=err)
        try:
            c = Control(port, token)
            body(c)
            check(f"QUIT answers OK (run {n})", c.ask("QUIT") == "OK")
            check(f"the app exits with 0 (run {n})", proc.wait(20) == 0)
        except Exception as e:
            check(f"run {n} completed", False, f"{type(e).__name__}: {e}")
            proc.kill()
            proc.wait(20)
        finally:
            err.close()

    state = {}

    def run1(c):
        state["first"] = size_left(c)
        check("fresh start: History is shown", panels(c).get("history") == "1", panels(c).get("history"))
        ans = c.ask("PANEL close history")
        check("PANEL close history answers OK", ans.startswith("OK "), ans)
        check("History hidden (history=0)", panels(c).get("history") == "0")
        ans = c.ask("DIVIDER drag left 40 0")
        check("DIVIDER drag answers OK", ans.startswith("OK "), ans)
        state["dragged"] = size_left(c)
        check("the left divider moved", state["dragged"] != state["first"], f"{state['first']} -> {state['dragged']}")

    def run2(c):
        check("after a restart History is still hidden", panels(c).get("history") == "0", panels(c).get("history"))
        check("after a restart size.left is what the drag left", size_left(c) == state["dragged"],
              f"{size_left(c)} != {state['dragged']}")
        ans = c.ask("WORKSPACE reset")
        check("WORKSPACE reset answers OK", ans.startswith("OK "), ans)
        check("after WORKSPACE reset History is shown", panels(c).get("history") == "1", panels(c).get("history"))
        check("after WORKSPACE reset size.left is the first value", size_left(c) == state["first"],
              f"{size_left(c)} != {state['first']}")

    run(1, run1)
    run(2, run2)

    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [line.strip()[:200] for p in log_files for line in open(p, errors="replace") if " ERROR " in line]
    check("the app's log has no ERROR line", bool(log_files) and not errors, errors[:3] or "no log file")

    passed = sum(1 for _, ok in checks if ok)
    print(f"{passed}/{len(checks)} checks passed")
    if passed == len(checks):
        shutil.rmtree(work, ignore_errors=True)
        sys.exit(0)
    print("kept for inspection:", work)
    sys.exit(1)


if __name__ == "__main__":
    main()
