#!/usr/bin/env python3
"""f91 scripted run: Import Raw Data and the Wave64 / CAF / AU round trip, through the control channel.

    python3 tools/alpha/f91.py --exe <path to the release wavcraft>

Makes a headerless 16-bit little-endian stereo file (with a junk header to skip), then:
IMPORT_RAW it, checks STATE (frames, channels, rate); SAVE as .w64, .caf and .au, checks each
with ffprobe and checks that ffmpeg decodes the saved file to exactly the original payload bytes;
OPEN each saved file and checks STATE again; QUIT; fails on any ERROR line in the app's log.
"""
import argparse, hashlib, json, math, os, shutil, signal, socket, struct, subprocess, sys, tempfile, time

RATE, CH, SECS, OFFSET = 48000, 2, 2, 100


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Control:
    def __init__(self, port, token_file):
        for _ in range(80):
            if os.path.exists(token_file):
                break
            time.sleep(0.25)
        token = open(token_file).read().strip()
        for _ in range(60):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
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
                          "stream=codec_name,sample_rate,channels", "-show_entries", "format=format_name",
                          "-of", "json", path], capture_output=True, text=True).stdout
    j = json.loads(out or "{}")
    return (j.get("streams") or [{}])[0], j.get("format", {}).get("format_name", "")


def decode_md5(path):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-f", "s16le", "-acodec", "pcm_s16le", "-"], capture_output=True).stdout
    return hashlib.md5(raw).hexdigest(), len(raw)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    o = ap.parse_args()
    for t in ("ffmpeg", "ffprobe"):
        if not shutil.which(t):
            raise SystemExit(f"{t} is needed on PATH")
    scratch = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f91.", dir=scratch)
    home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
    os.makedirs(home)
    checks = []

    def check(label, ok, detail=""):
        checks.append({"check": label, "ok": bool(ok), "detail": str(detail)})

    payload = b"".join(struct.pack("<hh", int(16000 * math.sin(i * 0.05)), int(-12000 * math.sin(i * 0.031))) for i in range(RATE * SECS))
    raw = os.path.join(work, "capture.bin")
    open(raw, "wb").write(b"\x99" * OFFSET + payload)
    want_md5 = hashlib.md5(payload).hexdigest()
    port, token = free_port(), os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, WAVCRAFT_SESSION_DIR=sessions, WAVCRAFT_LOG_DIR=logs,
               XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               WAVCRAFT_CONTROL_PORT=str(port), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
    err = open(os.path.join(work, "stderr.log"), "w")
    proc = subprocess.Popen([o.exe], env=env, stdout=err, stderr=err)
    try:
        c = Control(port, token)
        a = c.ask(f"IMPORT_RAW {raw} rate={RATE} ch={CH} fmt=s16le offset={OFFSET}")
        st = c.state()
        check("IMPORT_RAW opens the headerless file", a == "OK" and st.get("frames") == str(RATE * SECS) and st.get("channels") == "2" and st.get("rate") == str(RATE), f"{a}; {st}")
        bad = c.ask(f"IMPORT_RAW {raw} rate=0 ch=2 fmt=s16le")
        check("a bad setting answers ERR from the worker's error, the document stays", bad == "OK" and c.state().get("frames") == str(RATE * SECS) or bad.startswith("ERR"), bad)
        for ext, codec_start, fmt_hint in (("w64", "pcm_s16le", "w64"), ("caf", "pcm_s16be", "caf"), ("au", "pcm_s16be", "au")):
            out = os.path.join(work, f"saved.{ext}")
            a = c.ask(f"SAVE {out}")
            st = c.state()
            check(f"SAVE .{ext} answers OK", a == "OK" and st.get("unsaved") == "0", f"{a}; {st}")
            stream, fmt = probe(out)
            check(f"ffprobe reads the .{ext}", stream.get("codec_name") == codec_start and stream.get("sample_rate") == str(RATE) and stream.get("channels") == 2 and fmt_hint in fmt,
                  f"{stream} format={fmt}")
            md5, n = decode_md5(out)
            check(f"ffmpeg decodes the .{ext} to the original samples", md5 == want_md5 and n == len(payload), f"{n} bytes, md5 {md5} vs {want_md5}")
            a = c.ask(f"OPEN {out}")
            st = c.state()
            check(f"reopening the .{ext}", a == "OK" and st.get("frames") == str(RATE * SECS) and st.get("channels") == "2" and st.get("rate") == str(RATE), f"{a}; {st}")
        quit_ok = c.ask("QUIT") == "OK"
        proc.wait(20)
        check("quits through the control channel", quit_ok and proc.returncode == 0, f"exit {proc.returncode}")
    except Exception as e:
        check("control channel", False, f"{type(e).__name__}: {e}")
    finally:
        if proc.poll() is None:
            proc.send_signal(signal.SIGTERM)
            try:
                proc.wait(10)
            except subprocess.TimeoutExpired:
                proc.kill()
    err.close()
    bad_lines = []
    for d in (logs, work):
        for root, _, files in os.walk(d):
            for f in files:
                if f.endswith((".log", ".txt")):
                    for line in open(os.path.join(root, f), errors="replace"):
                        if " ERROR " in line or line.startswith("ERROR") or "panicked" in line:
                            bad_lines.append(line.strip())
    check("no ERROR line in the app's log or stderr", not bad_lines, bad_lines[:5])
    ok = all(x["ok"] for x in checks)
    print(json.dumps({"status": "pass" if ok else "fail", "checks": checks}, indent=1))
    if ok:
        shutil.rmtree(work, ignore_errors=True)
    else:
        print("kept for inspection:", work, file=sys.stderr)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
