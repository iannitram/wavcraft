#!/usr/bin/env python3
"""f63 scripted run: Open Recent, File > Import > File, and the Files panel's metadata columns.

    python3 tools/alpha/f63.py --exe <wavcraft> [--port 47163] [--scratch DIR]

Three runs of the app share one HOME (so the config folder carries over):
  1. opens twelve synthetic files; the recent list holds the last ten, newest first.
  2. restarts: the list is the same. One file is deleted: it shows as missing, and opening it
     says so and drops it. Opening another entry moves it to the front. Imports a file with a
     title: it is listed (imported=1, not current), the current document and the document count
     are unchanged, and no session store was made for it. Duplicates and non-audio are refused.
     Chooses the columns (title, size, name).
  3. restarts: the column choice is still there; selecting the imported row opens it (a session
     store appears only now); Clear Recent empties the list and the file.
The app's log must hold no ERROR line. HOME, the session folder, the log folder and the token
file are inside a fresh scratch folder, removed at the end.
"""
import argparse, glob, math, os, shutil, socket, struct, subprocess, sys, tempfile, time


def write_wav(path, seconds=0.2, rate=22050, title=None):
    n = int(rate * seconds)
    data = b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / rate))) for i in range(n))
    fmt = struct.pack("<HHIIHH", 1, 1, rate, rate * 2, 2, 16)
    chunks = [b"fmt " + struct.pack("<I", len(fmt)) + fmt]
    if title:
        t = title.encode() + b"\0"
        t += b"\0" * (len(t) % 2)
        info = b"INFO" + b"INAM" + struct.pack("<I", len(title) + 1) + t
        chunks.append(b"LIST" + struct.pack("<I", len(info)) + info)
    chunks.append(b"data" + struct.pack("<I", len(data)) + data)
    body = b"WAVE" + b"".join(chunks)
    with open(path, "wb") as f:
        f.write(b"RIFF" + struct.pack("<I", len(body)) + body)


class App:
    def __init__(self, exe, env, port, token_file, work, name):
        self.err = open(os.path.join(work, f"stderr-{name}.log"), "w")
        self.proc = subprocess.Popen([exe], env=env, stdout=self.err, stderr=self.err)
        for _ in range(120):
            token = open(token_file).read().strip() if os.path.exists(token_file) else ""
            try:
                s = socket.create_connection(("127.0.0.1", port), timeout=60)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("no control channel")
        self.f = s.makefile("rw", encoding="utf-8")
        if self.ask(f"TOKEN {token}") != "OK":
            raise RuntimeError("token refused")

    def ask(self, line):
        self.f.write(line + "\n")
        self.f.flush()
        return self.f.readline().strip()

    def idle(self):
        for _ in range(200):
            if " busy=0" in self.ask("STATE"):
                return
            time.sleep(0.2)
        raise RuntimeError("still busy")

    def recent(self):
        head, *rows = [x.strip() for x in self.ask("RECENT").split(" | ")]
        out = []
        for r in rows:
            pre, _, path = r.partition(" path=")
            out.append((int(pre.split()[0]), pre.split("exists=")[1] == "1", path))
        return int(head.split("count=")[1]), out

    def files(self):
        head, *rows = [x.strip() for x in self.ask("FILES").split(" | ")]
        h = dict(kv.split("=", 1) for kv in head.split()[1:] if "=" in kv)
        out = []
        for r in rows:
            r, _, title = r.partition(" title=")
            pre, _, rest = r.partition(" name=")
            name, _, path = rest.partition(" path=")
            d = dict(kv.split("=", 1) for kv in pre.split()[1:])
            d.update(name=name, path=path, title=title)
            out.append(d)
        return h, out

    def quit(self):
        self.ask("QUIT discard")
        try:
            return self.proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--port", type=int, default=47163)
    ap.add_argument("--scratch", default=os.path.expanduser("~/dev/craft/_wt/scratch"))
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    os.makedirs(args.scratch, exist_ok=True)
    work = tempfile.mkdtemp(prefix="f63.", dir=args.scratch)
    home, sessions, logs = (os.path.join(work, d) for d in ("home", "sessions", "logs"))
    os.makedirs(home)
    token_file = os.path.join(work, "control.token")
    env = dict(os.environ, HOME=home, USERPROFILE=home, APPDATA=os.path.join(home, "AppData"), WAVCRAFT_SESSION_DIR=sessions,
               WAVCRAFT_LOG_DIR=logs, XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_CACHE_HOME=os.path.join(home, ".cache"),
               RUST_BACKTRACE="1", WAVCRAFT_CONTROL_PORT=str(args.port), WAVCRAFT_CONTROL_TOKEN_FILE=token_file)
    paths = []
    for i in range(12):
        p = os.path.join(work, f"take{i:02d}.wav")
        write_wav(p, 0.2 + 0.01 * i)
        paths.append(p)
    imp = os.path.join(work, "ambience.wav")
    write_wav(imp, 1.5, 44100, title="Rain on a roof")
    junk = os.path.join(work, "notes.wav")
    open(junk, "wb").write(b"this is not audio")
    checks = []

    def check(label, ok, detail=""):
        checks.append((label, bool(ok), detail))
        print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"  [{detail}]"), flush=True)

    def stores():
        return sorted(d for d in os.listdir(sessions)) if os.path.isdir(sessions) else []

    app = None
    try:
        # ---- run 1: twelve opens, ten remembered
        app = App(args.exe, env, args.port, token_file, work, "1")
        app.idle()
        check("a fresh start has an empty recent list", app.recent() == (0, []), app.recent())
        for p in paths:
            app.ask(f"OPEN {p}")
            app.idle()
        n, rows = app.recent()
        want = list(reversed(paths))[:10]
        check("the list keeps the last ten, newest first", n == 10 and [r[2] for r in rows] == want, (n, [os.path.basename(r[2]) for r in rows]))
        check("the oldest two files fell off", paths[0] not in [r[2] for r in rows] and paths[1] not in [r[2] for r in rows])
        check("the app exits", app.quit() == 0)

        # ---- run 2: the list survives a restart; a missing file; an import
        app = App(args.exe, env, args.port, token_file, work, "2")
        app.idle()
        n2, rows2 = app.recent()
        check("the recent list is the same after a restart", n2 == 10 and [r[2] for r in rows2] == want, (n2, [os.path.basename(r[2]) for r in rows2]))
        check("nothing is open after the restart", app.files()[0]["total"] == "0")
        gone = want[3]
        os.remove(gone)
        n3, rows3 = app.recent()
        check("a deleted file shows as missing", [r[1] for r in rows3] == [i != 3 for i in range(10)], rows3)
        r = app.ask("RECENT open 3")
        check("opening a missing entry says so and drops it", r.startswith("ERR") and "missing" in r and "removed" in r, r)
        n4, rows4 = app.recent()
        check("the missing entry is off the list", n4 == 9 and gone not in [x[2] for x in rows4], (n4, rows4))
        check("opening past the end is an error", app.ask("RECENT open 40").startswith("ERR"))
        third = rows4[2][2]
        r = app.ask("RECENT open 2")
        app.idle()
        n5, rows5 = app.recent()
        check("opening an entry opens it and moves it to the front", r.startswith("OK") and rows5[0][2] == third and n5 == 9, (r, rows5[:2]))
        h, frows = app.files()
        check("the file is open and current", h["total"] == "1" and frows[0]["path"] == third and frows[0]["current"] == "1", (h, frows))

        before_state, before_stores = app.ask("STATE"), stores()
        before_docs = app.ask("DOCS")
        r = app.ask(f"IMPORT {imp}")
        check("IMPORT answers OK", r == "OK listed=1 open=1", r)
        h, frows = app.files()
        row = next((x for x in frows if x["path"] == imp), None)
        check("the imported file is listed after the open one", h["total"] == "2" and row is not None and frows[-1] is row, (h, frows))
        check("it is marked imported and is not current", row and row["imported"] == "1" and row["current"] == "0" and h["current"] == "0", (h, row))
        check("its header was read (length, rate, channels, bits)", row and (row["frames"], row["rate"], row["ch"], row["bits"], row["format"]) == ("66150", "44100", "1", "16", "Wav"), row)
        check("its size on disk is shown", row and row["size"] == str(os.path.getsize(imp)), row)
        check("its INFO title is read", row and row["title"] == "Rain on a roof", row)
        check("the editor still shows the current document", app.ask("STATE") == before_state)
        check("DOCS does not list it (it is not open)", app.ask("DOCS") == before_docs, (app.ask("DOCS"), before_docs))
        check("no session store was made for it", stores() == before_stores, (before_stores, stores()))
        check("importing it again is refused", app.ask(f"IMPORT {imp}").startswith("ERR"))
        check("importing the open file is refused", app.ask(f"IMPORT {third}").startswith("ERR"))
        check("importing a file that is not audio is refused", app.ask(f"IMPORT {junk}").startswith("ERR"))
        check("importing a missing file is refused", app.ask(f"IMPORT {work}/nope.wav").startswith("ERR"))
        check("an import is not put on the recent list", imp not in [x[2] for x in app.recent()[1]])

        r = app.ask("FILES columns")
        check("by default the metadata columns are hidden", r == "OK columns=name,unsaved,duration,rate,channels,bits,format,path", r)
        check("FILES columns refuses an unknown column", app.ask("FILES columns name,colour").startswith("ERR"))
        r = app.ask("FILES columns title,size,name")
        check("FILES columns sets the shown columns, in order", r == "OK columns=title,size,name", r)
        check("the table header reports them", app.files()[0]["columns"] == "title,size,name")
        app.ask("FILES sort title desc")
        names = [x["name"] for x in app.files()[1]]
        check("the table sorts by the title column", names == ["ambience.wav", os.path.basename(third)], names)
        app.ask("FILES sort none")
        check("the app exits", app.quit() == 0)

        # ---- run 3: the column choice survives; selecting the import opens it
        app = App(args.exe, env, args.port, token_file, work, "3")
        app.idle()
        check("the column choice is remembered across a restart", app.ask("FILES columns") == "OK columns=title,size,name")
        check("the import is not remembered (it was not opened)", app.files()[0]["total"] == "0")
        check("IMPORT works with nothing open", app.ask(f"IMPORT {imp}") == "OK listed=1 open=0")
        stores_before = stores()
        r = app.ask("FILES select 0")
        check("selecting an imported row starts opening it", r == "OK opening", r)
        app.idle()
        h, frows = app.files()
        check("it is now an open, current document", h["total"] == "1" and frows[0]["imported"] == "0" and frows[0]["current"] == "1", (h, frows))
        check("its session store exists only now", len(stores()) == len(stores_before) + 1, (stores_before, stores()))
        check("it is on the recent list now", app.recent()[1][0][2] == imp, app.recent())
        r = app.ask("RECENT clear")
        check("Clear Recent empties the list", r == "OK count=0" and app.recent() == (0, []), r)
        check("FILES columns default restores the columns", app.ask("FILES columns default").startswith("OK columns=name,"))
        check("the app exits", app.quit() == 0)
        files = glob.glob(os.path.join(home, "**", "recent.txt"), recursive=True)
        lines = [l for p in files for l in open(p) if l.strip() and not l.startswith("#")]
        check("the cleared list is saved empty", bool(files) and not lines, (files, lines))
    except Exception as e:  # noqa: BLE001
        check("the run completed", False, repr(e))
    finally:
        if app and app.proc.poll() is None:
            app.proc.kill()
    log_files = [os.path.join(d, n) for d, _, fs in os.walk(logs) for n in fs if n == "wavcraft.log"]
    errors = [l.strip()[:200] for p in log_files for l in open(p, errors="replace") if " ERROR " in l]
    check("the app's log has no ERROR line", bool(log_files) and not errors, errors[:3] or "no log file")
    if not all(ok for _, ok, _ in checks) and log_files:
        print("--- log tail ---\n" + "".join(open(log_files[0], errors="replace").readlines()[-15:]))
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    bad = [c for c in checks if not c[1]]
    print(f"{len(checks) - len(bad)}/{len(checks)} checks passed")
    sys.exit(1 if bad else 0)


main()
