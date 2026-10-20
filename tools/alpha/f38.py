#!/usr/bin/env python3
"""f38 scripted run: a synthetic WAV with bext + iXML + unknown chunks + LIST/INFO is opened in the
release app (scratch HOME, control port 47138), its metadata is read and edited through META,
the start is cut, the file is saved, and the saved file's chunks are checked with a small RIFF
reader. Fails on any ERROR line in the app's log.

    python3 tools/alpha/f38.py --exe <path to the release wavcraft>
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time

RATE, FRAMES, PORT = 48000, 96000, 47138
IXML = b"<BWFXML><PROJECT>demo</PROJECT></BWFXML>"
ODD = b"odd-sized unknown body"
TAIL = b"tail chunk after the audio!"


def chunk(cid, body):
    return cid + struct.pack("<I", len(body)) + body + (b"\0" if len(body) & 1 else b"")


def make_wav():
    bext = bytearray(602)
    bext[0:9] = b"scene one"
    bext[256:260] = b"mic1"
    bext[320:330] = b"2026-10-20"
    bext[330:338] = b"12:30:00"
    bext[338:346] = struct.pack("<Q", 5_000_000)
    bext[348] = 0xAB
    bext += b"A=PCM,F=48000,W=16,M=stereo,T=Recorder\r\n"
    info = b"INFO" + chunk(b"INAM", b"Old title\0") + chunk(b"ISFT", b"RecorderX\0")
    fmt = struct.pack("<HHIIHH", 1, 2, RATE, RATE * 4, 4, 16)
    pcm = b"".join(struct.pack("<hh", (i * 37) % 20000 - 10000, (i * 91) % 20000 - 10000) for i in range(FRAMES))
    body = b"WAVE" + chunk(b"fmt ", fmt) + chunk(b"bext", bytes(bext)) + chunk(b"iXML", IXML) + chunk(b"junk", ODD) \
        + chunk(b"LIST", info) + chunk(b"data", pcm) + chunk(b"tail", TAIL)
    return b"RIFF" + struct.pack("<I", len(body)) + body


def read_chunks(data):
    assert data[:4] == b"RIFF" and struct.unpack("<I", data[4:8])[0] == len(data) - 8, "RIFF size"
    pos, out = 12, {}
    order = []
    while pos + 8 <= len(data):
        cid, size = data[pos:pos + 4], struct.unpack("<I", data[pos + 4:pos + 8])[0]
        out[cid] = data[pos + 8:pos + 8 + size]
        order.append(cid.decode("latin1"))
        pos += 8 + size + (size & 1)
    assert pos == len(data), "chunks end at the end of the file"
    return order, out


class Control:
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

    def state(self):
        while True:
            a = self.ask("STATE")
            head, _, labels = a.partition(" undo=")
            f = dict(kv.split("=", 1) for kv in head.split()[1:])
            f["undo"], _, f["redo"] = labels.partition(" redo=")
            if f.get("busy") == "0" and f.get("frames") != "0" or a.startswith("ERR"):
                return f
            time.sleep(0.2)

    def wait_idle(self):
        while self.ask("STATE").split(" busy=")[1][0] != "0":
            time.sleep(0.2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f38.", dir=scratch_root)
    checks = []

    def check(name, ok, detail=""):
        checks.append((name, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + name + ("" if ok else f"  [{detail}]"), flush=True)

    try:
        home, logs, sessions = (os.path.join(work, d) for d in ("home", "logs", "sessions"))
        os.makedirs(home)
        src, out = os.path.join(work, "in.wav"), os.path.join(work, "out.wav")
        open(src, "wb").write(make_wav())
        token = os.path.join(work, "control.token")
        env = dict(os.environ, HOME=home, WAVCRAFT_LOG_DIR=logs, WAVCRAFT_SESSION_DIR=sessions, XDG_CONFIG_HOME=os.path.join(home, ".config"),
                   XDG_CACHE_HOME=os.path.join(home, ".cache"), WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
        proc = subprocess.Popen([exe, src], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = time.time() + 60
            while not os.path.exists(token) and time.time() < deadline:
                time.sleep(0.2)
            c = Control(open(token).read().strip())
            st = c.state()
            check("the file opens", int(st["frames"]) == FRAMES, st)
            check("bext description read", c.ask("META get description") == "OK scene one", c.ask("META get description"))
            check("bext time reference read", c.ask("META get time_reference") == "OK 5000000")
            check("INFO title read", c.ask("META get title") == "OK Old title")
            check("an unknown field is refused", c.ask("META get nope").startswith("ERR"))
            check("a bad date is refused", c.ask("META set origination_date yesterday").startswith("ERR"))
            check("a refused set makes no step", c.state()["steps"] == "1")
            check("set description", c.ask("META set description edited text") == "OK changed=1")
            st = c.state()
            check("one undoable step", st["steps"] == "2" and st["undo"] == "Edit Metadata" and st["unsaved"] == "1", st)
            check("get shows the edit", c.ask("META get description") == "OK edited text")
            c.ask("ACTION edit.undo")
            check("undo restores it", c.ask("META get description") == "OK scene one")
            c.ask("ACTION edit.redo")
            check("redo reapplies it", c.ask("META get description") == "OK edited text")
            c.ask("META set title New title")
            c.ask("META set comment line one\\r\\nline two")
            c.ask("SELECT 0 1000")
            c.ask("ACTION edit.delete")
            check("the start is cut", int(c.state()["frames"]) == FRAMES - 1000)
            check("save starts", c.ask(f"SAVE {out}") == "OK")
            c.wait_idle()
            st = c.state()
            check("saved and clean", st["unsaved"] == "0", st)
            check("the metadata still reads after the save", c.ask("META get description") == "OK edited text")
            # A second save of the same state keeps everything (chunk table follows the new file).
            c.ask("META set originator second")
            check("second save starts", c.ask(f"SAVE {out}") == "OK")
            c.wait_idle()
            check("quit", c.ask("QUIT") == "OK")
            proc.wait(timeout=30)
        finally:
            if proc.poll() is None:
                proc.kill()
        order, ch = read_chunks(open(out, "rb").read())
        check("chunk order kept", order == ["fmt ", "bext", "iXML", "junk", "LIST", "data", "tail"], order)
        check("iXML byte-identical", ch[b"iXML"] == IXML)
        check("unknown chunk byte-identical", ch[b"junk"] == ODD)
        check("chunk after the audio kept", ch[b"tail"] == TAIL)
        bx = ch[b"bext"]
        check("bext description edited", bx[:256].rstrip(b"\0") == b"edited text", bx[:20])
        check("bext originator edited by the second save", bx[256:288].rstrip(b"\0") == b"second", bx[256:290])
        check("bext time reference moved by the cut", struct.unpack("<Q", bx[338:346])[0] == 5_001_000, struct.unpack("<Q", bx[338:346]))
        check("bext UMID byte kept", bx[348] == 0xAB)
        hist = bx[602:].decode("latin1")
        check("coding history keeps the old line and adds one", hist.startswith("A=PCM,F=48000,W=16,M=stereo,T=Recorder\r\n") and hist.endswith("T=WavCraft\r\n") and hist.count("WavCraft") == 1, hist)
        info = ch[b"LIST"]
        check("INFO has the new title, the comment and the untouched ISFT", b"New title" in info and b"line one\r\nline two" in info and b"RecorderX" in info and b"Old title" not in info, info)
        check("data is the shortened audio", len(ch[b"data"]) == (FRAMES - 1000) * 4)
        log = ""
        for d, _, files in os.walk(logs):
            for f in files:
                log += open(os.path.join(d, f), errors="replace").read()
        check("no ERROR line in the app's log", "ERROR" not in log, [l for l in log.splitlines() if "ERROR" in l][:3])
        if "ERROR" in log:
            print(log[-2000:])
    finally:
        shutil.rmtree(work, ignore_errors=True)
    bad = [c for c in checks if not c[1]]
    print(f"{len(checks) - len(bad)}/{len(checks)} checks passed")
    sys.exit(1 if bad else 0)


main()
