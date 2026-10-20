#!/usr/bin/env python3
"""Memory of the app on one long synthetic file, for the f05 before/after comparison.

    python3 tools/alpha/f05-mem.py --exe <wavcraft> --wav <file> --port 47105

Opens the file in a scratch HOME, and reports macOS `vmmap -summary` physical footprint (now and
peak) and `ps` resident size: after the open, after playing 4 s, and after Delete of 10 minutes.
Prints one JSON line.
"""
import argparse, json, os, re, shutil, subprocess, tempfile, time, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from f05 import Control, rss_mb, RATE


def mb(text):
    m = re.match(r"([\d.]+)([KMG]?)", text.strip())
    v = float(m.group(1))
    return v * {"": 1 / 1048576, "K": 1 / 1024, "M": 1, "G": 1024}[m.group(2)]


def footprint(pid):
    out = subprocess.run(["vmmap", "-summary", str(pid)], capture_output=True, text=True).stdout
    now = re.search(r"Physical footprint:\s+(\S+)", out)
    peak = re.search(r"Physical footprint \(peak\):\s+(\S+)", out)
    return {"footprint_mb": round(mb(now.group(1))) if now else None, "peak_mb": round(mb(peak.group(1))) if peak else None}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--wav", required=True)
    ap.add_argument("--port", type=int, default=47105)
    o = ap.parse_args()
    work = tempfile.mkdtemp(prefix="f05mem.", dir=os.path.expanduser("~/dev/craft/_wt/scratch"))
    out = {}
    try:
        home = os.path.join(work, "home")
        os.makedirs(home)
        token = os.path.join(work, "t")
        env = dict(os.environ, HOME=home, WAVCRAFT_SESSION_DIR=os.path.join(work, "s"), WAVCRAFT_LOG_DIR=os.path.join(work, "l"),
                   XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
                   WAVCRAFT_CONTROL_PORT=str(o.port), WAVCRAFT_CONTROL_TOKEN_FILE=token)
        proc = subprocess.Popen([o.exe, o.wav], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(120):
            if os.path.exists(token):
                break
            time.sleep(0.25)
        c = Control(o.port, open(token).read().strip())
        t0 = time.time()
        st = c.state()
        out["open_s"] = round(time.time() - t0, 1)
        out["frames"] = int(st["frames"])
        time.sleep(3)
        out["after_open"] = dict(footprint(proc.pid), rss_mb=round(rss_mb(proc.pid)))
        c.ask(f"PLAY 1000000 {int(st['frames'])}")
        time.sleep(4)
        c.ask("PLAY 0 0")
        out["after_play"] = dict(footprint(proc.pid), rss_mb=round(rss_mb(proc.pid)))
        c.ask(f"SELECT {10 * 60 * RATE} {20 * 60 * RATE}")
        c.ask("ACTION edit.delete")
        t1 = time.time()
        st2 = c.state()
        out["edit_s"] = round(time.time() - t1, 1)
        out["frames_after_edit"] = int(st2["frames"])
        time.sleep(3)
        out["after_edit"] = dict(footprint(proc.pid), rss_mb=round(rss_mb(proc.pid)))
        c.ask("QUIT discard")
        proc.wait(30)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    print(json.dumps(out))


if __name__ == "__main__":
    main()
