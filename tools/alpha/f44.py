#!/usr/bin/env python3
"""f44 scripted run: record into the open document (C026), punch-in (C027), record target channel (C030).

    python3 tools/alpha/f44.py --exe <wavcraft> [--keep]

Fake input only (WAVCRAFT_FAKE_INPUT=sine:440): no real input device is opened. A synthetic 6 s 48 kHz
stereo 24-bit file (quiet, so the punch-in's preroll is not loud) is opened; each case goes back to the
Open step, records, saves the document and compares it sample by sample with the original and with the
take file: lengths, that untouched regions and channels are identical, and that the recording is one
history step. Fails on any ERROR line in the app's log.
"""
import argparse, json, os, shutil, subprocess, sys, tempfile, time, wave

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from golden import Control  # noqa: E402

PORT = 47144
RATE = 48000
FRAMES = 6 * RATE
MARGIN = 600  # a little over the 5 ms (240 frame) edge crossfade; outside it audio must be identical


def scratch():
    base = os.environ.get("ALPHA_SCRATCH") or tempfile.gettempdir()
    os.makedirs(base, exist_ok=True)
    return tempfile.mkdtemp(prefix="f44.", dir=base)


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
               WAVCRAFT_FAKE_INPUT="sine:440", WAVCRAFT_RECORD_DIR=takes, STDERR=os.path.join(work, "stderr.log"), RUST_BACKTRACE="1")
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
        st = settle()
        assert r == "OK", r
        return read_wav(out)

    def wait_recording_over(secs=12):
        t = time.time()
        while time.time() - t < secs:
            if c.state(wait=False)["recording"] == "0":
                break
            time.sleep(0.1)
        return settle()

    def case(name, select, start, secs=None, edit_during=False):
        """Back to Open, select, record (stop after `secs`, or let a punch end itself), return the facts."""
        c.ask("HISTORY 0")
        settle()
        c.ask(f"SELECT {select[0]} {select[1]}")
        r = c.ask(f"RECORD start {start}")
        if not r.startswith("OK path="):
            return {"start": r}
        path = r.partition("path=")[2]
        facts = {"start": r}
        if secs is not None:
            time.sleep(secs / 2)
            facts["during"] = c.state(wait=False)
            if edit_during:
                facts["edit_during"] = c.ask("ACTION edit.delete")
                facts["history_during"] = c.ask("HISTORY 0")
            time.sleep(secs / 2)
            stop = c.ask("RECORD stop")
            facts["stop"] = stop
        else:
            facts["during"] = c.state(wait=False)
        st = wait_recording_over()
        facts["state"] = st
        facts["take"] = read_wav(path)
        facts["doc"] = save(f"{name}.wav")
        return facts

    try:
        r = c.ask("RECORD start mode=insert")
        check("recording into a file with none open is refused", r.startswith("ERR"), r)
        c.ask(f"OPEN {src}")
        st = settle()
        check("the source opens: 6 s stereo", (st["frames"], st["channels"], st["steps"]) == (str(FRAMES), "2", "1"), st)

        # Refusals before anything is recorded.
        for line, why in [("mode=insert channel=L", "insert into one channel"), ("mode=punch", "punch with no selection"),
                          ("mode=splice", "an unknown mode"), ("channel=L", "a channel without a mode")]:
            sel = (90000, 90000)
            c.ask(f"SELECT {sel[0]} {sel[1]}")
            r = c.ask(f"RECORD start {line}")
            check(f"refused: {why}", r.startswith("ERR"), r)
        st = settle()
        check("refusals recorded nothing", st["recording"] == "0" and st["steps"] == "1" and (not os.path.isdir(takes) or not os.listdir(takes)), st)

        # C026: insert at the cursor, both channels, one history step; edits are locked meanwhile.
        cur = 2 * RATE
        f = case("insert", (cur, cur), "mode=insert channel=both", secs=1.2, edit_during=True)
        st, take, doc = f["state"], f["take"], f["doc"]
        n = len(take[0])
        d = f.get("during", {})
        check("STATE shows the recording going into the file", d.get("rec_mode") == "insert" and d.get("rec_target") == "both" and d.get("rec_ch") == "2", d)
        check("an edit or undo during the recording is refused", f["edit_during"].startswith("ERR") and f["history_during"].startswith("ERR"), (f["edit_during"], f["history_during"]))
        check("insert: the take is stereo and about 1.2 s", len(take) == 2 and abs(n - 1.2 * RATE) < 0.4 * RATE, n)
        check("insert: the file grew by exactly the take", int(st["frames"]) == FRAMES + n and len(doc[0]) == FRAMES + n, (st["frames"], n))
        check("insert: one history step, labelled Record", (st["steps"], st["undo"]) == ("2", "Record"), (st["steps"], st["undo"]))
        ok = all(doc[ch][:cur - MARGIN] == orig[ch][:cur - MARGIN] and doc[ch][cur + n + MARGIN:] == orig[ch][cur + MARGIN:] for ch in range(2))
        check("insert: before the cursor and after the take is sample-identical in both channels", ok)
        check("insert: the take is in the file (both channels)", all(doc[ch][cur + MARGIN:cur + n - MARGIN] == take[ch][MARGIN:n - MARGIN] for ch in range(2)))
        check("insert: the selection is the take", st["selection"] == f"{cur}-{cur + n}", st["selection"])
        c.ask("ACTION edit.undo")
        st = settle()
        check("one undo takes the whole recording out", st["frames"] == str(FRAMES) and st["cursor"] == "0", st)
        und = save("insert-undone.wav")
        check("after undo every sample is the original", und == orig)

        # C026: replace the selection.
        sel = (RATE, 2 * RATE)
        f = case("replace", sel, "mode=replace channel=both", secs=0.6)
        st, take, doc = f["state"], f["take"], f["doc"]
        n = len(take[0])
        check("replace: the selection is swapped for the take", int(st["frames"]) == FRAMES - (sel[1] - sel[0]) + n and len(doc[0]) == int(st["frames"]), (st["frames"], n))
        check("replace: one history step labelled Record Over", (st["steps"], st["undo"]) == ("2", "Record Over"), (st["steps"], st["undo"]))
        ok = all(doc[ch][:sel[0] - MARGIN] == orig[ch][:sel[0] - MARGIN] and doc[ch][sel[0] + n + MARGIN:] == orig[ch][sel[1] + MARGIN:] for ch in range(2))
        check("replace: outside the selection is sample-identical", ok)
        check("replace: the take is in the file", all(doc[ch][sel[0] + MARGIN:sel[0] + n - MARGIN] == take[ch][MARGIN:n - MARGIN] for ch in range(2)))

        # C026 with only a cursor: overwrite from the cursor for the take's length; the file keeps its length.
        cur = 3 * RATE
        f = case("overwrite", (cur, cur), "mode=replace channel=both", secs=0.5)
        st, take, doc = f["state"], f["take"], f["doc"]
        n = len(take[0])
        check("replace at a cursor keeps the length", st["frames"] == str(FRAMES) and len(doc[0]) == FRAMES, st["frames"])
        ok = all(doc[ch][:cur - MARGIN] == orig[ch][:cur - MARGIN] and doc[ch][cur + n + MARGIN:] == orig[ch][cur + n + MARGIN:] for ch in range(2))
        check("replace at a cursor: outside the take is sample-identical", ok)
        check("replace at a cursor: the take is in the file", all(doc[ch][cur + MARGIN:cur + n - MARGIN] == take[ch][MARGIN:n - MARGIN] for ch in range(2)))

        # C030: one channel; the other is exactly the original, the length is the same.
        for name, chan, rec, other in [("left", "L", 0, 1), ("right", "R", 1, 0)]:
            cur = 2 * RATE + 1000
            f = case(f"chan-{name}", (cur, cur), f"mode=replace channel={chan}", secs=0.5)
            st, take, doc = f["state"], f["take"], f["doc"]
            n = len(take[0])
            d = f.get("during", {})
            check(f"channel {chan}: STATE shows target and a mono take", d.get("rec_target") == chan and d.get("rec_ch") == "1", d)
            check(f"channel {chan}: length unchanged, one history step", st["frames"] == str(FRAMES) and (st["steps"], st["undo"]) == ("2", "Record Over"), (st["frames"], st["steps"], st["undo"]))
            check(f"channel {chan}: the other channel is sample-identical over the whole file", doc[other] == orig[other])
            ok = doc[rec][:cur - MARGIN] == orig[rec][:cur - MARGIN] and doc[rec][cur + n + MARGIN:] == orig[rec][cur + n + MARGIN:]
            check(f"channel {chan}: the recorded channel is identical outside the take", ok)
            check(f"channel {chan}: the take is in that channel", doc[rec][cur + MARGIN:cur + n - MARGIN] == take[0][MARGIN:n - MARGIN])

        # C027: punch-in. Plays a 0.5 s preroll, records only inside the selection, stops by itself.
        sel = (2 * RATE, 3 * RATE)
        f = case("punch", sel, "mode=punch channel=both preroll=500")
        st, take, doc = f["state"], f["take"], f["doc"]
        n_take, n = len(take[0]), min(len(take[0]), sel[1] - sel[0])  # the writer may run a few ms past the limit; only the selection is used
        d = f["during"]
        check("punch: STATE shows a punch-in waiting for the selection", d.get("rec_mode") == "punch" and d.get("rec_gate") == "0" and d.get("rec_frames") == "0", d)
        check("punch: it stopped by itself at the selection's length (the take may run a few ms over)", RATE <= n_take <= 1.2 * RATE, n_take)
        check("punch: the length is unchanged and it is one history step labelled Punch In", st["frames"] == str(FRAMES) and (st["steps"], st["undo"]) == ("2", "Punch In"), (st["frames"], st["steps"], st["undo"]))
        ok = all(doc[ch][:sel[0] - MARGIN] == orig[ch][:sel[0] - MARGIN] and doc[ch][sel[0] + n + MARGIN:] == orig[ch][sel[0] + n + MARGIN:] for ch in range(2))
        check("punch: nothing outside the selection changed (sample-identical)", ok)
        check("punch: the selection holds the take", all(doc[ch][sel[0] + MARGIN:sel[0] + n - MARGIN] == take[ch][MARGIN:n - MARGIN] for ch in range(2)))
        check("punch: no recording happened before the selection", doc[0][sel[0] - 2 * MARGIN:sel[0] - MARGIN] == orig[0][sel[0] - 2 * MARGIN:sel[0] - MARGIN])

        # C027 + C030: punch one channel.
        sel = (RATE, RATE + RATE // 2)
        f = case("punch-r", sel, "mode=punch channel=R preroll=200")
        st, take, doc = f["state"], f["take"], f["doc"]
        n = min(len(take[0]), sel[1] - sel[0])
        check("punch on R: left is sample-identical over the whole file", doc[0] == orig[0])
        ok = doc[1][:sel[0] - MARGIN] == orig[1][:sel[0] - MARGIN] and doc[1][sel[0] + n + MARGIN:] == orig[1][sel[0] + n + MARGIN:]
        check("punch on R: right is identical outside the selection", ok)
        check("punch on R: right holds the take inside it", doc[1][sel[0] + MARGIN:sel[0] + n - MARGIN] == take[0][MARGIN:n - MARGIN] and st["undo"] == "Punch In", st["undo"])
        check("the app is idle afterwards", st["recording"] == "0" and st["busy"] == "0", st)

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
    check("the punch-in was logged", any("punch-in: recording from the selection's start" in l for l in lines))
    result = {"feature": "f44 record into an existing file", "status": "pass" if all(x["ok"] for x in checks) else "fail", "seconds": round(time.time() - t0, 1), "checks": checks}
    if a.keep or result["status"] != "pass":
        result["work"] = work
    else:
        shutil.rmtree(work, ignore_errors=True)
    print(json.dumps(result, indent=1))
    return 0 if result["status"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
