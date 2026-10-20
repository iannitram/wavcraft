#!/usr/bin/env python3
"""f62 scripted run: a synthetic WAV with an XMP packet (known and unknown properties) in a `_PMX`
chunk is opened in the release app (scratch HOME, control port 47162). XMP get/set edit it as undoable
steps, the file is saved, and the saved file is re-read both by a small RIFF reader and by the app
itself (a second launch on the saved file). A malformed packet is reported and kept verbatim.
Fails on any ERROR line in the app's logs.

    python3 tools/alpha/f62.py --exe <path to the release wavcraft>
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time

RATE, FRAMES, PORT = 48000, 48000, 47162
PACKET = (
    "<?xpacket begin=\"﻿\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n"
    "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n"
    " <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n"
    "  <rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:acme=\"http://example.com/acme/1.0/\" acme:take=\"7\">\n"
    "   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Old title</rdf:li></rdf:Alt></dc:title>\n"
    "   <acme:mood>calm</acme:mood>\n"
    "   <acme:shape rdf:parseType=\"Resource\"><acme:w>3</acme:w></acme:shape>\n"
    "  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>"
).encode("utf-8")
BAD = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF><oops></x:xmpmeta>"
DC, ACME = "http://purl.org/dc/elements/1.1/", "http://example.com/acme/1.0/"
TAIL = b"tail chunk after the audio!"


def chunk(cid, body):
    return cid + struct.pack("<I", len(body)) + body + (b"\0" if len(body) & 1 else b"")




def make_wav(packet):
    fmt = struct.pack("<HHIIHH", 1, 2, RATE, RATE * 4, 4, 16)
    pcm = b"".join(struct.pack("<hh", (i * 37) % 20000 - 10000, (i * 91) % 20000 - 10000) for i in range(FRAMES))
    body = b"WAVE" + chunk(b"fmt ", fmt) + chunk(b"_PMX", packet) + chunk(b"data", pcm) + chunk(b"tail", TAIL)
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




class Run:
    """One app launch on a file, in its own scratch HOME."""

    def __init__(self, exe, work, name, src):
        self.home, self.logs, self.sessions = (os.path.join(work, name + "-" + d) for d in ("home", "logs", "sessions"))
        os.makedirs(self.home)
        token = os.path.join(work, name + ".token")
        env = dict(os.environ, HOME=self.home, WAVCRAFT_LOG_DIR=self.logs, WAVCRAFT_SESSION_DIR=self.sessions, XDG_CONFIG_HOME=os.path.join(self.home, ".config"),
                   XDG_CACHE_HOME=os.path.join(self.home, ".cache"), WAVCRAFT_CONTROL_PORT=str(PORT), WAVCRAFT_CONTROL_TOKEN_FILE=token, RUST_BACKTRACE="1")
        self.proc = subprocess.Popen([exe, src], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.time() + 60
        while not os.path.exists(token) and time.time() < deadline:
            time.sleep(0.2)
        self.c = Control(open(token).read().strip())

    def log(self):
        out = ""
        for d, _, files in os.walk(self.logs):
            for f in files:
                out += open(os.path.join(d, f), errors="replace").read()
        return out

    def quit(self):
        self.c.ask("QUIT discard")
        try:
            self.proc.wait(timeout=30)
        finally:
            if self.proc.poll() is None:
                self.proc.kill()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    exe = ap.parse_args().exe
    scratch_root = os.path.expanduser("~/dev/craft/_wt/scratch")
    os.makedirs(scratch_root, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f62.", dir=scratch_root)
    checks, logs = [], ""

    def check(name, ok, detail=""):
        checks.append((name, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + name + ("" if ok else f"  [{detail}]"), flush=True)

    try:
        src, out, bad = (os.path.join(work, n) for n in ("in.wav", "out.wav", "bad.wav"))
        open(src, "wb").write(make_wav(PACKET))
        open(bad, "wb").write(make_wav(BAD))

        r = Run(exe, work, "a", src)
        c = r.c
        st = c.state()
        check("the file opens", int(st["frames"]) == FRAMES, st)
        listing = c.ask("XMP get")
        check("the packet is read: status and property count", listing.startswith("OK status=ok props=4 "), listing)
        check("unknown properties are listed with ?", f"{ACME}:mood?=calm" in listing and f"{ACME}:take?=7" in listing, listing)
        check("get by prefix", c.ask("XMP get dc:title") == "OK Old title")
        check("get by namespace URI", c.ask(f"XMP get {DC}:title") == "OK Old title")
        check("a missing property is an error", c.ask("XMP get dc:rights").startswith("ERR"))
        check("a structured property is not editable", c.ask("XMP set acme:shape x").startswith("ERR"))
        check("a refused set makes no step", c.state()["steps"] == "1")
        check("set the title", c.ask("XMP set dc:title New title & more") == "OK changed=1")
        st = c.state()
        check("one undoable step", st["steps"] == "2" and st["undo"] == "Edit Metadata" and st["unsaved"] == "1", st)
        check("get shows the edit", c.ask("XMP get dc:title") == "OK New title & more")
        c.ask("ACTION edit.undo")
        check("undo restores it", c.ask("XMP get dc:title") == "OK Old title")
        c.ask("ACTION edit.redo")
        check("add a creator list", c.ask("XMP set dc:creator Ann; Bo") == "OK changed=1")
        check("edit an unknown property", c.ask("XMP set acme:mood tense") == "OK changed=1")
        check("the panel field reads the same value", c.ask("META get xmp_title") == "OK New title & more")
        check("the same value again is no change", c.ask("XMP set acme:mood tense") == "OK changed=0")
        check("save starts", c.ask(f"SAVE {out}") == "OK")
        c.wait_idle()
        check("saved and clean", c.state()["unsaved"] == "0")
        r.quit()
        logs += r.log()

        order, ch = read_chunks(open(out, "rb").read())
        check("chunk order kept", order == ["fmt ", "_PMX", "data", "tail"], order)
        check("tail chunk byte-identical", ch[b"tail"] == TAIL)
        x = ch[b"_PMX"].decode("utf-8")
        check("title edited in the file", "<rdf:li xml:lang=\"x-default\">New title &amp; more</rdf:li>" in x, x)
        check("creator added in the file", "<rdf:li>Ann</rdf:li><rdf:li>Bo</rdf:li>" in x)
        check("unknown property edited in the file", "<acme:mood>tense</acme:mood>" in x)
        check("untouched unknown content is byte-identical", "acme:take=\"7\"" in x and "<acme:shape rdf:parseType=\"Resource\"><acme:w>3</acme:w></acme:shape>" in x)
        p = PACKET.decode()
        check("wrapper kept", x.startswith(p[:p.index("<dc:title>")]) and x.endswith("<?xpacket end=\"w\"?>"))

        # The app re-reads the saved file.
        r = Run(exe, work, "b", out)
        c = r.c
        c.state()
        check("re-read: title", c.ask("XMP get dc:title") == "OK New title & more")
        check("re-read: creator", c.ask("XMP get dc:creator") == "OK Ann; Bo")
        check("re-read: unknown property", c.ask("XMP get acme:mood") == "OK tense")
        r.quit()
        logs += r.log()

        # A malformed packet is reported, not edited, and kept by a save.
        r = Run(exe, work, "c", bad)
        c = r.c
        c.state()
        listing = c.ask("XMP get")
        check("malformed: reported", listing.startswith("OK status=malformed props=0"), listing)
        check("malformed: an edit is refused", c.ask("XMP set dc:title x").startswith("ERR"))
        check("malformed: the app is alive", int(c.state()["frames"]) == FRAMES)
        c.ask("SELECT 0 1000")
        c.ask("ACTION edit.delete")
        saved = os.path.join(work, "bad-out.wav")
        check("malformed: save starts", c.ask(f"SAVE {saved}") == "OK")
        c.wait_idle()
        r.quit()
        logs += r.log()
        _, ch2 = read_chunks(open(saved, "rb").read())
        check("malformed: packet kept verbatim", ch2[b"_PMX"] == BAD, ch2[b"_PMX"])

        check("no ERROR line in the app's logs", "ERROR" not in logs, [l for l in logs.splitlines() if "ERROR" in l][:3])
        if "ERROR" in logs:
            print(logs[-2000:])
    finally:
        shutil.rmtree(work, ignore_errors=True)
    failed = [c for c in checks if not c[1]]
    print(f"{len(checks) - len(failed)}/{len(checks)} checks passed")
    sys.exit(1 if failed else 0)


main()
