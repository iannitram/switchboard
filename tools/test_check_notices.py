"""SHELL-2 (#11): the notice check for code brought in from BroCrew. Locked test (R28).

Run: python3 -I -m unittest discover -s tools -p "test_*.py" -v
"""

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import MappingProxyType

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "check-notices.py"
_spec = importlib.util.spec_from_file_location("check_notices", SCRIPT)
cn = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cn)

HEADER = (
    "// SPDX-License-Identifier: Apache-2.0\n"
    "// Copyright the BroCrew authors (https://github.com/brocrew/brocrew).\n"
    "// Lifted from BroCrew apps/desktop/src/renderer/src/lib/a.ts.\n"
    "// Changed: nothing.\n"
)
README = "# Switchboard\n\nCredits: BroCrew (Apache-2.0).\n"


def notice_for(*paths):
    return "Switchboard includes code from BroCrew (Apache-2.0).\n" + "".join(f"- {p}\n" for p in paths)


GOOD = MappingProxyType({
    "README.md": README,
    "NOTICE": notice_for("app/src/brocrew/a.ts"),
    "app/src/brocrew/README.md": "Our note, no header.\n",
    "app/src/brocrew/LICENSE": "Apache License\n",
    "app/src/brocrew/a.ts": HEADER + "export const a = 1\n",
})


def build(tree):
    tmp = tempfile.TemporaryDirectory()
    root = Path(tmp.name)
    for rel, text in tree.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    return tmp, root


def changed(edits):
    """GOOD with some files replaced; a value of None removes that file."""
    tree = dict(GOOD)
    for rel, text in edits.items():
        if text is None:
            tree.pop(rel, None)
        else:
            tree[rel] = text
    return MappingProxyType(tree)


def run_cli(*args):
    return subprocess.run(
        [sys.executable, "-I", str(SCRIPT), *args],
        capture_output=True, text=True, timeout=60,
    )


class LiftedFiles(unittest.TestCase):
    def test_no_brocrew_folder_means_no_lifted_files(self):
        tmp, root = build(MappingProxyType({"README.md": README}))
        with tmp:
            self.assertEqual(cn.lifted_files(root), [])
            self.assertEqual(cn.problems(root), [])

    def test_folder_readme_and_license_are_not_lifted_files(self):
        tree = MappingProxyType({
            "README.md": README,
            "app/src/brocrew/README.md": "no header\n",
            "app/src/brocrew/LICENSE": "no header\n",
        })
        tmp, root = build(tree)
        with tmp:
            self.assertEqual(cn.lifted_files(root), [])
            self.assertEqual(cn.problems(root), [])

    def test_lifted_files_are_every_file_below_sorted_as_posix_paths(self):
        tree = MappingProxyType({
            "app/src/brocrew/b.ts": HEADER,
            "app/src/brocrew/a.ts": HEADER,
            "app/src/brocrew/sub/c.ts": HEADER,
            "app/src/other.ts": "not lifted\n",
        })
        tmp, root = build(tree)
        with tmp:
            self.assertEqual(
                cn.lifted_files(root),
                ["app/src/brocrew/a.ts", "app/src/brocrew/b.ts", "app/src/brocrew/sub/c.ts"],
            )


class Problems(unittest.TestCase):
    def test_a_good_tree_has_no_problems(self):
        tmp, root = build(GOOD)
        with tmp:
            self.assertEqual(cn.problems(root), [])

    def test_each_of_the_three_markers_is_required(self):
        for marker in (
            "SPDX-License-Identifier: Apache-2.0",
            "https://github.com/brocrew/brocrew",
            "Changed:",
        ):
            with self.subTest(marker=marker):
                text = HEADER.replace(marker, "x") + "export const a = 1\n"
                tmp, root = build(changed({"app/src/brocrew/a.ts": text}))
                with tmp:
                    self.assertEqual(cn.problems(root), ["missing notice: app/src/brocrew/a.ts"])

    def test_a_notice_after_line_15_does_not_count(self):
        text = "\n" * 15 + HEADER
        tmp, root = build(changed({"app/src/brocrew/a.ts": text}))
        with tmp:
            self.assertEqual(cn.problems(root), ["missing notice: app/src/brocrew/a.ts"])

    def test_a_notice_ending_on_line_15_counts(self):
        text = "\n" * 11 + HEADER
        tmp, root = build(changed({"app/src/brocrew/a.ts": text}))
        with tmp:
            self.assertEqual(cn.problems(root), [])

    def test_no_notice_file_is_one_problem(self):
        tmp, root = build(changed({"NOTICE": None}))
        with tmp:
            self.assertEqual(cn.problems(root), ["NOTICE is missing"])

    def test_a_notice_file_that_does_not_credit_brocrew(self):
        tmp, root = build(changed({"NOTICE": "Third-party code:\n- app/src/brocrew/a.ts\n"}))
        with tmp:
            self.assertEqual(cn.problems(root), ["NOTICE does not credit BroCrew"])

    def test_a_lifted_file_not_named_in_notice(self):
        tmp, root = build(changed({"app/src/brocrew/b.ts": HEADER}))
        with tmp:
            self.assertEqual(cn.problems(root), ["not in NOTICE: app/src/brocrew/b.ts"])

    def test_a_readme_that_does_not_credit_brocrew(self):
        tmp, root = build(changed({"README.md": "# Switchboard\n"}))
        with tmp:
            self.assertEqual(cn.problems(root), ["README.md does not credit BroCrew"])

    def test_no_readme_is_the_same_problem(self):
        tmp, root = build(changed({"README.md": None}))
        with tmp:
            self.assertEqual(cn.problems(root), ["README.md does not credit BroCrew"])

    def test_problems_come_in_order(self):
        tree = MappingProxyType({
            "README.md": "# Switchboard\n",
            "NOTICE": "Third-party code:\n",
            "app/src/brocrew/b.ts": "no header\n",
            "app/src/brocrew/a.ts": "no header\n",
        })
        tmp, root = build(tree)
        with tmp:
            self.assertEqual(cn.problems(root), [
                "missing notice: app/src/brocrew/a.ts",
                "missing notice: app/src/brocrew/b.ts",
                "NOTICE does not credit BroCrew",
                "not in NOTICE: app/src/brocrew/a.ts",
                "not in NOTICE: app/src/brocrew/b.ts",
                "README.md does not credit BroCrew",
            ])

    def test_a_file_that_is_not_utf8_is_read_without_crashing(self):
        tmp, root = build(GOOD)
        with tmp:
            (root / "app/src/brocrew/icon.bin").write_bytes(b"\xff\xfe\x00binary")
            self.assertEqual(cn.problems(root), [
                "missing notice: app/src/brocrew/icon.bin",
                "not in NOTICE: app/src/brocrew/icon.bin",
            ])


class Cli(unittest.TestCase):
    def test_a_good_tree_exits_0_and_says_ok(self):
        tmp, root = build(GOOD)
        with tmp:
            out = run_cli(str(root))
            self.assertEqual(out.returncode, 0, out.stderr)
            self.assertEqual(out.stdout, "ok: 1 lifted file(s) carry their notice\n")

    def test_problems_exit_1_one_per_line(self):
        tmp, root = build(changed({"NOTICE": None, "app/src/brocrew/b.ts": "no header\n"}))
        with tmp:
            out = run_cli(str(root))
            self.assertEqual(out.returncode, 1)
            self.assertEqual(out.stdout, "missing notice: app/src/brocrew/b.ts\nNOTICE is missing\n")

    def test_main_returns_the_exit_code(self):
        tmp, root = build(GOOD)
        with tmp:
            self.assertEqual(cn.main(["check-notices.py", str(root)]), 0)
            (root / "NOTICE").unlink()
            self.assertEqual(cn.main(["check-notices.py", str(root)]), 1)

    def test_this_repo_passes(self):
        out = run_cli()
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)
        self.assertTrue(out.stdout.startswith("ok: "), out.stdout)


if __name__ == "__main__":
    unittest.main()
