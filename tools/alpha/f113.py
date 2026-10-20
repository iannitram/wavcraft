#!/usr/bin/env python3
"""f113 scripted run: the Mixer panel through the control channel.

    python3 tools/alpha/f113.py --exe <path to the wavcraft binary>

Starts the app with a fresh scratch HOME, makes a session with two tracks (a synthetic tone clip each),
sets fader, pan and mute through `MIXER set` and checks `SESSION state` (the session model) and
`MIXER state` (the strips), adds and removes a track and checks the strips follow, quits through the
channel and fails on any ERROR line in the app's log. The scratch folder is deleted at the end.
"""
import argparse, json, math, os, struct, wave, shutil, socket, subprocess, sys, tempfile, time

PORT = 47213


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
    work = tempfile.mkdtemp(prefix="f113.", dir=base)
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

    work_wav = os.path.join(work, "tone.wav")
    with wave.open(work_wav, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(48000)
        w.writeframes(b"".join(struct.pack("<h", int(16000 * math.sin(2 * math.pi * 440 * i / 48000))) for i in range(48000 * 2)))

    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([exe], env=env, stdout=err, stderr=err)
    try:
        for _ in range(60):
            if os.path.exists(token) or proc.poll() is not None:
                break
            time.sleep(0.5)
        c = Control(PORT, token)
        check("without a session the mixer says so", c.ask("MIXER state").startswith("ERR MIXER: no multitrack session"))
        check("the Mixer panel opens", c.ask("MIXER show") == "OK")
        check("a session is made", c.ask("SESSION new Alpha rate=48000").startswith("OK"))
        check("two tracks", c.ask("TRACK add Drums mono").startswith("OK") and c.ask("TRACK add Bass mono").startswith("OK"))
        check("a clip on each", c.ask(f"CLIP add 1 {work_wav} 0").startswith("OK") and c.ask(f"CLIP add 2 {work_wav} 0").startswith("OK"))
        s = parse(c.ask("MIXER state"))
        check("one strip per session track, plus the master", s["head"]["tracks"] == "2" and "t1" in s and "t2" in s and "master" in s, s["head"])
        check("the strips meter the session's clips", s["t1"]["peak"] > -20 and s["t2"]["peak"] > -20, s)

        def tracks():
            return json.loads(c.ask("SESSION state")[3:].strip())["tracks"]

        c.ask("MIXER set 1 vol=-6 pan=-0.5 mute=1")
        t = tracks()[0]
        check("fader, pan and mute are in SESSION state", abs(t["volume_db"] + 6) < 0.05 and abs(t["pan"] + 0.5) < 0.01 and t["mute"] is True, t)
        check("the other track is untouched", tracks()[1]["volume_db"] == 0 and tracks()[1]["mute"] is False)
        c.ask("TRACK volume 2 -9")
        s = parse(c.ask("MIXER state"))
        check("an edit made on the session shows on the strip", abs(s["t2"]["vol"] + 9) < 0.05, s["t2"])
        check("a muted strip is silent", s["t1"]["audible"] == 0 and s["t1"]["peak"] < -100, s["t1"])
        c.ask("TRACK add Vox stereo")
        check("an added track is a new strip", parse(c.ask("MIXER state"))["head"]["tracks"] == "3")
        c.ask("TRACK remove 1")
        s = parse(c.ask("MIXER state"))
        check("a removed track's strip goes", s["head"]["tracks"] == "2" and abs(s["t1"]["vol"] + 9) < 0.05, s["head"])
        check("an unknown strip is refused", c.ask("MIXER set 9 vol=0").startswith("ERR"))
        time.sleep(1.0)  # let the panel draw a few frames
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
