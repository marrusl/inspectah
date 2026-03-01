#!/bin/bash
set -euo pipefail

IMAGE="ghcr.io/marrusl/yoinkc:latest"
OUTPUT_DIR="/home/mark/output"

echo "=== Preflight checks ==="
if ! command -v podman &>/dev/null; then
    echo "podman not found — installing..."
    if command -v dnf &>/dev/null; then
        dnf install -y podman
    elif command -v yum &>/dev/null; then
        yum install -y podman
    else
        echo "ERROR: podman is not installed and no supported package manager found." >&2
        exit 1
    fi
fi

mkdir -p "$OUTPUT_DIR"

echo "=== Pulling yoinkc image ==="
podman pull "$IMAGE"

echo "=== Cleaning output directory ==="
rm -rf "${OUTPUT_DIR:?}"/*

echo "=== Running yoinkc ==="
podman run --rm \
  --pid=host \
  --privileged \
  --security-opt label=disable \
  -e YOINKC_DEBUG=1 \
  -v /:/host:ro \
  -v "${OUTPUT_DIR}:/output:z" \
  "$IMAGE" --output-dir /output

echo "=== Packaging results ==="
STAMP="yoinkc-output-$(date +%Y%m%d-%H%M%S)"
TARBALL="/home/mark/${STAMP}.tar.gz"
tar -czf "$TARBALL" -C "$OUTPUT_DIR" --transform "s,^\.,$STAMP," .
echo "=== Done. Output in ${OUTPUT_DIR}, tarball at ${TARBALL} ==="
