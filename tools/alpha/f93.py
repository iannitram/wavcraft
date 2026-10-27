#!/usr/bin/env python3
"""f93 scripted run: Save As Ogg Vorbis with a VBR quality, through the control channel.

    python3 tools/alpha/f93.py --exe <path to the release wavcraft> [--port 47193] [--keep]

A fresh scratch HOME; a 3 s synthetic stereo 24-bit WAV is opened, edited (Delete 1 s), then saved as
.ogg at quality -1, 5 and 10 (the quality stays set for later saves, like the sample type). Each file is checked with ffprobe (vorbis, rate, channels,
length within one block of the document's) and its size must grow with the quality; the app reopens
the quality-5 file at the saved length; quality=11, quality=loud and a bad extension are refused and
leave no file and no temporary file; the app log must hold no ERROR line.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, sys, tempfile, time

RATE, SECONDS = 48000, 3


class Control:
    def __init__(self, port, token, tries=80):
        for _ in range(tries):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.25)
        else:
            raise RuntimeError(f"no control channel on port {port}")
        self.f = self.s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self, timeout=60):
        t0 = time.time()
        while True:
            head = self.ask("STATE").partition(" undo=")[0]
            d = dict(kv.split("=", 1) for kv in head.split()[1:])
            if d.get("busy") == "0" or time.time() - t0 > timeout:
                return d
            time.sleep(0.1)


def probe(path):
    out = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries",
                          "stream=codec_name,sample_rate,channels,duration_ts", "-of", "json", path],
                         capture_output=True, text=True).stdout
    s = json.loads(out or "{}").get("streams", [{}])
    return s[0] if s else {}


def pcm_md5(path, fmt):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-f", fmt, "-"], capture_output=True).stdout
    return hashlib.md5(raw).hexdigest(), len(raw)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47193)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f93.", dir=base)
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"), flush=True)

    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    src = os.path.join(work, "src.wav")
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", f"sine=frequency=440:sample_rate={RATE}:duration={SECONDS}",
                    "-f", "lavfi", "-i", f"anoisesrc=d={SECONDS}:c=pink:r={RATE}:a=0.05", "-filter_complex",
                    "[0][1]amix=inputs=2,aformat=channel_layouts=stereo", "-c:a", "pcm_s24le", src], check=True)
    token = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions, XDG_CONFIG_HOME=os.path.join(home, ".config"),
               XDG_CACHE_HOME=os.path.join(home, ".cache"), RUST_BACKTRACE="1", WAVCRAFT_CONTROL_PORT=str(a.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([a.exe, src], env=env, stdout=err, stderr=err)
    try:
        for _ in range(80):
            if os.path.exists(token):
                break
            time.sleep(0.25)
        c = Control(a.port, open(token).read().strip())
        for _ in range(100):
            st = c.state()
            if int(st.get("frames", 0)) == SECONDS * RATE:
                break
            time.sleep(0.2)
        check("opened the synthetic file", int(st["frames"]) == SECONDS * RATE, st)
        c.ask(f"SELECT {RATE} {2 * RATE}")
        c.ask("ACTION edit.delete")
        frames = (SECONDS - 1) * RATE
        check("Delete 1 s", int(c.state()["frames"]) == frames, c.state())

        sizes = {}
        for name, args in [("q-1.ogg", "quality=-1"), ("q5.ogg", "quality=5"), ("q10.ogg", "quality=10")]:
            path = os.path.join(work, name)
            ans = c.ask(f"SAVE {path} {args}".strip())
            st = c.state(timeout=120)
            info = probe(path) if os.path.exists(path) else {}
            dur = int(info.get("duration_ts", -1))
            ok = ans == "OK" and info.get("codec_name") == "vorbis" and info.get("sample_rate") == str(RATE) \
                and str(info.get("channels")) == "2" and abs(dur - frames) <= 2048
            sizes[name] = os.path.getsize(path) if os.path.exists(path) else 0
            check(f"Save As {name} ({args or 'default quality'})", ok, f"{ans}; {st}; {info}; wanted ~{frames} frames")
        check("file size grows with quality", sizes["q-1.ogg"] < sizes["q5.ogg"] < sizes["q10.ogg"], sizes)
        for name, args in [("no.ogg", "quality=11"), ("no.ogg", "quality=loud"), ("no.mp3", "quality=5")]:
            path = os.path.join(work, name)
            ans = c.ask(f"SAVE {path} {args}")
            check(f"SAVE {name} {args} is refused", ans.startswith("ERR") and not os.path.exists(path), ans)
        left = [n for n in os.listdir(work) if ".wavcraft-tmp-" in n]
        check("no temporary file left behind", not left, left)
        ans = c.ask(f"OPEN {os.path.join(work, 'q5.ogg')}")
        st = c.state(timeout=120)
        for _ in range(50):
            if abs(int(st.get("frames", 0)) - frames) <= 2048 and st.get("busy") == "0":
                break
            time.sleep(0.2)
            st = c.state()
        check("the app reopens q5.ogg", ans == "OK" and abs(int(st.get("frames", 0)) - frames) <= 2048 and st.get("channels") == "2", f"{ans}; {st}")
        check("QUIT", c.ask("QUIT discard") == "OK")
        proc.wait(20)
    except Exception as e:
        check("scripted run", False, f"{type(e).__name__}: {e}")
        proc.kill()
    if proc.poll() is None:
        proc.send_signal(signal.SIGTERM)
    err.close()
    text = ""
    for d, _, files in os.walk(work):
        for n in files:
            if n.endswith(".log"):
                text += open(os.path.join(d, n), errors="replace").read()
    bad = [l for l in text.splitlines() if "ERROR" in l or "panicked" in l]
    check("no ERROR line in the app's log or stderr", not bad, bad[:5])
    ok = all(x[1] for x in checks)
    print("RESULT", "PASS" if ok else "FAIL", f"{sum(x[1] for x in checks)}/{len(checks)}", work if a.keep or not ok else "")
    if not a.keep and ok:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(0 if ok else 1)


main()
