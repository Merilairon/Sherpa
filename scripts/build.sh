#!/usr/bin/env bash
# Build release binaries and desktop packages.
#
#   scripts/build.sh linux     # basecampd + AppImage/deb/tar.gz
#   scripts/build.sh win       # basecampd.exe (cross, x86_64-pc-windows-gnu) + NSIS installer/zip
#   scripts/build.sh all
#   scripts/build.sh bin       # daemon binaries only
#
# Cross-building the Windows installer on Linux needs mingw-w64 (for the daemon) and
# wine (for the NSIS installer). On Windows itself use scripts/build.ps1.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${1:-all}"
cd "$ROOT"

build_linux_bin() {
  cargo build --release -p basecampd
  install -Dm755 target/release/basecampd dist/bin/linux/basecampd
}

build_win_bin() {
  rustup target add x86_64-pc-windows-gnu >/dev/null 2>&1 || true
  cargo build --release -p basecampd --target x86_64-pc-windows-gnu
  install -Dm755 target/x86_64-pc-windows-gnu/release/basecampd.exe dist/bin/win/basecampd.exe
}

app_deps() {
  (cd app && { [ -d node_modules ] || npm ci; })
}

case "$TARGET" in
  bin)
    build_linux_bin
    build_win_bin
    ;;
  linux)
    build_linux_bin
    app_deps
    (cd app && npx electron-builder --linux --publish never)
    ;;
  win)
    build_win_bin
    app_deps
    (cd app && npx electron-builder --win --publish never)
    ;;
  all)
    build_linux_bin
    build_win_bin
    app_deps
    (cd app && npx electron-builder --linux --publish never)
    (cd app && npx electron-builder --win --publish never)
    ;;
  *)
    echo "usage: $0 [linux|win|all|bin]" >&2
    exit 2
    ;;
esac
echo "Artifacts are in dist/"
