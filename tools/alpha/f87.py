#!/usr/bin/env python3
"""f87 scripted run: estimated size, export presets and AIFF-C, through the control channel.

    python3 tools/alpha/f87.py --exe <path to the release wavcraft> [--port 47187] [--keep]

A fresh scratch HOME; a 3 s synthetic stereo 24-bit WAV is opened. For WAV and AIFF in each sample type
`SAVEOPTS estimate` must equal the byte size of the file `SAVE` then writes; for FLAC 16/24 the estimate
must be labelled inexact and within 15 % of the real file. `SAVE x.aiff aiff=sowt` must be AIFF-C
`sowt` (ffprobe pcm_s16le; ffmpeg reads sowt as 16-bit only) and decode to the same samples as the WAV; `aiff=aifc` must be
big-endian AIFF-C. Presets: `EXPORTPRESET save`, then a new app run on the same HOME must still list and
`load` it; `delete` removes it. The app log must hold no ERROR line.
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
                          "stream=codec_name,sample_rate,channels,bits_per_raw_sample,duration_ts", "-of", "json", path],
                         capture_output=True, text=True).stdout
    s = json.loads(out or "{}").get("streams", [{}])
    return s[0] if s else {}


def pcm_md5(path, fmt):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-f", fmt, "-"], capture_output=True).stdout
    return hashlib.md5(raw).hexdigest(), len(raw)


def start(a, work, home, logs, sessions, token, src, tag):
    env = dict(os.environ, HOME=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions, XDG_CONFIG_HOME=os.path.join(home, ".config"),
               XDG_CACHE_HOME=os.path.join(home, ".cache"), RUST_BACKTRACE="1", WAVCRAFT_CONTROL_PORT=str(a.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
    err = open(os.path.join(work, f"stderr-{tag}.log"), "w")
    proc = subprocess.Popen([a.exe, src], env=env, stdout=err, stderr=err)
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
    return proc, c, err


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47187)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    base = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(base, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f87.", dir=base)
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
    errs = []
    proc = None
    try:
        proc, c, err = start(a, work, home, logs, sessions, token, src, "1")
        errs.append(err)
        st = c.state()
        check("opened the synthetic file", int(st["frames"]) == SECONDS * RATE, st)
        frames = SECONDS * RATE

        # 1. Estimated size vs the real file.
        for fmt, ext in (("wav", "wav"), ("aiff", "aiff")):
            for ty, args in (("16", "bits=16"), ("24", "bits=24"), ("32", "bits=32"), ("float", "float=1")):
                est = c.ask(f"SAVEOPTS estimate {fmt} {ty}")
                path = os.path.join(work, f"e-{ty}.{ext}")
                ans = c.ask(f"SAVE {path} {args}")
                c.state(timeout=120)
                size = os.path.getsize(path) if os.path.exists(path) else -1
                check(f"{fmt} {ty}: estimate equals the file size", ans == "OK" and est == f"OK bytes={size} exact=1", f"{est} vs {size}")
        for ty in ("16", "24"):
            est = c.ask(f"SAVEOPTS estimate flac {ty}")
            path = os.path.join(work, f"e-{ty}.flac")
            c.ask(f"SAVE {path} bits={ty}")
            c.state(timeout=120)
            size = os.path.getsize(path)
            kv = dict(x.split("=") for x in est.split()[1:]) if est.startswith("OK") else {}
            ok = kv.get("exact") == "0" and 0.85 < int(kv.get("bytes", 0)) / size < 1.15
            check(f"flac {ty}: estimate is labelled inexact and within 15% ({est} vs {size})", ok, (est, size))
        check("flac float estimate is refused", c.ask("SAVEOPTS estimate flac float").startswith("ERR"))

        # 2. AIFF kinds.
        w24 = pcm_md5(os.path.join(work, "e-16.wav"), "s16le")  # ffmpeg reads sowt as 16-bit only
        for kind, codec in (("sowt", "pcm_s16le"), ("aifc", "pcm_s16be"), ("aiff", "pcm_s16be")):
            path = os.path.join(work, f"k-{kind}.aiff")
            ans = c.ask(f"SAVE {path} bits=16 aiff={kind}")
            c.state(timeout=120)
            info = probe(path)
            raw = open(path, "rb").read(64)
            form = raw[8:12]
            want_form = b"AIFF" if kind == "aiff" else b"AIFC"
            same = pcm_md5(path, "s16le") == w24
            check(f"SAVE aiff={kind}: {codec}, FORM {want_form.decode()}, same samples as the WAV",
                  ans == "OK" and info.get("codec_name") == codec and form == want_form and same and (kind != "sowt" or b"sowt" in open(path, "rb").read(80)), f"{ans}; {info}; {form}")
        info = probe(os.path.join(work, "k-sowt.aiff"))
        check("sowt file has the saved length and rate", int(info.get("duration_ts", -1)) == frames and info.get("sample_rate") == str(RATE), info)
        check("SAVE aiff=big is refused", c.ask(f"SAVE {os.path.join(work, 'no.aiff')} aiff=big").startswith("ERR"))
        # The sowt file reopens in the app.
        ans = c.ask(f"OPEN {os.path.join(work, 'k-sowt.aiff')}")
        for _ in range(50):
            st = c.state()
            if int(st.get("frames", 0)) == frames and st.get("busy") == "0":
                break
            time.sleep(0.2)
        check("the app reopens the sowt file", ans == "OK" and int(st.get("frames", 0)) == frames, st)

        # 3. Presets: save, restart, load, delete.
        check("SAVEOPTS set aiff 16 sowt", c.ask("SAVEOPTS set aiff 16 aiff=sowt") == "OK format=aiff type=16 aiff=sowt")
        check("EXPORTPRESET save Old sampler", c.ask("EXPORTPRESET save Old sampler") == "OK format=aiff type=16 aiff=sowt")
        c.ask("SAVEOPTS set flac 16")
        check("EXPORTPRESET save Archive", c.ask("EXPORTPRESET save Archive").startswith("OK format=flac type=16"))
        check("an empty name is refused", c.ask("EXPORTPRESET save ").startswith("ERR"))
        check("QUIT", c.ask("QUIT discard") == "OK")
        proc.wait(20)
        proc, c, err = start(a, work, home, logs, sessions, token, src, "2")
        errs.append(err)
        lst = c.ask("EXPORTPRESET list")
        check("the presets are still there after a restart", lst == "OK Old sampler|Archive", lst)
        check("EXPORTPRESET load Old sampler", c.ask("EXPORTPRESET load Old sampler") == "OK format=aiff type=16 aiff=sowt")
        check("SAVEOPTS show matches", c.ask("SAVEOPTS show") == "OK format=aiff type=16 aiff=sowt")
        path = os.path.join(work, "from-preset.aiff")
        c.ask(f"SAVE {path}")
        c.state(timeout=120)
        check("a save after loading the preset is sowt", probe(path).get("codec_name") == "pcm_s16le", probe(path))
        check("EXPORTPRESET load nothing is refused", c.ask("EXPORTPRESET load Nope").startswith("ERR"))
        check("EXPORTPRESET delete Archive", c.ask("EXPORTPRESET delete Archive") == "OK" and c.ask("EXPORTPRESET list") == "OK Old sampler")
        left = [n for n in os.listdir(work) if ".wavcraft-tmp-" in n]
        check("no temporary file left behind", not left, left)
        check("QUIT again", c.ask("QUIT discard") == "OK")
        proc.wait(20)
    except Exception as e:
        check("scripted run", False, f"{type(e).__name__}: {e}")
        if proc:
            proc.kill()
    if proc and proc.poll() is None:
        proc.send_signal(signal.SIGTERM)
    for e in errs:
        e.close()
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
