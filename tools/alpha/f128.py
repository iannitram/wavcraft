"""f128 alpha run: VST3 hosting in the Effects Rack, in a scratch HOME.

    python3 tools/alpha/f128.py --exe <path to the release wavcraft> [--plugins DIR] [--port 47228]

WAVCRAFT_VST3_PATH is set to: an empty folder, a folder with a broken "plugin" (a text file named
Broken.vst3), and DIR (a folder holding the test fixture bundle, when given). Checked:
  1. VST3 list before any scan: no plugins, not scanning;
  2. VST3 scan runs in the background; the broken plugin does not hang or crash the app;
  3. with the fixture: it is listed (the fixture's synth is not: instruments are left out);
  4. RACK set 1 vst3:<name> puts it in the slot; an unknown name is refused;
  5. RACK apply through the plugin is one step named "Effects Rack" and, the fixture's gain
     at its default (measured: halves the signal, so the plugin did process it) changes 0.25/-0.5 to 0.125/-0.25;
  6. the app's log holds no ERROR line.
Without --plugins the load checks (3 to 5) are skipped with a message.
"""
import argparse, glob, os, shutil, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

RATE = 48000


def make_wav(path):
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        frame = (8192).to_bytes(2, "little", signed=True) + (-16384).to_bytes(2, "little", signed=True)
        w.writeframes(frame * (3 * RATE))


def sample(path, frame):
    with wave.open(path, "rb") as w:
        ch, width = w.getnchannels(), w.getsampwidth()
        w.setpos(frame)
        raw = w.readframes(1)
    scale = float(1 << (8 * width - 1))
    return tuple(int.from_bytes(raw[i * width:(i + 1) * width], "little", signed=True) / scale for i in range(ch))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--plugins")
    ap.add_argument("--port", type=int, default=47228)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="f128.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
    home, logs, sessions, empty, broken = (os.path.join(work, d) for d in ("home", "logs", "sessions", "empty", "broken"))
    for d in (home, logs, sessions, empty, broken):
        os.makedirs(d)
    with open(os.path.join(broken, "Broken.vst3"), "w") as f:
        f.write("this is not a plugin\n")
    wav, saved, token = os.path.join(work, "in.wav"), os.path.join(work, "out.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    path = os.pathsep.join([empty, broken] + ([args.plugins] if args.plugins else []))
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token, WAVCRAFT_VST3_PATH=path)
    checks = []

    def check(label, ok, detail):
        checks.append((label, bool(ok), detail))

    proc = subprocess.Popen([args.exe, wav], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            if os.path.exists(token):
                break
            time.sleep(0.2)
        c = Control(args.port, open(token).read().strip())
        r = c.ask("VST3 list")
        check("before a scan nothing is listed", r.startswith("OK scanning=0 count=0"), r)
        check("a bad VST3 request answers ERR", c.ask("VST3 frobnicate").startswith("ERR"), c.ask("VST3 frobnicate"))
        check("RACK set with an unscanned plugin is refused", c.ask("RACK set 1 vst3:Fixture Gain").startswith("ERR"), c.ask("RACK"))
        r = c.ask("VST3 scan")
        check("the scan starts", r.startswith("OK scanning=1"), r)
        for _ in range(150):
            r = c.ask("VST3 list")
            if "scanning=0" in r:
                break
            time.sleep(0.2)
        check("the scan finishes (the broken plugin did not hang it)", "scanning=0" in r, r)
        check("the app still answers", c.state()["frames"] == str(3 * RATE), c.state())
        if args.plugins:
            check("the fixture effect is listed, its synth is not", "count=1" in r and "Fixture Gain" in r and "Fixture Synth" not in r, r)
            a = c.ask("RACK set 1 vst3:Fixture Gain")
            check("the plugin goes in slot 1", a.startswith("OK") and "1:vst3:Fixture Gain:on" in a, a)
            check("an unknown plugin name is refused", c.ask("RACK set 2 vst3:Nonesuch").startswith("ERR"), c.ask("RACK"))
            c.ask(f"SELECT {RATE} {2 * RATE}")
            n0 = int(c.state()["steps"])
            a = c.ask("RACK apply")
            st = c.state()
            check("apply through the plugin is one step named Effects Rack", a.startswith("OK") and int(st["steps"]) == n0 + 1 and st["undo"] == "Effects Rack", f"{a}; {st}")
            c.ask(f"SAVE {saved}")
            st = c.state()
            got = sample(saved, RATE * 3 // 2)
            check("the fixture's gain halved the samples (it ran: input 0.25/-0.5)", abs(got[0] - 0.125) < 2e-3 and abs(got[1] + 0.25) < 2e-3, f"got {got}, want (0.125, -0.25)")
        else:
            print("SKIP load checks: no fixture plugin was built (no --plugins)")
        c.ask("QUIT discard")
        proc.wait(timeout=30)
    finally:
        if proc.poll() is None:
            proc.kill()
    errors = [l.strip()[:200] for p in glob.glob(os.path.join(logs, "*")) if os.path.isfile(p) for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR line", not errors, errors[:3])
    ok = all(o for _, o, _ in checks)
    for label, o, detail in checks:
        print(("PASS " if o else "FAIL ") + label + ("" if o else f"  [{detail}]"))
    if ok and not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    else:
        print("kept", work)
    print("f128 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


main()
