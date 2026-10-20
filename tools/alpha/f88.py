#!/usr/bin/env python3
"""f88 scripted run: a synthetic WAV with an iXML TRACK_LIST (plus unknown iXML content) and
LIST/INFO is opened in the release app (scratch HOME, control port 47188); channel names and
INFO tags are read and edited through CHNAME and META, the file is saved, and the saved file is
re-read with a small RIFF reader. Fails on any ERROR line in the app's log.

    python3 tools/alpha/f88.py --exe <path to the release wavcraft>
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time

RATE, FRAMES, PORT = 48000, 96000, 47188
IXML = (b"<BWFXML>\n<PROJECT>demo</PROJECT>\n<TRACK_LIST><TRACK_COUNT>2</TRACK_COUNT>\n"
        b"<TRACK><INTERLEAVE_INDEX>1</INTERLEAVE_INDEX><NAME>Boom</NAME></TRACK>\n"
        b"<TRACK><INTERLEAVE_INDEX>2</INTERLEAVE_INDEX><NAME>Lav</NAME></TRACK>\n"
        b"</TRACK_LIST>\n<KEEP_ME>  exactly </KEEP_ME>\n</BWFXML>\n")


def chunk(cid, body):
    return cid + struct.pack("<I", len(body)) + body + (b"\0" if len(body) & 1 else b"")


def make_wav():
    info = b"INFO" + chunk(b"INAM", b"Old title\0") + chunk(b"ISFT", b"RecorderX\0") + chunk(b"IKEY", b"unknown tag\0")
    fmt = struct.pack("<HHIIHH", 1, 2, RATE, RATE * 4, 4, 16)
    pcm = b"".join(struct.pack("<hh", (i * 37) % 20000 - 10000, (i * 91) % 20000 - 10000) for i in range(FRAMES))
    body = b"WAVE" + chunk(b"fmt ", fmt) + chunk(b"iXML", IXML) + chunk(b"LIST", info) + chunk(b"data", pcm)
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
    work = tempfile.mkdtemp(prefix="f88.", dir=scratch_root)
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
            check("channel names read from iXML", c.ask("CHNAME get") == "OK Boom Lav", c.ask("CHNAME get"))
            check("INFO title read", c.ask("META get title") == "OK Old title")
            check("a channel out of range is refused", c.ask("CHNAME set 3 X").startswith("ERR"))
            check("a refused set makes no step", c.state()["steps"] == "1")
            check("rename channel 2", c.ask("CHNAME set 2 Plant mic") == "OK changed=1")
            st = c.state()
            check("one undoable step", st["steps"] == "2" and st["undo"] == "Edit Metadata" and st["unsaved"] == "1", st)
            c.ask("ACTION edit.undo")
            check("undo restores the name", c.ask("CHNAME get") == "OK Boom Lav")
            c.ask("ACTION edit.redo")
            check("redo reapplies it", c.ask("CHNAME get") == "OK Boom Plant_mic")
            for tag, v in (("INAM", "New title"), ("ICMT", "a comment"), ("ICRD", "2026-10-20"), ("IGNR", "Ambient")):
                check("set INFO." + tag, c.ask(f"META set INFO.{tag} {v}") == "OK changed=1")
            check("save starts", c.ask(f"SAVE {out}") == "OK")
            c.wait_idle()
            st = c.state()
            check("saved and clean", st["unsaved"] == "0", st)
            check("quit", c.ask("QUIT") == "OK")
            proc.wait(timeout=30)
        finally:
            if proc.poll() is None:
                proc.kill()
        order, ch = read_chunks(open(out, "rb").read())
        check("chunk order kept", order == ["fmt ", "iXML", "LIST", "data"], order)
        want = IXML.replace(b"<NAME>Lav</NAME>", b"<NAME>Plant mic</NAME>")
        check("iXML: only the renamed track's name changed", ch[b"iXML"] == want, ch[b"iXML"])
        info = ch[b"LIST"]
        check("INFO has the edited tags", all(x in info for x in (b"New title", b"a comment", b"2026-10-20", b"Ambient")) and b"Old title" not in info, info)
        check("INFO keeps ISFT and the unknown tag", b"RecorderX" in info and b"IKEY" in info and b"unknown tag" in info, info)
        check("data is intact", len(ch[b"data"]) == FRAMES * 4)
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
