#!/usr/bin/env bash
# Build the Switchboard AppImage inside the craft-build distrobox (SHELL-1, #10).
# Nothing is compiled or installed on the host (R34): outside the container it refuses.
set -euo pipefail

if [ "${CONTAINER_ID:-}" != "craft-build" ]; then
  echo "build-appimage: run this inside the craft-build container: distrobox enter craft-build -- tools/build-appimage.sh" >&2
  exit 2
fi

dry_run=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    *)
      echo "build-appimage: unknown argument: $arg" >&2
      exit 64
      ;;
  esac
done

# A relative CARGO_TARGET_DIR is taken from where the script was started, made absolute before
# any cd, so the build and the bundle lookup below name the same folder (#433).
if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  case "$CARGO_TARGET_DIR" in
    /*) ;;
    *) CARGO_TARGET_DIR="$PWD/$CARGO_TARGET_DIR" ;;
  esac
  export CARGO_TARGET_DIR
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

step() {
  echo "+ $*"
  if [ "$dry_run" -eq 0 ]; then
    "$@"
  fi
}

echo "+ cd app"
cd app
step npm ci
step env NO_STRIP=1 APPIMAGE_EXTRACT_AND_RUN=1 npm run tauri -- build --bundles appimage

# Where the bundle lands (#433): CARGO_TARGET_DIR when it is set (made absolute above), else the
# target folder cargo reports, else <root>/target.
if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  target_dir="$CARGO_TARGET_DIR"
elif [ "$dry_run" -eq 1 ]; then
  target_dir="target"
else
  target_dir="$(cargo metadata --no-deps --format-version 1 2>/dev/null \
    | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' || true)"
  target_dir="${target_dir:-$root/target}"
fi
bundle_dir="$target_dir/release/bundle/appimage"
if [ "$dry_run" -eq 1 ]; then
  echo "AppImage: $bundle_dir/ (dry run, nothing built)"
  exit 0
fi

appimage="$(ls -t "$bundle_dir"/*.AppImage 2>/dev/null | head -n 1 || true)"
if [ -z "$appimage" ]; then
  echo "build-appimage: no AppImage under $bundle_dir" >&2
  exit 1
fi
echo "AppImage: $appimage"
