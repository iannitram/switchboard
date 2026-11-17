import importlib.util
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("arch_lint.py")
SPEC = importlib.util.spec_from_file_location("arch_lint", SCRIPT)
arch_lint = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(arch_lint)


TOOL_LISTS = {
    "read-only": [
        {
            "name": "sky",
            "description": "The sky.",
            "inputSchema": {"type": "object", "properties": {}},
        }
    ],
    "project-manager": [],
    "budget-manager": [],
    "satellite": [],
}


class ArchitectureLintTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / "app/src").mkdir(parents=True)
        (self.root / "crates/core/src").mkdir(parents=True)
        (self.root / "app/src/core.ts").write_text("export const call = () => {};\n", encoding="utf-8")
        (self.root / "app/src/dev-core.ts").write_text("export const call = () => {};\n", encoding="utf-8")
        doc = self.root / arch_lint.DOC
        doc.parent.mkdir(parents=True)
        doc.write_text(arch_lint.render_tool_doc(TOOL_LISTS), encoding="utf-8")

    def tearDown(self):
        self.temp.cleanup()

    def test_face_io_rule_fails_with_invoke_fix_then_passes(self):
        view = self.root / "app/src/View.tsx"
        view.write_text("const answer = await fetch('/data');\n", encoding="utf-8")
        finding = arch_lint.face_findings(self.root)
        self.assertEqual(len(finding), 1)
        self.assertIn("View.tsx:1", finding[0])
        self.assertIn("invoke", finding[0])
        self.assertIn("app/src/core.ts", finding[0])
        view.write_text("import { call } from './core';\n", encoding="utf-8")
        self.assertEqual(arch_lint.face_findings(self.root), [])

    def test_process_rule_fails_outside_allowlist_then_passes(self):
        module = self.root / "crates/core/src/new_module.rs"
        module.write_text("Command::new(\"git\");\n", encoding="utf-8")
        finding = arch_lint.spawn_findings(self.root)
        self.assertEqual(len(finding), 1)
        self.assertIn("new_module.rs:1", finding[0])
        self.assertIn("FleetBridge", finding[0])
        module.write_text("FleetBridge::run();\n", encoding="utf-8")
        self.assertEqual(arch_lint.spawn_findings(self.root), [])

    def test_spawn_dir_allowlist_covers_only_the_named_test_dirs(self):
        allowed = self.root / "crates/core/tests/fixture.rs"
        allowed.parent.mkdir(parents=True)
        allowed.write_text("Command::new(\"git\");\n", encoding="utf-8")
        self.assertEqual(arch_lint.spawn_findings(self.root), [])
        nested = self.root / "crates/core/src/tests/inner.rs"
        nested.parent.mkdir(parents=True)
        nested.write_text("Command::new(\"git\");\n", encoding="utf-8")
        finding = arch_lint.spawn_findings(self.root)
        self.assertEqual(len(finding), 1)
        self.assertIn("inner.rs:1", finding[0])

    def test_generated_mcp_doc_fails_when_stale_then_passes(self):
        doc = self.root / arch_lint.DOC
        doc.write_text("# old tool list\n", encoding="utf-8")
        finding = arch_lint.doc_findings(self.root, TOOL_LISTS)
        self.assertEqual(len(finding), 1)
        self.assertIn("tools/arch_lint.py --write-mcp-tools", finding[0])
        doc.write_text(arch_lint.render_tool_doc(TOOL_LISTS), encoding="utf-8")
        self.assertEqual(arch_lint.doc_findings(self.root, TOOL_LISTS), [])


if __name__ == "__main__":
    unittest.main()
