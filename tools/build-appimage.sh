#!/usr/bin/env bash
set -euo pipefail

# Builds the Switchboard AppImage. Runs only inside the craft-build distrobox;
# nothing is compiled or installed on the host (R34).

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

# Print the step, then run it unless this is a dry run.
run() {
  echo "+ $*"
  if [ "$dry_run" -eq 0 ]; then
    "$@"
  fi
}

echo "+ cd app"
cd app
run npm ci
# NO_STRIP=1: linuxdeploy's bundled strip fails on the container's newer binaries.
# APPIMAGE_EXTRACT_AND_RUN=1: linuxdeploy is itself an AppImage; run it without FUSE.
run env NO_STRIP=1 APPIMAGE_EXTRACT_AND_RUN=1 npm run tauri -- build --bundles appimage

if [ "$dry_run" -eq 1 ]; then
  echo "AppImage: target/release/bundle/appimage/ (dry run, nothing built)"
  exit 0
fi

cd "$root"
newest="$(ls -t target/release/bundle/appimage/*.AppImage 2>/dev/null | head -n 1 || true)"
if [ -z "$newest" ]; then
  echo "build-appimage: no AppImage under target/release/bundle/appimage" >&2
  exit 1
fi
echo "AppImage: $newest"
