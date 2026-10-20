"""f31 alpha run: the Effects Rack and presets, in a scratch HOME.

    python3 tools/alpha/f31.py --exe <path to the release wavcraft> [--port 47131]

A 3 s synthetic stereo file (left constant +0.25, right constant -0.5) is opened. Checked, via
STATE and a saved copy read back:
  1. RACK set 1 invert, RACK set 2 gain gain_db=-6.0206, RACK mix / power / bad requests;
  2. RACK apply on 1 s to 2 s is ONE history step named "Effects Rack", and the rack then
     resets to bypass (master power and slot power off); a second apply is refused;
  3. Mix 50 % with Invert on 2 s to 3 s leaves silence there;
  4. presets: an effect preset and a rack preset saved, listed, loaded back, deleted;
     bad names and unknown effects are refused;
  5. the saved samples match, and the app's log holds no ERROR line.
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
    """(left, right) of a PCM WAV at `frame`, as -1..1."""
    with wave.open(path, "rb") as w:
        ch, width = w.getnchannels(), w.getsampwidth()
        w.setpos(frame)
        raw = w.readframes(1)
    scale = float(1 << (8 * width - 1))
    return tuple(int.from_bytes(raw[i * width:(i + 1) * width], "little", signed=True) / scale for i in range(ch))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47131)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(os.path.expanduser("~/dev/craft/_wt/scratch"), exist_ok=True)
    work = tempfile.mkdtemp(prefix="f31.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    wav, saved, token = os.path.join(work, "in.wav"), os.path.join(work, "out.wav"), os.path.join(work, "control.token")
    make_wav(wav)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
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
        steps = lambda: int(c.state()["steps"])
        st = c.state()
        check("the file opens", st["frames"] == str(3 * RATE) and st["steps"] == "1", st)
        r = c.ask("RACK")
        check("the rack starts with 10 empty slots, power on", r.startswith("OK power=on in=0 out=0 mix=100") and "10:-" in r and "11:" not in r, r)
        a = c.ask("RACK set 1 invert")
        b = c.ask("RACK set 2 gain gain_db=-6.0206")
        check("two slots set", "1:invert:on" in b and "2:gain:on gain_db=-6.0206" in b, f"{a}; {b}")
        bad = [c.ask("RACK set 99 invert"), c.ask("RACK set 3 nonesuch"), c.ask("RACK set 3 gain gain_db=999"), c.ask("RACK mix 150"),
               c.ask("RACK in 99"), c.ask("RACK power 1 maybe"), c.ask("RACK frobnicate")]
        check("bad rack requests answer ERR", all(x.startswith("ERR") for x in bad), bad)
        check("power toggles", "1:invert:off" in c.ask("RACK power 1 off") and "1:invert:on" in c.ask("RACK power 1 on"), c.ask("RACK"))
        # apply: one step, then bypass
        c.ask(f"SELECT {RATE} {2 * RATE}")
        n0 = steps()
        a = c.ask("RACK apply")
        st = c.state()
        check("apply is one step named Effects Rack", a.endswith("changed=1") and steps() == n0 + 1 and st["undo"] == "Effects Rack", f"{a}; {st}")
        r = c.ask("RACK")
        check("the rack reset to bypass", r.startswith("OK power=off") and "1:invert:off" in r and "2:gain:off" in r, r)
        a = c.ask("RACK apply")
        check("a second apply is refused (nothing is on)", a.startswith("ERR") and steps() == n0 + 1, a)
        # mix
        c.ask("RACK power master on")
        c.ask("RACK power 1 on")
        c.ask("RACK mix 50")
        c.ask(f"SELECT {2 * RATE} {3 * RATE}")
        a = c.ask("RACK apply")
        check("Mix 50 % of Invert is silence", a.endswith("changed=1") and steps() == n0 + 2, a)
        # presets
        r1 = c.ask("PRESET save gain tame")
        l = c.ask("PRESET list gain")
        check("an effect preset (from the first Gain slot) saves and lists", r1.startswith("OK") and "tame" in l, f"{r1}; {l}")
        c.ask("RACK set 2 gain gain_db=3")
        ld = c.ask("PRESET load gain tame")
        r = c.ask("RACK")
        check("an effect preset loads back into the rack slot", ld.startswith("OK") and "2:gain:on gain_db=-6.0206" in r, f"{ld}; {r}")
        c.ask("EFFECT_DIALOG gain gain_db=5")
        ld = c.ask("PRESET load gain tame")
        d = c.ask("EFFECT_DIALOG")
        check("an effect preset loads into an open dialog", ld.startswith("OK") and "-6.02" in d, f"{ld}; {d}")
        c.ask("EFFECT_DIALOG CANCEL")
        c.ask("RACK mix 100")
        c.ask("RACK power master on")
        c.ask("RACK power 1 on")
        rs = c.ask("PRESET save rack my rack")
        c.ask("RACK in -6")
        c.ask("RACK set 1 none")
        rl = c.ask("PRESET load rack my rack")
        r = c.ask("RACK")
        check("a rack preset round-trips", rs.startswith("OK") and rl.startswith("OK") and "power=on in=0" in r and "1:invert:on" in r and "mix=100" in r, f"{rs}; {rl}; {r}")
        check("rack presets list", "my rack" in c.ask("PRESET list rack"), c.ask("PRESET list rack"))
        d1 = c.ask("PRESET delete gain tame")
        d2 = c.ask("PRESET delete rack my rack")
        check("presets delete", d1.startswith("OK") and d2.startswith("OK") and "tame" not in c.ask("PRESET list gain") and c.ask("PRESET load gain tame").startswith("ERR"), f"{d1}; {d2}")
        bad = [c.ask("PRESET save gain ../evil"), c.ask("PRESET save nonesuch x"), c.ask("PRESET load gain missing"), c.ask("PRESET delete gain missing"), c.ask("PRESET bogus")]
        check("bad preset requests answer ERR", all(x.startswith("ERR") for x in bad), bad)
        c.ask(f"SAVE {saved}")
        st = c.state()
        check("save finished", os.path.exists(saved) and st.get("busy") == "0", st)
        want = {
            RATE // 2: (0.25, -0.5),                # before the selection
            RATE * 3 // 2: (-0.125, 0.25),          # Invert then Gain -6.0206 dB
            RATE * 5 // 2: (0.0, 0.0),              # Mix 50 % of Invert: silence
        }
        for frame, (l, r) in want.items():
            got = sample(saved, frame)
            check(f"saved samples at {frame / RATE:.2f} s", abs(got[0] - l) < 2e-3 and abs(got[1] - r) < 2e-3, f"got {got}, want {(l, r)}")
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
    print("f31 scripted run:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


main()
