#!/bin/sh
set -eu

HOST_ROOT="/"
IMAGE="${YOINKC_IMAGE:-ghcr.io/marrusl/yoinkc:latest}"
OUTPUT_DIR="${1:-${YOINKC_OUTPUT:-./yoinkc-output}}"
shift 2>/dev/null || true

if ! command -v podman >/dev/null 2>&1; then
    echo "podman not found — installing..." >&2
    if command -v dnf >/dev/null 2>&1; then
        dnf install -y podman
    elif command -v yum >/dev/null 2>&1; then
        yum install -y podman
    else
        echo "ERROR: podman is not installed and no supported package manager found." >&2
        exit 1
    fi
fi

mkdir -p "$OUTPUT_DIR"
OUTPUT_DIR="$(cd "$OUTPUT_DIR" && pwd)"

echo "Image:  $IMAGE"
echo "Output: $OUTPUT_DIR"

echo "=== Pulling yoinkc image ==="
podman pull "$IMAGE"

echo "=== Running yoinkc ==="
podman run --rm \
  --pid=host \
  --privileged \
  --security-opt label=disable \
  -v "${HOST_ROOT}:/host:ro" \
  -v "${OUTPUT_DIR}:/output:z" \
  "$IMAGE" --output-dir /output "$@"

echo "=== Packaging results ==="
HOSTNAME="$(hostname -s 2>/dev/null || cat /etc/hostname 2>/dev/null || echo unknown)"
STAMP="${HOSTNAME}-$(date +%Y%m%d-%H%M%S)"
TARBALL="${OUTPUT_DIR}/../${STAMP}.tar.gz"
tar -czf "$TARBALL" -C "$OUTPUT_DIR" --transform "s,^\.,$STAMP," .
echo "=== Done. Output in ${OUTPUT_DIR}, tarball at ${TARBALL} ==="
