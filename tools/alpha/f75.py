#!/usr/bin/env python3
"""A134 scripted run: a generated 6-channel 5.1 WAV (WAVE_FORMAT_EXTENSIBLE, mask 0x3F) is
opened in the release app; STATE must report 6 channels with the 5.1 labels; an edit on three
channels through CHANNELS + ACTION; SAVE As; the saved file must hold the same channel count
and mask, and the untouched channels must be sample-identical. Fails on any ERROR in the log.

    python3 tools/alpha/f75.py --exe <path to wavcraft> [--port 47175]
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time

RATE, FRAMES, CH, MASK = 48000, 48000 * 3, 6, 0x3F
PORT = 47175


def sample(c, k):
    return ((k * 31 + c * 977) % 65536) - 32768  # 16-bit exact


def write_wav(path):
    data = bytearray()
    for k in range(FRAMES):
        for c in range(CH):
            data += struct.pack("<h", sample(c, k))
    fmt = struct.pack("<HHIIHHHHI", 0xFFFE, CH, RATE, RATE * CH * 2, CH * 2, 16, 22, 16, MASK)
    fmt += struct.pack("<H", 1) + bytes([0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71])
    body = b"WAVE" + b"fmt " + struct.pack("<I", len(fmt)) + fmt + b"data" + struct.pack("<I", len(data)) + bytes(data)
    with open(path, "wb") as f:
        f.write(b"RIFF" + struct.pack("<I", len(body)) + body)


def read_wav(path):
    b = open(path, "rb").read()
    pos, info, data = 12, None, None
    while pos + 8 <= len(b):
        cid, size = b[pos:pos + 4], struct.unpack("<I", b[pos + 4:pos + 8])[0]
        body = b[pos + 8:pos + 8 + size]
        if cid == b"fmt ":
            info = dict(tag=struct.unpack("<H", body[0:2])[0], ch=struct.unpack("<H", body[2:4])[0],
                        mask=struct.unpack("<I", body[20:24])[0] if len(body) >= 24 else None)
        elif cid == b"data":
            data = body
        pos += 8 + size + (size & 1)
    return info, struct.unpack("<%dh" % (len(data) // 2), data)


class Control:
    def __init__(self, port, token):
        for _ in range(120):
            try:
                self.s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = self.s.makefile("rw", encoding="utf-8")
        assert self.ask(f"TOKEN {token}") == "OK", "token refused"

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def state(self, timeout=60):
        t0 = time.time()
        while True:
            a = self.ask("STATE")
            head, _, tail = a.partition(" undo=")
            d = dict(kv.split("=", 1) for kv in head.split()[1:])
            d["undo"] = tail
            if d.get("busy") == "0" or time.time() - t0 > timeout:
                return d
            time.sleep(0.2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=PORT)
    args = ap.parse_args()
    root = tempfile.mkdtemp(prefix="f75.", dir=os.environ.get("WAVCRAFT_SCRATCH"))
    fails = []

    def check(label, ok, detail=""):
        print(("PASS " if ok else "FAIL ") + label + (f"  [{detail}]" if detail else ""))
        if not ok:
            fails.append(label)

    proc = None
    try:
        home, logs, sess = (os.path.join(root, n) for n in ("home", "logs", "sessions"))
        for d in (home, logs, sess):
            os.makedirs(d)
        src, out = os.path.join(root, "surround.wav"), os.path.join(root, "saved.wav")
        write_wav(src)
        token_file = os.path.join(root, "control.token")
        env = dict(os.environ, HOME=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sess,
                   WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token_file)
        proc = subprocess.Popen([args.exe], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        for _ in range(120):
            if os.path.exists(token_file) and os.path.getsize(token_file) >= 64:
                break
            time.sleep(0.25)
        c = Control(args.port, open(token_file).read().strip())
        c.ask(f"OPEN {src}")
        st = c.state()
        check("opens 6 channels", st["channels"] == "6" and st["frames"] == str(FRAMES), str(st))
        check("labels come from the mask", st["labels"] == "FL,FR,FC,LFE,BL,BR", st["labels"])
        check("all channels active at first", st["active"] == "0,1,2,3,4,5", st["active"])
        check("CHANNELS rejects out of range", c.ask("CHANNELS 9").startswith("ERR"))
        check("CHANNELS 1,3,4", c.ask("CHANNELS 1,3,4") == "OK")
        st = c.state()
        check("STATE shows the active channels", st["active"] == "1,3,4", st["active"])
        lo, hi = FRAMES // 4, FRAMES // 2
        c.ask(f"SELECT {lo} {hi}")
        a = c.ask("ACTION edit.silence")
        check("silence on three channels", a.endswith("changed=1"), a)
        check("SAVE As", c.ask(f"SAVE {out}") == "OK")
        st = c.state()
        check("saved", st["unsaved"] == "0" and os.path.exists(out), str(st))
        info, data = read_wav(out)
        check("saved file: 6 channels, extensible, mask 0x3F", info == dict(tag=0xFFFE, ch=6, mask=MASK), str(info))
        bad, margin = [], 480  # the app's 5 ms edge crossfade (240 frames) shapes the range's edges
        for c_ in range(CH):
            for k in range(FRAMES):
                if c_ in (1, 3, 4):
                    if lo - margin < k < lo + margin or hi - margin < k < hi + margin:
                        continue
                    want = 0 if lo <= k < hi else sample(c_, k)
                else:
                    want = sample(c_, k)
                if data[k * CH + c_] != want:
                    bad.append((c_, k))
                    break
        check("untouched channels identical, edited ones silent in the range", not bad, str(bad))
        # Reopen the saved file: the labels are still there.
        c.ask(f"OPEN {out}")
        st = c.state()
        check("reopened: 6 channels, labels", st["channels"] == "6" and st["labels"] == "FL,FR,FC,LFE,BL,BR", str(st))
        check("QUIT", c.ask("QUIT discard") == "OK")
        proc.wait(timeout=30)
        log_text = ""
        for dp, _, fs in os.walk(logs):
            for f in fs:
                log_text += open(os.path.join(dp, f), errors="replace").read()
        errs = [l for l in log_text.splitlines() if "ERROR" in l]
        check("no ERROR line in the app's log", not errs, "; ".join(errs[:3]))
    finally:
        if proc and proc.poll() is None:
            proc.kill()
        shutil.rmtree(root, ignore_errors=True)
    print("RESULT", "FAIL" if fails else "PASS")
    sys.exit(1 if fails else 0)


main()
