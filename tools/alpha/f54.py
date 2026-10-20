#!/usr/bin/env python3
"""f54 scripted run: record offset / latency compensation (C010, C050) and software input monitoring (C025).

    python3 tools/alpha/f54.py --exe <wavcraft> [--keep]

Fake input only (WAVCRAFT_FAKE_INPUT=sine:440): no real input device is opened; monitoring goes to a null
sink (WAVCRAFT_MONITOR_SINK=null), so nothing is played. A synthetic 6 s 48 kHz stereo file is opened; takes are
recorded into it with different PREF record_offset_ms values and the saved document is compared with the take
file: the placed take is shifted by the offset, the take file on disk is unchanged. Fails on any ERROR in the log.
"""
import argparse, hashlib, json, os, shutil, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

PORT = 47154
RATE = 48000
FRAMES = 6 * RATE
MARGIN = 600  # a little over the 5 ms (240 frame) edge crossfade; outside it audio must be identical


def scratch():
    base = os.environ.get("ALPHA_SCRATCH") or tempfile.gettempdir()
    os.makedirs(base, exist_ok=True)
    return tempfile.mkdtemp(prefix="f54.", dir=base)


def write_source(path):
    """Quiet distinct patterns in the two channels, 24-bit, so every sample can be told apart."""
    chans = [[((i * 37 + c * 1013) % 20000 - 10000) * 13 for i in range(FRAMES)] for c in range(2)]
    raw = bytearray()
    for i in range(FRAMES):
        for c in range(2):
            raw += chans[c][i].to_bytes(3, "little", signed=True)
    with wave.open(path, "wb") as w:
        w.setnchannels(2), w.setsampwidth(3), w.setframerate(RATE)
        w.writeframes(bytes(raw))
    return chans


def read_wav(path):
    with wave.open(path, "rb") as w:
        n, ch, width = w.getnframes(), w.getnchannels(), w.getsampwidth()
        assert width == 3, f"expected 24-bit, got {width * 8}-bit"
        raw = w.readframes(n)
    ints = [int.from_bytes(raw[i:i + 3], "little", signed=True) for i in range(0, len(raw), 3)]
    return [ints[c::ch] for c in range(ch)]


def launch(exe, env):
    proc = subprocess.Popen([exe], env=env, stdout=subprocess.DEVNULL, stderr=open(env["STDERR"], "a"))
    token = env["WAVCRAFT_CONTROL_TOKEN_FILE"]
    for _ in range(120):
        if os.path.exists(token) or proc.poll() is not None:
            break
        time.sleep(0.25)
    return proc, Control(PORT, open(token).read().strip())


def sha(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    work = scratch()
    home, logs, sessions, takes = (os.path.join(work, d) for d in ("home", "logs", "sessions", "takes"))
    for d in (home, sessions):
        os.makedirs(d)
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=os.path.join(work, "control.token"),
               WAVCRAFT_FAKE_INPUT="sine:440", WAVCRAFT_MONITOR_SINK="null", WAVCRAFT_RECORD_DIR=takes,
               STDERR=os.path.join(work, "stderr.log"), RUST_BACKTRACE="1")
    checks = []

    def check(label, ok, detail=None):
        checks.append({"check": label, "ok": bool(ok), "detail": detail})

    src = os.path.join(work, "source.wav")
    orig = write_source(src)
    t0 = time.time()
    proc, c = launch(a.exe, env)

    def settle():
        return c.state(timeout=60)

    def save(name):
        out = os.path.join(work, name)
        r = c.ask(f"SAVE {out}")
        settle()
        assert r == "OK", r
        return read_wav(out)

    def wait_over(secs=15):
        t = time.time()
        while time.time() - t < secs:
            if c.state(wait=False)["recording"] == "0":
                break
            time.sleep(0.1)
        return settle()

    def case(name, select, start, secs=None):
        c.ask("HISTORY 0")
        settle()
        c.ask(f"SELECT {select[0]} {select[1]}")
        r = c.ask(f"RECORD start {start}")
        if not r.startswith("OK path="):
            return {"start": r}
        path = r.partition("path=")[2]
        facts = {"start": r, "path": path}
        time.sleep((secs or 1.0) / 2)
        facts["during"] = c.state(wait=False)
        if secs is not None:
            time.sleep(secs / 2)
            facts["stop"] = c.ask("RECORD stop")
        st = wait_over()
        facts["state"] = st
        facts["take"] = read_wav(path)
        facts["take_sha"] = sha(path)
        facts["doc"] = save(f"{name}.wav")
        facts["take_sha_after"] = sha(path)
        return facts

    try:
        # Preferences.
        r = c.ask("PREF")
        check("preferences start at default and off", r == "OK record_offset_ms=default monitor_input=0", r)
        for line in ("set record_offset_ms -5", "set record_offset_ms abc", "set record_offset_ms 99999", "set monitor_input maybe", "set nothing 1"):
            r = c.ask(f"PREF {line}")
            check(f"refused: PREF {line}", r.startswith("ERR"), r)
        check("a refused value changed nothing", c.ask("PREF") == "OK record_offset_ms=default monitor_input=0")
        r = c.ask("PREF set record_offset_ms 20")
        check("PREF set record_offset_ms 20", r.startswith("OK") and c.ask("PREF get record_offset_ms") == "OK record_offset_ms=20", r)
        cfg = [os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs if f == "settings.txt"]
        check("the preference was saved in the settings file", bool(cfg) and "record_offset_ms = 20" in open(cfg[0]).read(), cfg)

        c.ask(f"OPEN {src}")
        st = settle()
        check("the source opens: 6 s stereo", (st["frames"], st["channels"], st["steps"]) == (str(FRAMES), "2", "1"), st)

        # Punch-in with a 20 ms offset (960 frames at 48 kHz) and monitoring on for this take.
        OFF = 960
        sel = (2 * RATE, 3 * RATE)
        f = case("punch", sel, "mode=punch channel=both preroll=300 monitor=1")
        st, take, doc, d = f["state"], f["take"], f["doc"], f["during"]
        check("monitoring was on during the take (STATE)", d.get("mon") == "1", d.get("mon"))
        n_take = len(take[0])
        check("the punch take ran the offset longer than the selection", RATE + OFF <= n_take <= RATE + OFF + 0.2 * RATE, n_take)
        check("STATE reports the applied offset", (st["rec_offset_ms"], st["rec_offset_frames"]) == ("20", str(OFF)), (st["rec_offset_ms"], st["rec_offset_frames"]))
        n = sel[1] - sel[0]
        ok = all(doc[ch][sel[0] + MARGIN:sel[0] + n - MARGIN] == take[ch][OFF + MARGIN:OFF + n - MARGIN] for ch in range(2))
        check("punch: the placed take is shifted by the offset (doc == take[960:])", ok)
        check("punch: ... and is not the unshifted take", any(doc[0][sel[0] + MARGIN + k] != take[0][MARGIN + k] for k in range(0, 2000)))
        ok = all(doc[ch][:sel[0] - MARGIN] == orig[ch][:sel[0] - MARGIN] and doc[ch][sel[1] + MARGIN:] == orig[ch][sel[1] + MARGIN:] for ch in range(2))
        check("punch: outside the selection is sample-identical", ok and st["frames"] == str(FRAMES))
        check("punch: the take file on disk is unchanged by the placement and the save", f["take_sha"] == f["take_sha_after"])
        check("monitoring heard the input and dropped nothing", int(st["mon_played"]) > 0 and st["mon_overflow"] == "0" and st["mon_underrun"] == "0", {k: st[k] for k in st if k.startswith("mon")})
        check("a monitored take is not a different file: format and channels unchanged", len(take) == 2)

        # Insert with a 10 ms offset (480 frames), monitoring off for this take.
        c.ask("PREF set record_offset_ms 10")
        cur = 2 * RATE
        f = case("insert", (cur, cur), "mode=insert channel=both monitor=0", secs=1.2)
        st, take, doc, d = f["state"], f["take"], f["doc"], f["during"]
        OFF = 480
        n_take = len(take[0])
        n = n_take - OFF
        check("insert: monitor=0 means no monitoring", d.get("mon") == "0" and st["mon_played"] == "0", (d.get("mon"), st["mon_played"]))
        check("insert: the file grew by the take less the offset", int(st["frames"]) == FRAMES + n and len(doc[0]) == FRAMES + n, (st["frames"], n_take, OFF))
        check("insert: the offset applied is 10 ms = 480 frames", (st["rec_offset_ms"], st["rec_offset_frames"]) == ("10", "480"))
        ok = all(doc[ch][:cur - MARGIN] == orig[ch][:cur - MARGIN] and doc[ch][cur + n + MARGIN:] == orig[ch][cur + MARGIN:] for ch in range(2))
        check("insert: before the cursor and after the take is sample-identical", ok)
        check("insert: the file holds take[480:]", all(doc[ch][cur + MARGIN:cur + n - MARGIN] == take[ch][OFF + MARGIN:OFF + n - MARGIN] for ch in range(2)))
        check("insert: the take file is unchanged", f["take_sha"] == f["take_sha_after"])
        check("insert: one history step", (st["steps"], st["undo"]) == ("2", "Record"), (st["steps"], st["undo"]))

        # Offset 0 explicitly, and back to default (the fake input reports no latency, so 0).
        for label, pref in (("0", "0"), ("default", "default")):
            c.ask(f"PREF set record_offset_ms {pref}")
            f = case(f"insert-{label}", (cur, cur), "mode=insert channel=both", secs=0.8)
            st, take, doc = f["state"], f["take"], f["doc"]
            n = len(take[0])
            check(f"offset {label}: the whole take is placed", int(st["frames"]) == FRAMES + n and st["rec_offset_frames"] == "0", (st["frames"], n, st["rec_offset_frames"]))
            check(f"offset {label}: the file holds the whole take", all(doc[ch][cur + MARGIN:cur + n - MARGIN] == take[ch][MARGIN:n - MARGIN] for ch in range(2)))

        # The monitoring preference, and the per-take override over it.
        check("PREF set monitor_input 1", c.ask("PREF set monitor_input 1").startswith("OK") and c.ask("PREF get monitor_input") == "OK monitor_input=1")
        r = c.ask("RECORD start")
        time.sleep(0.8)
        d = c.state(wait=False)
        c.ask("RECORD stop")
        st = wait_over()
        check("with the preference on a plain take is monitored", r.startswith("OK") and d.get("mon") == "1" and int(st["mon_played"]) > 0, (r, d.get("mon"), st["mon_played"]))
        r = c.ask("RECORD start monitor=0")
        time.sleep(0.4)
        d = c.state(wait=False)
        c.ask("RECORD stop")
        st = wait_over()
        check("monitor=0 overrides the preference for that take", r.startswith("OK") and d.get("mon") == "0" and st["mon_played"] == "0", (r, d.get("mon")))
        c.ask("PREF set monitor_input 0")

        c.ask("QUIT discard")
        proc.wait(timeout=30)
    except Exception as e:  # report what happened instead of a traceback
        check("the run completed", False, repr(e))
    finally:
        if proc.poll() is None:
            proc.kill()

    log_files = [os.path.join(d, f) for d, _, fs in os.walk(logs) for f in fs if f.startswith("wavcraft") and f.endswith(".log")]
    lines = [l.strip() for p in log_files for l in open(p, errors="replace")]
    errors = [l[:200] for l in lines if " ERROR " in l]
    check("the app's log has no ERROR", bool(log_files) and not errors, errors[:3] or f"{len(log_files)} log file(s)")
    check("the offsets were logged", any("record offset 20 ms = 960 frames" in l for l in lines) and any("record offset 10 ms = 480 frames" in l for l in lines))
    check("monitoring was logged", any("input monitoring on (to a null sink)" in l for l in lines))
    result = {"feature": "f54 latency compensation and input monitoring", "status": "pass" if all(x["ok"] for x in checks) else "fail", "seconds": round(time.time() - t0, 1), "checks": checks}
    if a.keep or result["status"] != "pass":
        result["work"] = work
    else:
        shutil.rmtree(work, ignore_errors=True)
    print(json.dumps(result, indent=1))
    return 0 if result["status"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
