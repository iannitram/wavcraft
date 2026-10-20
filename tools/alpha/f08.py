#!/usr/bin/env python3
"""f08 scripted run: selection gestures through the control channel (GESTURE verb)."""
import argparse, math, os, shutil, socket, struct, subprocess, sys, tempfile, time, wave

PORT, RATE, SECS = 47108, 8000, 10
FRAMES = RATE * SECS


class Control:
    def __init__(self, port, token):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=30)
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

    def sel(self, line):
        answer = self.ask(line)
        for w in answer.split():
            if w.startswith("selection="):
                return w[10:]
        return answer


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f08.", dir=base)
    ok_all, results = True, []
    try:
        home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
        os.makedirs(home)
        wav = os.path.join(work, "tone.wav")
        with wave.open(wav, "wb") as w:
            w.setnchannels(1); w.setsampwidth(2); w.setframerate(RATE)
            w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(i * 0.2))) for i in range(FRAMES)))
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
                   WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token)
        err = open(os.path.join(work, "stderr.log"), "w")
        proc = subprocess.Popen([exe, wav], env=env, stdout=err, stderr=err)

        def check(name, got, want):
            nonlocal ok_all
            ok = got == want
            ok_all &= ok
            results.append(f"{'ok  ' if ok else 'FAIL'} {name}: got {got!r}, want {want!r}")

        try:
            for _ in range(40):
                if os.path.exists(token):
                    break
                time.sleep(0.25)
            c = Control(PORT, open(token).read().strip())
            for _ in range(200):
                if f"frames={FRAMES}" in c.ask("STATE") and "busy=0" in c.ask("STATE"):
                    break
                time.sleep(0.2)
            time.sleep(1)  # let the editor draw once so it knows its width
            check("select", c.sel("SELECT 20000 40000"), "20000-40000")
            check("drag start edge out", c.sel("GESTURE edge_drag 20000 10000"), "10000-40000")
            check("drag end edge out", c.sel("GESTURE edge_drag 40000 60000"), "10000-60000")
            r = c.ask("GESTURE edge_drag 35000 1000")
            check("no edge in the middle", r.startswith("ERR"), True)
            check("shift-click right", c.sel("GESTURE shift_click 70000"), "10000-70000")
            check("shift-click left", c.sel("GESTURE shift_click 2000"), "2000-70000")
            check("shift+left", c.sel("GESTURE shift_left 1000"), "1000-70000")
            check("shift+right", c.sel("GESTURE shift_right 3000"), "4000-70000")
            check("shift+home", c.sel("GESTURE shift_home"), "0-70000")
            check("shift+end", c.sel("GESTURE shift_end"), "70000-80000")
            check("shift+right from cursor", (c.ask("SELECT 5000 5000"), c.sel("GESTURE shift_right 100"))[1], "5000-5100")
            check("triple-click selects all", c.sel("GESTURE triple_click"), f"0-{FRAMES}")
            c.ask("ACTION view.zoomIn"); c.ask("ACTION view.zoomIn")
            time.sleep(1)
            vis = c.sel("GESTURE double_click")
            a, b = (int(x) for x in vis.split("-"))
            check("double-click selects the visible range (a part of the file)", 0 <= a < b < FRAMES, True)
            check("quit", c.ask("QUIT"), "OK")
            proc.wait(20)
        finally:
            if proc.poll() is None:
                proc.kill()
            err.close()
        text = open(os.path.join(work, "stderr.log"), errors="replace").read()
        for d, _, fs in os.walk(logs):
            for f in fs:
                text += open(os.path.join(d, f), errors="replace").read()
        bad = [l for l in text.splitlines() if "ERROR" in l or "panicked" in l]
        check("no ERROR line in the logs", bad[:3], [])
    finally:
        print("\n".join(results))
        shutil.rmtree(work, ignore_errors=True)
    print("PASS" if ok_all else "FAIL")
    sys.exit(0 if ok_all else 1)


main()
