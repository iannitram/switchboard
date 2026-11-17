#!/usr/bin/env python3
"""Architecture checks for the React face, process boundaries and MCP tool docs."""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Iterable

ROOT = Path(__file__).resolve().parent.parent
DOC = Path("docs/mcp-tools.md")
SPAWN_ALLOWLIST = {
    "crates/core/src/autopilot/driver.rs",
    "crates/core/src/autopilot/instructions.rs",
    "crates/core/src/autopilot/loops.rs",
    "crates/core/src/autopilot/passes.rs",
    "crates/core/src/autopilot/weekly.rs",
    "crates/core/src/backup.rs",
    "crates/core/src/bug/fix.rs",
    "crates/core/src/fleet.rs",
    "crates/core/src/github/merge.rs",
    "crates/core/src/it.rs",
    "crates/core/src/lock.rs",
    "crates/core/src/platform.rs",
    "crates/core/src/reports.rs",
    "crates/core/src/unit/claim.rs",
}
# Directory exemption: Rust integration tests under crates/*/tests/ spawn their fixture processes
# on purpose (the CLI and MCP binaries under test), so the spawn rule does not cover them.
SPAWN_TESTS_DIR_EXEMPT = "tests"
FACE_PATTERNS = (
    (re.compile(r"\bfetch\s*\("), "use invoke through app/src/core.ts's call"),
    (re.compile(r"\bXMLHttpRequest\b"), "use invoke through app/src/core.ts's call"),
    (re.compile(r"\bWebSocket\s*\("), "use invoke through app/src/core.ts's call"),
    (re.compile(r"@tauri-apps/api/core"), "import invoke only in app/src/core.ts; use its call"),
)


def _relative(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def face_findings(root: Path) -> list[str]:
    findings = []
    src = root / "app" / "src"
    for path in sorted(src.rglob("*")):
        if not path.is_file() or path.suffix not in {".ts", ".tsx", ".js", ".jsx"}:
            continue
        rel = _relative(path, root)
        if rel in {"app/src/core.ts", "app/src/dev-core.ts"} or re.search(
            r"(?:^|[./_])(test|tests|spec|specs)(?:[./_]|$)", rel, re.IGNORECASE
        ):
            continue
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError:
            continue
        for number, line in enumerate(lines, 1):
            for pattern, fix in FACE_PATTERNS:
                if pattern.search(line):
                    findings.append(f"{rel}:{number}: React face I/O boundary violated; fix: {fix}")
                    break
    return findings


def spawn_findings(root: Path) -> list[str]:
    findings = []
    bases = [root / "crates", root / "app" / "src-tauri" / "src"]
    for base in bases:
        if not base.exists():
            continue
        for path in sorted(base.rglob("*.rs")):
            rel = _relative(path, root)
            if rel in SPAWN_ALLOWLIST or SPAWN_TESTS_DIR_EXEMPT in path.relative_to(base).parts:
                continue
            try:
                lines = path.read_text(encoding="utf-8").splitlines()
            except UnicodeDecodeError:
                continue
            for number, line in enumerate(lines, 1):
                if "Command::new" in line:
                    findings.append(
                        f"{rel}:{number}: process spawn is outside the allowlist; fix: route it "
                        "through FleetBridge, or add this file to SPAWN_ALLOWLIST in "
                        "tools/arch_lint.py with a reason in the same change"
                    )
    return findings


def tool_lists(root: Path) -> dict[str, list[dict]]:
    """Ask the actual MCP server's tools/list registry for every supported role."""
    roles: tuple[str | None, ...] = (None, "project-manager", "budget-manager", "satellite")
    result: dict[str, list[dict]] = {}
    request = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}})
    for role in roles:
        args = ["cargo", "run", "--quiet", "-p", "switchboard-cli", "--"]
        if role:
            args.extend(["--role", role])
        args.append("mcp")
        try:
            completed = subprocess.run(
                args,
                cwd=root,
                input=request + "\n",
                capture_output=True,
                text=True,
                encoding="utf-8",
                check=False,
            )
        except OSError as exc:
            raise RuntimeError(f"could not run {' '.join(args)}: {exc}") from exc
        if completed.returncode != 0:
            detail = completed.stderr.strip() or f"exit {completed.returncode}"
            raise RuntimeError(f"{' '.join(args)} failed: {detail}")
        try:
            response = json.loads(completed.stdout.splitlines()[0])
            result[role or "read-only"] = response["result"]["tools"]
        except (IndexError, json.JSONDecodeError, KeyError, TypeError) as exc:
            raise RuntimeError(f"tools/list response for {role or 'read-only'} was invalid") from exc
    return result


def render_tool_doc(lists: dict[str, list[dict]]) -> str:
    lines = [
        "# MCP tools",
        "",
        "Generated from `switchboard mcp`'s `tools/list` registry by `tools/arch_lint.py`.",
        "Regenerate with `python3 -I tools/arch_lint.py --write-mcp-tools`.",
        "",
    ]
    for role, tools in lists.items():
        label = "Read only" if role == "read-only" else role
        lines.extend([f"## {label}", "", "| Tool | Description | Input schema |", "| --- | --- | --- |"])
        for tool in tools:
            name = str(tool.get("name", ""))
            description = str(tool.get("description", "")).replace("|", "\\|").replace("\n", " ")
            schema = json.dumps(tool.get("inputSchema", {}), ensure_ascii=False, separators=(",", ":"))
            lines.append(f"| `{name}` | {description} | `{schema}` |")
        lines.append("")
    return "\n".join(lines)


def doc_findings(root: Path, lists: dict[str, list[dict]]) -> list[str]:
    expected = render_tool_doc(lists)
    path = root / DOC
    actual = path.read_text(encoding="utf-8") if path.exists() else ""
    if actual == expected:
        return []
    return [
        "docs/mcp-tools.md:1: generated MCP tool list is stale; fix: "
        "python3 -I tools/arch_lint.py --write-mcp-tools"
    ]


def lint(root: Path, lists: dict[str, list[dict]] | None = None) -> list[str]:
    findings = face_findings(root) + spawn_findings(root)
    if lists is None:
        lists = tool_lists(root)
    findings.extend(doc_findings(root, lists))
    return findings


def main(argv: Iterable[str] = sys.argv[1:]) -> int:
    args = list(argv)
    if args not in ([], ["--write-mcp-tools"]):
        print("usage: python3 -I tools/arch_lint.py [--write-mcp-tools]", file=sys.stderr)
        return 2
    try:
        lists = tool_lists(ROOT)
        if args:
            path = ROOT / DOC
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(render_tool_doc(lists), encoding="utf-8", newline="\n")
            print(f"wrote {DOC.as_posix()}")
            return 0
        findings = face_findings(ROOT) + spawn_findings(ROOT) + doc_findings(ROOT, lists)
    except RuntimeError as exc:
        print(f"architecture lint could not check MCP tools: {exc}", file=sys.stderr)
        return 1
    if findings:
        print("\n".join(findings), file=sys.stderr)
        return 1
    print("architecture lints passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
