#!/usr/bin/env python3
"""f74 (A205) scripted run: a 44.1 kHz file played on a forced 48 kHz silent test sink.

    python3 tools/alpha/f74.py --exe <path to the wavcraft binary>

Starts the app with HOME in a fresh scratch folder, WAVCRAFT_NO_AUDIO=1 (test sink) at
WAVCRAFT_NO_AUDIO_RATE=48000, opens a generated 10 s 44.1 kHz stereo sine, and through the
control channel checks: PLAYBACK reports device 48000 / doc 44100 / resampling on; Play advances
the playhead in document frames at the device's pace (about 44,100 a second, measured against the
wall clock); Stop holds it; a 48 kHz file reports resampling off; the quality setting is written
to settings.txt and read back; no ERROR line in the app's own log.
"""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from golden import Control, free_port  # noqa: E402


def write_sine(path, rate, seconds, hz=1000.0):
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(rate)
        n = int(rate * seconds)
        w.writeframes(b"".join(struct.pack("<hh", v, v) for v in (int(12000 * math.sin(2 * math.pi * hz * i / rate)) for i in range(n))))


def pb(c):
    answer = c.ask("PLAYBACK")
    assert answer.startswith("OK "), answer
    return dict(kv.split("=", 1) for kv in answer.split()[1:])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f74.", dir=scratch_root)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    for d in (home, logs, sessions):
        os.makedirs(d)
    a, b = os.path.join(work, "a-44k.wav"), os.path.join(work, "b-48k.wav")
    write_sine(a, 44100, 10)
    write_sine(b, 48000, 4)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"), RUST_BACKTRACE="1",
               WAVCRAFT_NO_AUDIO="1", WAVCRAFT_NO_AUDIO_RATE="48000")
    port, token = free_port(), os.path.join(work, "control.token")
    env.update(WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    checks = []

    def check(label, ok, detail):
        checks.append((label, bool(ok), detail))

    proc = subprocess.Popen([args.exe, a], env=env, stdout=open(os.path.join(work, "stdout.log"), "w"), stderr=subprocess.STDOUT)
    try:
        for _ in range(120):
            if os.path.exists(token) and os.path.getsize(token) >= 64:
                break
            time.sleep(0.25)
        c = Control(port, open(token).read().strip())
        st = c.state()
        check("the 44.1 kHz file opened", st.get("rate") == "44100" and int(st["frames"]) == 441000, st)
        p = pb(c)
        check("PLAYBACK reports device 48000, doc 44100, resampling on, test sink",
              p["device_rate"] == "48000" and p["doc_rate"] == "44100" and p["resampling"] == "1" and p["sink"] == "1" and p["playing"] == "0", p)
        c.ask("SELECT 0 0")
        t0 = time.time()
        c.ask("PLAYBACK PLAY")
        time.sleep(0.5)
        early = int(pb(c)["underruns"])  # the first callback may beat the producer's first chunk
        time.sleep(1.5)
        p = pb(c)
        wall = time.time() - t0
        pos = int(p["position"])
        want = wall * 44100
        # The pace is the device's: 44,100 document frames per second, within 5 % (start-up latency, timer slop).
        check("playing: the position advances at 44,100 document frames a second", p["playing"] == "1" and abs(pos - want) < 0.05 * want + 2000,
              f"position {pos} after {wall:.2f} s wall (expected about {want:.0f}); {p}")
        check("no underruns after the start-up buffer fills", int(p["underruns"]) == early and early <= 2, f"{early} at 0.5 s, {p['underruns']} at 2 s")
        c.ask("PLAYBACK STOP")
        held = int(pb(c)["position"])
        time.sleep(0.5)
        p = pb(c)
        check("Stop holds the playhead", p["playing"] == "0" and int(p["position"]) == held, p)
        # A selection plays only itself and the playhead ends exactly on its end.
        c.ask("SELECT 44100 66150")
        c.ask("PLAYBACK PLAY")
        time.sleep(1.2)
        p = pb(c)
        check("a half-second selection (22,050 frames) plays to its end and stops", p["playing"] == "0" and int(p["position"]) == 66150, p)
        # Quality setting is stored.
        a1 = c.ask("PLAYBACK QUALITY fast")
        settings = os.path.join(home, ".config", "wavcraft", "settings.txt")
        alt = [os.path.join(home, "Library", "Application Support", "wavcraft", "settings.txt"), settings]
        text = next((open(x).read() for x in alt if os.path.exists(x)), "")
        check("QUALITY fast is reported and written to settings.txt", "quality=fast" in a1 and "playback_quality = fast" in text, f"{a1}; settings: {text.strip()[-40:]!r}")
        check("a bad quality word is refused", c.ask("PLAYBACK QUALITY best").startswith("ERR"), "")
        # A file at the device's rate does not resample.
        c.ask(f"OPEN {b}")
        st = c.state()
        p = pb(c)
        check("a 48 kHz file on the 48 kHz sink reports resampling off", st.get("rate") == "48000" and p["resampling"] == "0" and p["doc_rate"] == "48000", p)
        c.ask("SELECT 0 0")
        t0 = time.time()
        c.ask("PLAYBACK PLAY")
        time.sleep(1.0)
        p = pb(c)
        wall = time.time() - t0
        check("same-rate playback advances 48,000 a second", abs(int(p["position"]) - wall * 48000) < 0.05 * wall * 48000 + 2000, f"{p['position']} after {wall:.2f} s")
        c.ask("PLAYBACK STOP")
        check("QUIT", c.ask("QUIT") .startswith("OK"), "")
        proc.wait(timeout=30)
    finally:
        if proc.poll() is None:
            proc.kill()
    errors = []
    for root, _, files in os.walk(logs):
        for f in files:
            errors += [l.strip()[:200] for l in open(os.path.join(root, f), errors="replace") if " ERROR " in l]
    check("the app's own log has no ERROR", not errors, errors[:3])
    bad = [x for x in checks if not x[1]]
    for label, ok, detail in checks:
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  -- {detail}"))
        if ok:
            print(f"       {detail}")
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    print("f74 scripted run:", "PASS" if not bad else f"FAIL ({len(bad)})")
    sys.exit(1 if bad else 0)


main()
