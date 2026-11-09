#!/usr/bin/env bash
# Download the pinned aria2c sidecar for Windows into src-tauri/binaries/ (Tauri externalBin
# naming: <name>-<target-triple>.exe). The archive is checked against its SHA-256; a
# mismatch stops the build. Used by CI before the Windows bundle.
#
# Linux does not bundle aria2c: the .deb depends on Ubuntu's `aria2` package and the app
# finds `aria2c` on PATH.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/src-tauri/binaries"
mkdir -p "$OUT"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

ARIA2_VERSION=1.37.0
WIN_URL="https://github.com/aria2/aria2/releases/download/release-${ARIA2_VERSION}/aria2-${ARIA2_VERSION}-win-64bit-build1.zip"
WIN_SHA=67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288
DEST="$OUT/aria2c-x86_64-pc-windows-msvc.exe"

if [ -s "$DEST" ]; then echo "ok  $(basename "$DEST") (already present)"; exit 0; fi
curl -fsSL --retry 3 -o "$WORK/aria2.zip" "$WIN_URL"
echo "$WIN_SHA  $WORK/aria2.zip" | sha256sum -c --quiet - || { echo "checksum mismatch for $WIN_URL" >&2; exit 1; }
unzip -p "$WORK/aria2.zip" "aria2-${ARIA2_VERSION}-win-64bit-build1/aria2c.exe" >"$DEST"
echo "got $(basename "$DEST")"
