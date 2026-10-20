#!/usr/bin/env python3
"""f98 scripted run (control port 47198): a synthetic WAV with an XMP packet is opened in the release
app (scratch HOME). META filter narrows the properties by name or value (known and unknown ones);
METASET fields/save/load/delete choose and name the fields shown, and a second launch with the same
HOME finds the sets again. Fails on any ERROR line in the app's logs.

    python3 tools/alpha/f98.py --exe <path to the release wavcraft>
"""
import argparse, os, shutil, socket, struct, subprocess, sys, tempfile, time

RATE, FRAMES, PORT = 48000, 48000, 47198
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
        os.makedirs(self.home, exist_ok=True)
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
    work = tempfile.mkdtemp(prefix="f98.", dir=scratch_root)
    checks, logs = [], ""

    def check(name, ok, detail=""):
        checks.append((name, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + name + ("" if ok else f"  [{detail}]"), flush=True)

    try:
        src = os.path.join(work, "in.wav")
        open(src, "wb").write(make_wav(PACKET))
        r = Run(exe, work, "a", src)
        c = r.c
        st = c.state()
        check("the file opens", int(st["frames"]) == FRAMES, st)
        c.ask("XMP set dc:creator Ann Writer")
        a = c.ask("META filter calm")
        check("filter by value finds an unknown property", a.startswith("OK 1 ") and ":mood" in a, a)
        a = c.ask("META filter OLD TITLE")
        check("filter by value, case-insensitive", a.startswith("OK 1 ") and "xmp_title" in a, a)
        a = c.ask("META filter creator")
        check("filter by name", "xmp_creator" in a, a)
        check("no match gives none", c.ask("META filter zzz-none") == "OK 0")
        check("empty filter gives all", int(c.ask("META filter").split()[1]) >= 18)
        check("the filter made no history step", c.state()["steps"] == "2")
        show = "title,xmp_creator"
        check("choose the fields shown", c.ask("METASET fields " + show) == "OK")
        check("list shows them", c.ask("METASET list").endswith("shown=" + show), c.ask("METASET list"))
        check("save a named set", c.ask("METASET save Tags only") == "OK")
        check("show all again", c.ask("METASET fields all") == "OK")
        check("load the set", c.ask("METASET load Tags only") == "OK" and c.ask("METASET list").endswith("shown=" + show))
        check("unknown field refused", c.ask("METASET fields nope").startswith("ERR"))
        check("unknown set refused", c.ask("METASET load nope").startswith("ERR"))
        check("save needs a name", c.ask("METASET save ").startswith("ERR"))
        check("a second set", c.ask("METASET fields all") == "OK" and c.ask("METASET save Everything") == "OK")
        r.quit()
        logs += r.log()
        r = Run(exe, work, "a", src)
        c = r.c
        c.state()
        a = c.ask("METASET list")
        check("sets persist across launches", "Everything" in a and "Tags_only" in a, a)
        check("load after relaunch", c.ask("METASET load Tags only") == "OK" and c.ask("METASET list").endswith("shown=" + show))
        check("delete a set", c.ask("METASET delete Everything") == "OK" and "Everything" not in c.ask("METASET list"))
        r.quit()
        logs += r.log()
        r = Run(exe, work, "a", src)
        c = r.c
        c.state()
        check("delete persists", "Everything" not in c.ask("METASET list"))
        r.quit()
        logs += r.log()
        check("no ERROR line in the app's logs", "ERROR" not in logs, [l for l in logs.splitlines() if "ERROR" in l][:3])
        if "ERROR" in logs:
            print(logs[-2000:])
    finally:
        shutil.rmtree(work, ignore_errors=True)
    failed = [c for c in checks if not c[1]]
    print(f"{len(checks) - len(failed)}/{len(checks)} checks passed")
    sys.exit(1 if failed else 0)


main()
