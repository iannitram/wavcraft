#!/usr/bin/env python3
"""f104 scripted run: the Mixer panel through the control channel.

    python3 tools/alpha/f104.py --exe <path to the wavcraft binary>

Starts the app with a fresh scratch HOME, shows the Mixer panel, then checks `MIXER state` against
`MIXER set` for fader, pan, mute and solo (the meters are the mix engine's own), quits through the
channel and fails on any ERROR line in the app's log. The scratch folder is deleted at the end.
"""
import argparse, math, os, shutil, socket, subprocess, sys, tempfile, time

PORT = 47204


class Control:
    def __init__(self, port, token_file, tries=120):
        for _ in range(tries):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask("TOKEN " + open(token_file).read().strip()) != "OK":
            raise RuntimeError("the control channel refused the token")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()


def parse(line):
    """`OK tracks=4 any_solo=0 | master vol=.. | t1 vol=..` -> {name: {key: value}}"""
    parts = line.split(" | ")
    out = {"head": dict(kv.split("=") for kv in parts[0].split()[1:])}
    for p in parts[1:]:
        w = p.split()
        out[w[0]] = {k: float(v) for k, v in (kv.split("=") for kv in w[1:])}
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f104.", dir=base)
    home = os.path.join(work, "home")
    logs = os.path.join(work, "logs")
    os.makedirs(home)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"), RUST_BACKTRACE="1",
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"), flush=True)

    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([exe], env=env, stdout=err, stderr=err)
    try:
        for _ in range(60):
            if os.path.exists(token) or proc.poll() is not None:
                break
            time.sleep(0.5)
        c = Control(PORT, token)
        check("the Mixer panel opens", c.ask("MIXER show") == "OK")
        s = parse(c.ask("MIXER state"))
        t1 = s["t1"]
        check("four tracks and a master", s["head"]["tracks"] == "4" and "master" in s and "t4" in s, s["head"])
        check("a new strip is at unity, centred, audible and sounding", t1["vol"] == 0 and t1["pan"] == 0 and t1["audible"] == 1 and abs(t1["peak"] + 6.0) < 0.5, t1)  # a 0.5 sine: -6 dB, strip meter taken before the pan
        base_peak = t1["peak"]
        s = parse(c.ask("MIXER set 1 vol=-12"))
        check("the fader lowers the strip's meter by the same dB", abs((base_peak - s["t1"]["peak"]) - 12) < 0.5 and s["t1"]["vol"] == -12, s["t1"])
        check("the other strips are untouched", s["t2"]["vol"] == 0 and abs(s["t2"]["peak"] - parse(c.ask("MIXER state"))["t2"]["peak"]) < 0.01)
        parse(c.ask("MIXER set 1 vol=0 pan=0 solo=1"))
        centre = parse(c.ask("MIXER state"))["master"]["peak"]
        s = parse(c.ask("MIXER set 1 pan=-1"))
        left = s["master"]["peak"]
        check("hard-left pan: the main mix is 3 dB louder than centre in its loudest channel (constant-power law)",
              s["t1"]["pan"] == -1 and abs((left - centre) - 3.01) < 0.5, f"centre {centre} left {left}")
        parse(c.ask("MIXER set 1 solo=0"))
        s = parse(c.ask("MIXER set 1 pan=0"))
        s = parse(c.ask("MIXER set 2 mute=1"))
        check("mute silences the strip and its meter", s["t2"]["mute"] == 1 and s["t2"]["audible"] == 0 and s["t2"]["peak"] < -100, s["t2"])
        check("the others keep sounding", s["t1"]["audible"] == 1 and s["t1"]["peak"] > -20)
        s = parse(c.ask("MIXER set 3 solo=1"))
        check("solo silences every other strip", s["head"]["any_solo"] == "1" and s["t3"]["audible"] == 1 and s["t3"]["peak"] > -20
              and s["t1"]["audible"] == 0 and s["t1"]["peak"] < -100 and s["t4"]["audible"] == 0, s)
        s = parse(c.ask("MIXER set 3 solo=0 mute=0"))
        s = parse(c.ask("MIXER set 2 mute=0"))
        check("clearing solo and mute brings every strip back", all(s[f"t{i}"]["audible"] == 1 for i in range(1, 5)) and s["head"]["any_solo"] == "0", s["head"])
        s = parse(c.ask("MIXER set master vol=-6"))
        check("the master fader is a strip too", s["master"]["vol"] == -6, s["master"])
        check("out-of-range values are refused or clamped", c.ask("MIXER set 1 vol=loud").startswith("ERR") and parse(c.ask("MIXER set 1 vol=99"))["t1"]["vol"] == 12)
        check("an unknown track is refused", c.ask("MIXER set 9 vol=0").startswith("ERR"))
        time.sleep(1.0)  # let the panel draw a few frames with real meters
        check("the app is still up after drawing the panel", proc.poll() is None)
        check("QUIT closes the app", c.ask("QUIT") == "OK")
        for _ in range(40):
            if proc.poll() is not None:
                break
            time.sleep(0.25)
        check("the app exited", proc.poll() is not None)
    finally:
        if proc.poll() is None:
            proc.kill()
    errors = []
    for d, _, fs in os.walk(logs):
        for f in fs:
            errors += [l.strip()[:200] for l in open(os.path.join(d, f), errors="replace") if " ERROR " in l]
    errtxt = open(os.path.join(work, "stderr.log"), errors="replace").read()
    check("the app's log has no ERROR line", not errors and "panicked" not in errtxt, errors[:3] or errtxt[-300:])
    ok = all(c[1] for c in checks)
    print("RESULT", "pass" if ok else "fail", f"{sum(c[1] for c in checks)}/{len(checks)}")
    shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


main()
