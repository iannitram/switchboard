"""A stub fleet.py for core tests (CORE-1, CORE-2). Point the bridge at it with
FLEET_ROOT=crates/core/tests/fixtures/fleet-stub.

It prints the fixture JSON for the command from ../../answers/<command>.json, where <command>
is the first argument; with FLEET_STUB_EXIT=<n> it prints FLEET_STUB_STDERR to stderr and
exits n; with FLEET_STUB_BADJSON=1 it prints text that is not JSON; with
FLEET_STUB_SLEEP=<seconds> it sleeps that long first.
"""
import os
import pathlib
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
ANSWERS = HERE.parent.parent / "answers"


def main(argv):
    sleep = os.environ.get("FLEET_STUB_SLEEP")
    if sleep:
        time.sleep(float(sleep))
    code = os.environ.get("FLEET_STUB_EXIT")
    if code:
        sys.stderr.write(os.environ.get("FLEET_STUB_STDERR", "stub failure\n"))
        return int(code)
    if os.environ.get("FLEET_STUB_BADJSON"):
        sys.stdout.write("this is not json\n")
        return 0
    cmd = argv[0] if argv else "none"
    path = ANSWERS / (cmd + ".json")
    if not path.exists():
        sys.stderr.write("stub: no answer for %s\n" % cmd)
        return 2
    sys.stdout.write(path.read_text(encoding="utf-8"))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
