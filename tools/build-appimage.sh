#!/usr/bin/env bash
set -euo pipefail

# SHELL-1 (#10): build the Switchboard AppImage. Rust and Tauri are compiled
# only inside the craft-build distrobox, never on the host (R34).
#   distrobox enter craft-build -- tools/build-appimage.sh [--dry-run]

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

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# Print a step as `+ <command>`, then run it unless this is a dry run.
step() {
  echo "+ $*"
  if [ "$dry_run" -eq 0 ]; then
    "$@"
  fi
}

echo "+ cd app"
cd app
step npm ci
# NO_STRIP=1: linuxdeploy's old strip fails on the container's newer binaries.
# APPIMAGE_EXTRACT_AND_RUN=1: linuxdeploy, itself an AppImage, runs without FUSE.
step env NO_STRIP=1 APPIMAGE_EXTRACT_AND_RUN=1 npm run tauri -- build --bundles appimage
cd "$root"

bundle_dir="target/release/bundle/appimage"
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
