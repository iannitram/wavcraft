#!/usr/bin/env python3
"""Alpha run for f89 (A005): resizing the dock zones by their dividers, through the control channel.

    python3 tools/alpha/f89.py --exe <path to the release wavcraft>

Run 1 (fresh scratch HOME): PANELS reports the zone sizes; DIVIDER drag changes one zone (left grows
with dx, right shrinks with dx, bottom grows when dy is negative); a huge drag stops at the window
and a huge negative one at the 120-point minimum; DIVIDER corner resizes a side zone and the bottom
one together; DIVIDER reset restores the default; bad words are refused; settings.txt holds the sizes.
Run 2 (same HOME): the dragged sizes are back after a restart.
The app's log must hold no ERROR line. The scratch folder is deleted at the end.
"""
import argparse, os, shutil, socket, subprocess, sys, tempfile, time, wave, math, struct


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


def state(c):
    ans = c.ask("PANELS")
    assert ans.startswith("OK "), ans
    return dict(kv.split("=", 1) for kv in ans.split()[1:])


def zones(c):
    s = state(c)
    return {k: s[k] for k in s if k.startswith(("zone.", "front.", "maximized"))}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    args = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f89.", dir=base)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    wav = os.path.join(work, "tone.wav")
    with wave.open(wav, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(22050)
        w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / 22050))) for i in range(22050 * 2)))
    port, token = 47189, os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               APPDATA=os.path.join(home, "AppData", "Roaming"), LOCALAPPDATA=os.path.join(home, "AppData", "Local"),
               WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))

    def run(body):
        err = open(os.path.join(work, "stderr.log"), "a")
        proc = subprocess.Popen([args.exe, wav], env=env, stdout=err, stderr=err)
        try:
            c = Control(port, token)
            body(c)
            check("QUIT closes the app", c.ask("QUIT") == "OK")
            proc.wait(20)
        except Exception as e:
            check("run completed", False, f"{type(e).__name__}: {e}")
            proc.kill()

    def sizes(c):
        s = state(c)
        return {z: float(s["size." + z]) for z in ("left", "right", "bottom")}

    def run1(c):
        c.ask("PANEL move history left")
        c.ask("PANEL move selection bottom")
        z0 = sizes(c)
        check("default sizes are reported", z0 == {"left": 220.0, "right": 240.0, "bottom": 180.0}, z0)
        c.ask("DIVIDER drag left 40 99")
        check("drag left by dx grows the left zone, dy ignored", sizes(c)["left"] == 260.0, sizes(c))
        c.ask("DIVIDER drag bottom 99 -50")
        check("drag bottom up by 50 grows the bottom zone", sizes(c)["bottom"] == 230.0, sizes(c))
        c.ask("DIVIDER drag left -9999 0")
        check("a huge negative drag stops at the minimum (120)", sizes(c)["left"] == 120.0, sizes(c))
        c.ask("DIVIDER drag left 9999 0")
        big = sizes(c)["left"]
        check("a huge drag stops at the window, not at the range limit", 120.0 < big < 1200.0, big)
        c.ask("DIVIDER drag bottom 0 -9999")
        big = sizes(c)["bottom"]
        check("the bottom zone stops at the window height", 230.0 < big < 1200.0, big)
        check("reset left restores 220", c.ask("DIVIDER reset left").startswith("OK") and sizes(c)["left"] == 220.0, sizes(c))
        check("reset bottom restores 180", c.ask("DIVIDER reset bottom").startswith("OK") and sizes(c)["bottom"] == 180.0, sizes(c))
        before = sizes(c)
        check("corner moves the left and bottom zones together", c.ask("DIVIDER corner 30 -40").startswith("OK"), c.ask("PANELS"))
        after = sizes(c)
        check("corner: left +30, bottom +40", after["left"] == before["left"] + 30 and after["bottom"] == before["bottom"] + 40, after)
        check("a corner on a side with no panel is refused", c.ask("DIVIDER corner 1 1 right").startswith("ERR"))
        check("a divider of an empty zone is refused", c.ask("DIVIDER drag right 5 0").startswith("ERR"))
        check("an unknown zone is refused", c.ask("DIVIDER drag moon 5 0").startswith("ERR"))
        check("a non-number is refused", c.ask("DIVIDER drag left x 0").startswith("ERR"))
        check("a bare DIVIDER is refused", c.ask("DIVIDER").startswith("ERR"))
        c.ask("DIVIDER drag left 20 0")
        c.ask("DIVIDER drag bottom 0 -10")
        found = [os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs if f == "settings.txt"]
        text = open(found[0]).read() if found else ""
        want = f"left:{int(after['left'] + 20)},right:240,bottom:{int(after['bottom'] + 10)}"
        check("settings.txt holds the sizes", "zone_sizes = " + want in text, text.replace("\n", " | ")[-200:])
        state.kept = sizes(c)

    def run2(c):
        now = sizes(c)
        check("after a restart the dragged sizes are back", now == state.kept, (now, state.kept))
        c.ask("DIVIDER reset left")
        check("and a reset works after the restart", sizes(c)["left"] == 220.0, sizes(c))

    run(run1)
    run(run2)
    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR line", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    ok = all(c[1] for c in checks)
    for label, good, detail in checks:
        print(("PASS " if good else "FAIL ") + label + ("" if good else f"  [{detail}]"))
    print("RESULT", "pass" if ok else "fail")
    if ok:
        shutil.rmtree(work, ignore_errors=True)
    else:
        print("kept for inspection:", work)
    sys.exit(0 if ok else 1)


main()
