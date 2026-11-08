#!/usr/bin/env python3
"""SHELL-2 (#11): the notice check for code brought in from BroCrew."""

import sys
from pathlib import Path

LIFTED_DIR = Path("app") / "src" / "brocrew"
SKIP = {"README.md", "LICENSE"}
MARKERS = (
    "SPDX-License-Identifier: Apache-2.0",
    "https://github.com/brocrew/brocrew",
    "Changed:",
)
HEAD_LINES = 15


def lifted_files(root: Path) -> list[str]:
    folder = root / LIFTED_DIR
    if not folder.is_dir():
        return []
    files = [
        path.relative_to(root).as_posix()
        for path in folder.rglob("*")
        if path.is_file() and path.name not in SKIP
    ]
    return sorted(files)


def problems(root: Path) -> list[str]:
    out = []
    files = lifted_files(root)
    for rel in files:
        text = (root / rel).read_text(encoding="utf-8", errors="replace")
        head = "\n".join(text.splitlines()[:HEAD_LINES])
        if any(marker not in head for marker in MARKERS):
            out.append(f"missing notice: {rel}")
    if not files:
        return out
    notice = root / "NOTICE"
    if not notice.is_file():
        out.append("NOTICE is missing")
    else:
        notice_text = notice.read_text(encoding="utf-8", errors="replace")
        if "BroCrew" not in notice_text:
            out.append("NOTICE does not credit BroCrew")
        for rel in files:
            if rel not in notice_text:
                out.append(f"not in NOTICE: {rel}")
    readme = root / "README.md"
    if not readme.is_file() or "BroCrew" not in readme.read_text(encoding="utf-8", errors="replace"):
        out.append("README.md does not credit BroCrew")
    return out


def main(argv: list[str]) -> int:
    root = Path(argv[1]) if len(argv) > 1 else Path(__file__).resolve().parent.parent
    found = problems(root)
    if found:
        for line in found:
            print(line)
        return 1
    print(f"ok: {len(lifted_files(root))} lifted file(s) carry their notice")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
