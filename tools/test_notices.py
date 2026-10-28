"""Tests for the licence gate: python3 -m unittest discover -s tools -p 'test_*.py'"""
import os, sys, tempfile, unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import notices as n


class Allowed(unittest.TestCase):
    def test_the_allowed_families_pass(self):
        for e in ["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "MPL-2.0", "OFL-1.1",
                  "Unicode-3.0", "Unicode-DFS-2016", "CC0-1.0", "Ubuntu-font-1.0", "0BSD"]:
            self.assertTrue(n.allowed(e), e)

    def test_gpl_and_friends_fail(self):
        for e in ["GPL-3.0", "GPL-2.0-only", "GPL-3.0-or-later", "LGPL-2.1", "AGPL-3.0", "SSPL-1.0", "CC-BY-NC-4.0",
                  "Proprietary", "LicenseRef-foo"]:
            self.assertFalse(n.allowed(e), e)

    def test_or_needs_one_allowed_branch(self):
        self.assertTrue(n.allowed("MIT OR GPL-3.0"))
        self.assertTrue(n.allowed("GPL-3.0 OR Apache-2.0"))
        self.assertFalse(n.allowed("GPL-2.0 OR GPL-3.0"))
        self.assertTrue(n.allowed("MIT/Apache-2.0"), "the old slash spelling is OR")

    def test_and_needs_every_part(self):
        self.assertTrue(n.allowed("(MIT OR Apache-2.0) AND Unicode-3.0"))
        self.assertTrue(n.allowed("(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0"))
        self.assertFalse(n.allowed("MIT AND GPL-3.0"))
        self.assertFalse(n.allowed("(MIT OR Apache-2.0) AND GPL-3.0-only"))

    def test_with_exceptions_and_plus(self):
        self.assertTrue(n.allowed("Apache-2.0 WITH LLVM-exception"))
        self.assertTrue(n.allowed("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT"))
        self.assertFalse(n.allowed("GPL-2.0-or-later WITH Classpath-exception-2.0"))
        self.assertTrue(n.allowed("Apache-2.0+"))

    def test_a_gpl_name_cannot_be_smuggled_in_by_a_prefix(self):
        self.assertFalse(n.allowed("BSD-3-Clause AND GPL-3.0"))
        self.assertFalse(n.allowed("BSD-GPL-hybrid"), "a forbidden word wins over an allowed prefix")

    def test_missing_or_broken_expressions_fail(self):
        for e in ["", "   ", None, "MIT OR", "(MIT", "MIT)", "AND MIT", "MIT MIT", "MIT WITH"]:
            self.assertFalse(n.allowed(e), repr(e))

    def test_problems_names_the_crate(self):
        rows = [("good", "1.0.0", "MIT", "", "", None, []), ("evil", "2.0.0", "GPL-3.0", "", "", None, []), ("none", "0.1.0", "", "", "", None, [])]
        msgs = n.problems(rows)
        self.assertEqual(len(msgs), 2)
        self.assertIn("evil 2.0.0", msgs[0])
        self.assertIn("none 0.1.0", msgs[1])


class Files(unittest.TestCase):
    def test_notice_files_finds_licences_fonts_and_the_manifest_file(self):
        with tempfile.TemporaryDirectory() as d:
            for p in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "README.md", "src/lib.rs", "fonts/OFL.txt", "fonts/a.ttf", "LICENSES/Z.txt", "odd-terms.txt"]:
                os.makedirs(os.path.dirname(os.path.join(d, p)), exist_ok=True)
                open(os.path.join(d, p), "w").write("x")
            got = n.notice_files(d, "odd-terms.txt")
            self.assertEqual(sorted(got), sorted(["LICENSE-APACHE", "LICENSE-MIT", "NOTICE", "fonts/OFL.txt", "LICENSES/Z.txt", "odd-terms.txt"]))

    def test_render_lists_each_crate_and_dedupes_long_texts(self):
        with tempfile.TemporaryDirectory() as d:
            a, b = os.path.join(d, "a"), os.path.join(d, "b")
            for f in (a, b):
                os.makedirs(f)
                open(os.path.join(f, "LICENSE"), "w").write("Long licence. " * 600)
            text = n.render([("a", "1.0.0", "MPL-2.0", "https://example.com/a", a, None, []), ("b", "2.0.0", "MPL-2.0", "", b, None, [])])
        self.assertIn("\na 1.0.0\n", text)
        self.assertIn("\nb 2.0.0\n", text)
        self.assertEqual(text.count(("Long licence. " * 600).strip()), 1, "the long text appears once")
        self.assertIn("the same text as a 1.0.0", text)
        self.assertIn("https://crates.io/crates/b/2.0.0", text)

    def test_a_crate_without_a_licence_file_gets_the_standard_text_and_its_authors(self):
        with tempfile.TemporaryDirectory() as d:
            text = n.render([("nofile", "1.0.0", "MIT OR Apache-2.0", "", d, None, ["Jane Doe <jane@example.com>", "Sam Roe"])])
            mpl = n.render([("m", "1.0.0", "MPL-2.0", "", d, None, [])])
        self.assertIn("Copyright (c) Jane Doe, Sam Roe", text)
        self.assertNotIn("jane@example.com", text, "e-mail addresses are left out")
        self.assertIn("Permission is hereby granted", text)
        self.assertIn("Mozilla Public License Version 2.0", mpl)
        self.assertEqual(n.fallback("GPL-3.0", []), (None, None))


if __name__ == "__main__":
    unittest.main()
