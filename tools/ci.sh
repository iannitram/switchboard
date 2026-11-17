#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

run_step() {
  local name="$1"
  shift
  printf '\n==> %s\n' "$name"
  "$@"
  local status=$?
  if [[ $status -ne 0 ]]; then
    printf 'FAILED: %s (exit %s)\n' "$name" "$status" >&2
    exit "$status"
  fi
}

run_rust() {
  run_step "cargo fmt --all -- --check" cargo fmt --all -- --check
  run_step "cargo clippy --all-targets -- -D warnings" cargo clippy --all-targets -- -D warnings
  run_step "cargo test" cargo test
}

run_app() {
  run_step "cargo clippy -p switchboard-app --all-targets -- -D warnings" cargo clippy -p switchboard-app --all-targets -- -D warnings
  run_step "cargo test -p switchboard-app" cargo test -p switchboard-app
}

run_face() {
  cd "$ROOT/app"
  run_step "npm ci (app/)" npm ci || exit $?
  run_step "npm run build (app/)" npm run build || exit $?
  run_step "npm test (app/)" npm test || exit $?
  cd "$ROOT"
}

run_lint() {
  # On Windows, prefer the Git Bash running this script over the WSL bash.exe shim.
  export PATH="$(dirname "$(command -v bash)"):$PATH"
  if command -v cygpath >/dev/null 2>&1; then
    export SWITCHBOARD_BASH="$(cygpath -w "$(command -v bash)")"
  else
    export SWITCHBOARD_BASH="$(command -v bash)"
  fi
  run_step 'python3 -I -m unittest discover -s tools -p "test_*.py"' python3 -I -m unittest discover -s tools -p 'test_*.py'
  run_step "python3 -I tools/check-notices.py" python3 -I tools/check-notices.py
  run_step "python3 -I tools/arch_lint.py" python3 -I tools/arch_lint.py
}

part="${1:-all}"
if [[ $# -gt 1 ]]; then
  printf 'usage: tools/ci.sh [rust|app|face|lint]\n' >&2
  exit 2
fi

case "$part" in
  all)
    run_rust
    run_face
    run_lint
    ;;
  rust) run_rust ;;
  app) run_app ;;
  face) run_face ;;
  lint) run_lint ;;
  *)
    printf 'unknown CI part: %s (choose rust, app, face or lint)\n' "$part" >&2
    exit 2
    ;;
esac
