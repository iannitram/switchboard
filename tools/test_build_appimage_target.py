"""#433: the AppImage build script finds the bundle under CARGO_TARGET_DIR when it is set.

Run: python3 -I -m unittest discover -s tools -p "test_*.py" -v
"""

import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "build-appimage.sh"

# A stand-in for npm: `npm run ...` drops an AppImage where cargo would, `npm ci` does nothing.
FAKE_NPM = """#!/usr/bin/env bash
if [ "$1" = "run" ]; then
  dir="${CARGO_TARGET_DIR:-$FAKE_DEFAULT_TARGET}/release/bundle/appimage"
  mkdir -p "$dir"
  : > "$dir/Switchboard_0.0.1_amd64.AppImage"
fi
exit 0
"""


def run(*args, target=None, fake_target=None):
    with tempfile.TemporaryDirectory() as tmp:
        bin_dir = Path(tmp) / "bin"
        bin_dir.mkdir()
        npm = bin_dir / "npm"
        npm.write_text(FAKE_NPM, encoding="utf-8")
        npm.chmod(npm.stat().st_mode | stat.S_IXUSR)
        env = {k: v for k, v in os.environ.items() if k not in ("CARGO_TARGET_DIR", "CONTAINER_ID")}
        env["CONTAINER_ID"] = "craft-build"
        env["PATH"] = f"{bin_dir}{os.pathsep}{env['PATH']}"
        if target is not None:
            env["CARGO_TARGET_DIR"] = target
        env["FAKE_DEFAULT_TARGET"] = fake_target or str(Path(tmp) / "unused")
        return subprocess.run(
            ["bash", str(SCRIPT), *args], capture_output=True, text=True, timeout=60, env=env
        )


class TargetDir(unittest.TestCase):
    def test_it_finds_the_bundle_under_cargo_target_dir(self):
        with tempfile.TemporaryDirectory() as target:
            out = run(target=target)
            self.assertEqual(out.returncode, 0, out.stderr)
            self.assertIn(
                f"AppImage: {target}/release/bundle/appimage/Switchboard_0.0.1_amd64.AppImage",
                out.stdout,
            )

    def test_dry_run_names_the_target_dir_folder(self):
        out = run("--dry-run", target="/some/where")
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn(
            "AppImage: /some/where/release/bundle/appimage/ (dry run, nothing built)", out.stdout
        )

    def test_a_missing_bundle_still_exits_1_and_names_the_folder(self):
        # The fake npm cannot make this folder, so no AppImage lands in it.
        out = run(target="/nonexistent-target-dir-for-test/x")
        self.assertEqual(out.returncode, 1)
        self.assertIn(
            "no AppImage under /nonexistent-target-dir-for-test/x/release/bundle/appimage",
            out.stderr,
        )


if __name__ == "__main__":
    unittest.main()
