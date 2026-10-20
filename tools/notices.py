#!/usr/bin/env python3
"""Third-party notices and the licence gate (A346, issue #49).

    python3 tools/notices.py --check            # fail if a dependency's licence is not allowed, or the notices are stale
    python3 tools/notices.py --write            # regenerate THIRD-PARTY-NOTICES.txt (also fails on a disallowed licence)
    python3 -m unittest tools.test_notices      # the checker's own tests (no cargo needed)

It reads `cargo metadata --locked` (every package in Cargo.lock, all platforms), and for each one
that is not a workspace member takes the licence expression and the licence/notice files in the
crate's source folder (the cargo registry or git checkout). It runs cargo itself, so on a build
machine run it through the build queue.
"""
import hashlib, json, os, re, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "THIRD-PARTY-NOTICES.txt")

# SPDX identifiers a dependency may be used under. BSD-* covers the BSD family; the Unicode
# licences cover unicode-ident and friends; OFL and the Ubuntu Font Licence cover egui's
# bundled fonts (they arrive through epaint_default_fonts); CC0 and 0BSD are public-domain-like.
ALLOWED_EXACT = {
    "MIT", "MIT-0", "Apache-2.0", "ISC", "Zlib", "MPL-2.0", "CC0-1.0", "0BSD", "Unlicense",
    "OFL-1.1", "Ubuntu-font-1.0", "BSL-1.0", "Unicode-3.0", "Unicode-DFS-2016",
}
ALLOWED_PREFIX = ("BSD-",)
# Licences that must never pass, even if someone adds them to the sets above by mistake.
FORBIDDEN_WORDS = ("GPL", "AGPL", "LGPL", "SSPL", "BUSL", "CC-BY-NC", "Commons-Clause")

# Crates whose Cargo.toml has no usable `license` field: the expression read from their licence
# files by hand. Keep this empty unless a crate really has none; say why next to each entry.
OVERRIDES = {}


def tokens(expr):
    return re.findall(r"\(|\)|[^\s()]+", expr)


def parse(expr):
    """An SPDX expression as nested tuples: ('id', name) | ('and'|'or', [..]). `WITH` exceptions
    are dropped (an exception only widens a licence). `/` is the old spelling of OR."""
    toks = tokens(expr.replace("/", " OR "))
    pos = 0

    def peek():
        return toks[pos] if pos < len(toks) else None

    def take():
        nonlocal pos
        pos += 1
        return toks[pos - 1]

    def atom():
        t = take() if peek() is not None else None
        if t is None:
            raise ValueError(f"unexpected end in {expr!r}")
        if t == "(":
            node = orx()
            if peek() != ")":
                raise ValueError(f"missing ) in {expr!r}")
            take()
            return node
        if t in (")", "AND", "OR", "WITH"):
            raise ValueError(f"unexpected {t!r} in {expr!r}")
        if peek() == "WITH":
            take()
            if peek() is None:
                raise ValueError(f"WITH without an exception in {expr!r}")
            take()
        return ("id", t.rstrip("+"))

    def andx():
        items = [atom()]
        while peek() == "AND":
            take()
            items.append(atom())
        return items[0] if len(items) == 1 else ("and", items)

    def orx():
        items = [andx()]
        while peek() == "OR":
            take()
            items.append(andx())
        return items[0] if len(items) == 1 else ("or", items)

    node = orx()
    if pos != len(toks):
        raise ValueError(f"trailing {toks[pos]!r} in {expr!r}")
    return node


def id_allowed(name):
    if any(w in name.upper() for w in FORBIDDEN_WORDS):
        return False
    return name in ALLOWED_EXACT or name.startswith(ALLOWED_PREFIX)


def allowed(expr):
    """True when the expression can be satisfied with allowed licences only: any OR branch, all
    AND parts. An expression that does not parse, or is empty, is not allowed."""
    if not expr or not expr.strip():
        return False
    try:
        node = parse(expr)
    except ValueError:
        return False

    def ok(n):
        if n[0] == "id":
            return id_allowed(n[1])
        return any(ok(c) for c in n[1]) if n[0] == "or" else all(ok(c) for c in n[1])

    return ok(node)


def metadata():
    out = subprocess.run(["cargo", "metadata", "--format-version", "1", "--locked"], cwd=ROOT, capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"cargo metadata failed:\n{out.stderr[-2000:]}")
    return json.loads(out.stdout)


def dependencies(meta):
    """(name, version, licence expression, repository, manifest dir) for every package that is not a workspace member."""
    members = set(meta["workspace_members"])
    rows = []
    for p in meta["packages"]:
        if p["id"] in members:
            continue
        expr = OVERRIDES.get(p["name"]) or p.get("license") or ""
        rows.append((p["name"], p["version"], expr, p.get("repository") or "", os.path.dirname(p["manifest_path"]), p.get("license_file"), p.get("authors") or []))
    rows.sort(key=lambda r: (r[0], r[1]))
    return rows


LICENCE_FILE = re.compile(r"^(licen[cs]e|copying|notice|unlicense|copyright|patents)", re.I)


def notice_files(folder, license_file=None):
    """Licence and notice files of a crate: top-level LICENSE*/COPYING*/NOTICE*..., files in a
    top-level LICENSES/ folder, the .txt files in a fonts/ folder, and the manifest's license-file."""
    found = []
    try:
        names = sorted(os.listdir(folder))
    except OSError:
        return found
    for n in names:
        path = os.path.join(folder, n)
        if os.path.isfile(path) and LICENCE_FILE.match(n):
            found.append(n)
        elif os.path.isdir(path) and n.lower() in ("licenses", "licences", "license", "licence"):
            found += [f"{n}/{f}" for f in sorted(os.listdir(path)) if os.path.isfile(os.path.join(path, f))]
        elif os.path.isdir(path) and n.lower() == "fonts":
            found += [f"{n}/{f}" for f in sorted(os.listdir(path)) if f.lower().endswith(".txt")]
    if license_file and license_file not in found and os.path.isfile(os.path.join(folder, license_file)):
        found.append(license_file)
    return found


# Standard texts, for a crate that ships no licence file of its own (its source repository has
# one, but the published package does not): the text of the first of these its licence
# expression offers, with the authors from its Cargo.toml as the copyright line.
FALLBACK_ORDER = ("MIT", "Apache-2.0", "MPL-2.0", "BSL-1.0")


def fallback(expr, authors):
    ids = set(tokens(expr.replace("/", " OR ")))
    for lic in FALLBACK_ORDER:
        if lic in ids:
            with open(os.path.join(ROOT, "tools", "licence-texts", f"{lic}.txt"), encoding="utf-8") as f:
                who = ", ".join(re.sub(r"\s*<[^>]*>", "", a) for a in authors) or "the authors of this library"
                return lic, f.read().replace("{authors}", who).strip("\n")
    return None, None


def read_text(path):
    with open(path, "rb") as f:
        data = f.read(400_000)
    return data.decode("utf-8", errors="replace").replace("\r\n", "\n").strip("\n")


HEADER = """\
WavCraft third-party notices
============================

WavCraft is built on the open-source libraries listed below. Each is used under the licence
shown; the licence and notice text that came with each library follows its entry. This file is
generated by tools/notices.py from Cargo.lock; do not edit it by hand.

Libraries under the Mozilla Public License 2.0 (MPL-2.0)
--------------------------------------------------------
The symphonia crates (symphonia, symphonia-core, symphonia-bundle-mp3, symphonia-codec-*,
symphonia-format-*, symphonia-metadata, symphonia-utils-xiph) decode audio files for WavCraft.
They are licensed under MPL-2.0 and are used unmodified. Their source code is available from
https://github.com/pdeljanov/Symphonia and from https://crates.io/crates/symphonia (and the
other crate names above, version 0.5.x). The MPL-2.0 text is reproduced in their entries below.
Any other MPL-2.0 library in the list is likewise used unmodified, and its source is available
at the address on its entry.

Fonts bundled with the user interface library (egui / epaint_default_fonts)
---------------------------------------------------------------------------
- Hack Regular: MIT licence with the Bitstream Vera licence (text under epaint_default_fonts).
- Ubuntu Light: Ubuntu Font Licence 1.0 (text under epaint_default_fonts).
- Noto Emoji Regular: SIL Open Font License 1.1 (text under epaint_default_fonts).
- emoji-icon-font: MIT licence (text under epaint_default_fonts).
The fonts are used unmodified. The OFL and UFL texts allow bundling and redistribution with
software, provided these notices stay with it.
"""


def render(rows):
    out = [HEADER, f"Libraries ({len(rows)})\n{'=' * 12}\n"]
    seen = {}  # hash of a long licence text -> first crate it appeared under
    for name, ver, expr, repo, folder, lic_file, authors in rows:
        out.append(f"{name} {ver}\n{'-' * len(f'{name} {ver}')}")
        out.append(f"Licence: {expr}")
        out.append(f"Source: {repo or 'https://crates.io/crates/' + name + '/' + ver}")
        files = notice_files(folder, lic_file)
        texts = [(f, read_text(os.path.join(folder, f))) for f in files]
        texts = [(f, t) for f, t in texts if t]
        if not texts:
            lic, text = fallback(expr, authors)
            if text:
                out.append("(the package ships no licence file; this is the standard text of the licence given in its Cargo.toml)")
                texts = [(f"standard {lic} text", text)]
            else:
                out.append("(the package ships no licence file; the licence above is from its Cargo.toml)")
        for f, text in texts:
            h = hashlib.sha256(text.encode()).hexdigest()
            out.append(f"\n[{f}]")
            if len(text) > 5000 and h in seen:
                out.append(f"(the same text as {seen[h]})")
            else:
                if len(text) > 5000:
                    seen[h] = f"{name} {ver}, {f}, above"
                out.append(text)
        out.append("\n")
    return "\n".join(out)


def problems(rows):
    return [f"{r[0]} {r[1]}: licence {r[2] or '(none)'} is not allowed" for r in rows if not allowed(r[2])]


def main(argv):
    mode = argv[1] if len(argv) > 1 else "--check"
    if mode not in ("--check", "--write"):
        sys.exit(__doc__)
    rows = dependencies(metadata())
    bad = problems(rows)
    for b in bad:
        print("FAIL", b)
    if bad:
        sys.exit(1)
    if mode == "--write":
        with open(OUT, "w", encoding="utf-8", newline="\n") as f:
            f.write(render(rows))
        print(f"wrote {OUT}: {len(rows)} libraries")
        return
    try:
        have = open(OUT, encoding="utf-8").read()
    except OSError:
        sys.exit("FAIL THIRD-PARTY-NOTICES.txt is missing: run tools/notices.py --write")
    stale = [f"{r[0]} {r[1]}" for r in rows if f"\n{r[0]} {r[1]}\n" not in have]
    for s in stale:
        print("FAIL not in THIRD-PARTY-NOTICES.txt:", s)
    if stale:
        sys.exit("FAIL the notices are out of date: run tools/notices.py --write")
    print(f"ok: {len(rows)} libraries, all licences allowed, notices current")


if __name__ == "__main__":
    main(sys.argv)
