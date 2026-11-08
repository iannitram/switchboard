"""SHELL-2 (#11): behaviours the issue states that the locked tests do not cover.

Added by the R25 review of PR #57. The locked file test_check_notices.py is unchanged.

Run: python3 -I -m unittest discover -s tools -p "test_*.py" -v
"""

import importlib.util
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("check_notices", HERE / "check-notices.py")
cn = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cn)


def build(tree):
    tmp = tempfile.TemporaryDirectory()
    root = Path(tmp.name)
    for rel, text in tree.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    return tmp, root


class NoLiftedFiles(unittest.TestCase):
    def test_without_lifted_files_the_notice_and_readme_checks_are_skipped(self):
        # The issue: "If there are no lifted files, stop here" (no NOTICE or README.md check).
        tmp, root = build({"README.md": "# Switchboard\n"})
        with tmp:
            self.assertEqual(cn.problems(root), [])


class OneLinePerFile(unittest.TestCase):
    def test_a_file_missing_every_marker_is_one_problem_line(self):
        # The issue: "one line per file, however many markers are missing".
        tmp, root = build({
            "app/src/brocrew/a.ts": "export const a = 1\n",
            "NOTICE": "BroCrew\napp/src/brocrew/a.ts\n",
            "README.md": "BroCrew\n",
        })
        with tmp:
            self.assertEqual(cn.problems(root), ["missing notice: app/src/brocrew/a.ts"])


class SkipByFileName(unittest.TestCase):
    def test_a_nested_readme_is_skipped_by_file_name(self):
        # The issue: SKIP applies to the file name, at any depth below the folder.
        tmp, root = build({
            "app/src/brocrew/sub/README.md": "no header\n",
            "README.md": "BroCrew\n",
        })
        with tmp:
            self.assertEqual(cn.lifted_files(root), [])
            self.assertEqual(cn.problems(root), [])


if __name__ == "__main__":
    unittest.main()
