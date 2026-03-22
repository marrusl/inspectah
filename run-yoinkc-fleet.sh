#!/bin/bash
set -euo pipefail

IMAGE="${YOINKC_IMAGE:-ghcr.io/marrusl/yoinkc:latest}"
OUTPUT_DIR="${YOINKC_OUTPUT_DIR:-$(pwd)}"

if [ $# -eq 0 ]; then
    echo "Usage: $(basename "$0") <input-dir> [yoinkc-fleet-args...]" >&2
    echo "" >&2
    echo "Runs yoinkc fleet inside the yoinkc container." >&2
    echo "" >&2
    echo "  <input-dir>  Directory containing yoinkc tarballs or JSON snapshots" >&2
    echo "" >&2
    echo "Environment:" >&2
    echo "  YOINKC_IMAGE        Container image  (default: $IMAGE)" >&2
    echo "  YOINKC_OUTPUT_DIR   Destination directory for output tarball (default: CWD)" >&2
    exit 1
fi

if ! command -v podman >/dev/null 2>&1; then
    echo "ERROR: podman is not installed." >&2
    exit 1
fi

INPUT_DIR="$(cd "$1" && pwd)"
DIR_NAME="$(basename "$INPUT_DIR")"
shift

TEMP_OUT="$(mktemp -d)"
trap 'rm -rf "$TEMP_OUT"' EXIT

echo "Image: $IMAGE"
echo "=== Running yoinkc fleet ==="
podman run --rm --pull=always \
    --security-opt label=disable \
    --entrypoint yoinkc \
    -w /output \
    -v "$INPUT_DIR":/input:ro \
    -v "$TEMP_OUT":/output \
    "$IMAGE" fleet /input -o "/output/${DIR_NAME}.tar.gz" "$@"

cp "$TEMP_OUT/${DIR_NAME}.tar.gz" "$OUTPUT_DIR/"
echo ""
echo "Output: $OUTPUT_DIR/${DIR_NAME}.tar.gz"
