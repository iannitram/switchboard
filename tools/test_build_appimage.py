"""SHELL-1 (#10): the AppImage build script runs only inside the craft-build container. Locked test (R28).

Run: python3 -I -m unittest discover -s tools -p "test_*.py" -v
"""

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
SCRIPT = HERE / "build-appimage.sh"

REFUSAL = (
    "build-appimage: run this inside the craft-build container: "
    "distrobox enter craft-build -- tools/build-appimage.sh\n"
)
DRY_RUN = (
    "+ cd app\n"
    "+ npm ci\n"
    "+ env NO_STRIP=1 APPIMAGE_EXTRACT_AND_RUN=1 npm run tauri -- build --bundles appimage\n"
    "AppImage: target/release/bundle/appimage/ (dry run, nothing built)\n"
)


def run(*args, container=None, cwd=None):
    env = {k: v for k, v in os.environ.items() if k != "CONTAINER_ID"}
    if container is not None:
        env["CONTAINER_ID"] = container
    return subprocess.run(
        ["bash", str(SCRIPT), *args],
        capture_output=True, text=True, timeout=60, env=env, cwd=cwd or ROOT,
    )


class BuildAppImage(unittest.TestCase):
    def test_the_script_is_an_executable_bash_script(self):
        self.assertTrue(SCRIPT.is_file())
        self.assertTrue(os.access(SCRIPT, os.X_OK), "chmod +x tools/build-appimage.sh")
        self.assertEqual(SCRIPT.read_text(encoding="utf-8").splitlines()[0], "#!/usr/bin/env bash")

    def test_outside_any_container_it_refuses_with_exit_2(self):
        out = run("--dry-run")
        self.assertEqual(out.returncode, 2)
        self.assertEqual(out.stderr, REFUSAL)
        self.assertEqual(out.stdout, "")

    def test_in_another_container_it_refuses_with_exit_2(self):
        out = run("--dry-run", container="some-other-box")
        self.assertEqual(out.returncode, 2)
        self.assertEqual(out.stderr, REFUSAL)
        self.assertEqual(out.stdout, "")

    def test_the_container_check_comes_before_the_arguments(self):
        out = run("--bogus")
        self.assertEqual(out.returncode, 2)
        self.assertEqual(out.stderr, REFUSAL)

    def test_an_unknown_argument_is_exit_64(self):
        out = run("--bogus", container="craft-build")
        self.assertEqual(out.returncode, 64)
        self.assertEqual(out.stderr, "build-appimage: unknown argument: --bogus\n")
        self.assertEqual(out.stdout, "")

    def test_dry_run_prints_each_step_and_runs_none(self):
        out = run("--dry-run", container="craft-build")
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(out.stdout, DRY_RUN)
        self.assertEqual(out.stderr, "")

    def test_dry_run_is_the_same_from_any_folder(self):
        with tempfile.TemporaryDirectory() as elsewhere:
            out = run("--dry-run", container="craft-build", cwd=elsewhere)
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(out.stdout, DRY_RUN)

    def test_the_bundle_is_the_switchboard_appimage(self):
        conf = json.loads((ROOT / "app" / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
        self.assertEqual(conf["productName"], "Switchboard")
        self.assertEqual(conf["bundle"]["targets"], ["appimage"])
        self.assertEqual(conf["build"]["frontendDist"], "../dist")
        self.assertEqual(conf["build"]["beforeBuildCommand"], "npm run build")


if __name__ == "__main__":
    unittest.main()
