#!/usr/bin/env bash
# Run a command inside the build container (endl-build, or ENViewer's lumen-build if that is all there is) with the project mounted at /src.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
mkdir -p "$HOME/.cargo/registry"
IMAGE="${ENDL_IMAGE:-endl-build}"
docker image inspect "$IMAGE" >/dev/null 2>&1 || IMAGE=lumen-build
# A passwd entry for the host user (dbus refuses to start for an unknown UID).
mkdir -p "$ROOT/target"
{ echo "root:x:0:0:root:/root:/bin/bash"; echo "builder:x:$(id -u):$(id -g):builder:/tmp/home:/bin/bash"; } >"$ROOT/target/.passwd"
exec docker run --rm -i --init \
  --user "$(id -u):$(id -g)" -e HOME=/tmp/home -e CARGO_INCREMENTAL=0 \
  -v "$ROOT/target/.passwd":/etc/passwd:ro \
  -v "$ROOT":/src -v "$HOME/.cargo/registry":/opt/cargo/registry \
  -w /src "$IMAGE" bash -lc "mkdir -p /tmp/home && $*"
