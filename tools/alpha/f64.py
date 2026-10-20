#!/usr/bin/env python3
"""f64 scripted run: navigation tools (hand, zoom, ruler right-drag) through the control channel."""
import argparse, os, re, shutil, socket, struct, subprocess, sys, tempfile, time, wave, math

PORT, RATE, SECS = 47164, 48000, 20


def view(state):
    m = re.search(r"view=(\d+)-(\d+)", state)
    return int(m.group(1)), int(m.group(2))


class C:
    def __init__(self, token):
        for _ in range(80):
            try:
                self.s = socket.create_connection(("127.0.0.1", PORT), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        assert self.ask(f"TOKEN {token}") == "OK"

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f64.", dir=base)
    ok_all, proc = False, None
    try:
        home, logs = os.path.join(work, "home"), os.path.join(work, "logs")
        os.makedirs(home)
        wav = os.path.join(work, "tone.wav")
        with wave.open(wav, "wb") as w:
            w.setnchannels(1); w.setsampwidth(2); w.setframerate(RATE)
            w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 220 * i / RATE))) for i in range(RATE * SECS)))
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, WAVCRAFT_SESSION_DIR=os.path.join(work, "sessions"), WAVCRAFT_LOG_DIR=logs,
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
                   WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token)
        err = open(os.path.join(work, "stderr.log"), "w")
        proc = subprocess.Popen([exe, wav], env=env, stdout=err, stderr=err)
        for _ in range(80):
            if os.path.exists(token):
                break
            time.sleep(0.25)
        c = C(open(token).read().strip())
        checks = []

        def check(name, ok, detail=""):
            checks.append(ok)
            print(("PASS " if ok else "FAIL ") + name, detail)

        def st():
            for _ in range(150):
                s = c.ask("STATE")
                if " busy=0" in s and "frames=0 " not in s:
                    # wait for the first frame to give the editor a width: the whole file is in view
                    a, b = view(s)
                    if b >= RATE * SECS - 2:
                        return s
                time.sleep(0.2)
            return s
        s = st()
        a0, b0 = view(s)
        check("opened, whole file in view", a0 == 0 and b0 >= RATE * SECS - 2, s)
        # zoom tool
        check("tool.zoom", c.ask("ACTION tool.zoom").startswith("OK") and "tool=zoom" in c.ask("STATE"))
        r = c.ask(f"GESTURE zoom_click {RATE * 10}")
        a1, b1 = view(r)
        check("zoom click zooms in around the point", (b1 - a1) < (b0 - a0) * 0.6 and a1 < RATE * 10 < b1, r)
        r = c.ask(f"GESTURE zoom_click {RATE * 10} out")
        a2, b2 = view(r)
        check("zoom click out zooms back", (b2 - a2) > (b1 - a1) * 1.8, r)
        # hand tool
        check("tool.hand", c.ask("ACTION tool.hand").startswith("OK") and "tool=hand" in c.ask("STATE"))
        c.ask(f"GESTURE zoom_click {RATE * 10}")
        a3, b3 = view(c.ask("STATE"))
        r = c.ask("GESTURE hand_drag -200")
        a4, b4 = view(r)
        check("hand drag left scrolls later, same width", a4 > a3 and abs((b4 - a4) - (b3 - a3)) <= 2, r)
        r = c.ask("GESTURE hand_drag 100000")
        check("hand drag clamps at the start", view(r)[0] == 0, r)
        # ruler right-drag
        r = c.ask(f"GESTURE ruler_rdrag {RATE * 8} {RATE * 4}")
        a5, b5 = view(r)
        check("ruler right-drag zooms to the range", abs(a5 - RATE * 4) <= 2 and abs(b5 - RATE * 8) <= 2, r)
        check("tool.select", "tool=select" in (c.ask("ACTION tool.select") and c.ask("STATE")))
        check("bad gesture is an error", c.ask("GESTURE nope").startswith("ERR"))
        check("quit", c.ask("QUIT") == "OK")
        proc.wait(20)
        err.close()
        bad = []
        for root, _, files in os.walk(logs):
            for f in files:
                bad += [l for l in open(os.path.join(root, f), errors="replace") if "ERROR" in l]
        bad += [l for l in open(os.path.join(work, "stderr.log"), errors="replace") if "ERROR" in l or "panicked" in l]
        check("no ERROR lines in the logs", not bad, bad[:3])
        ok_all = all(checks)
    finally:
        if proc and proc.poll() is None:
            proc.kill()
        if os.environ.get("KEEP") != "1" or ok_all:
            shutil.rmtree(work, ignore_errors=True)
        else:
            print("kept", work)
    print("RESULT", "PASS" if ok_all else "FAIL")
    sys.exit(0 if ok_all else 1)


main()
