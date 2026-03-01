#!/bin/bash
set -euo pipefail

REPO_DIR="/home/mark/yoinkc"
OUTPUT_DIR="/home/mark/output"
IMAGE_NAME="yoinkc"
REPO_URL="https://github.com/marrusl/yoinkc.git"

echo "=== Cleaning up ==="
rm -rf "$REPO_DIR"
rm -rf "${OUTPUT_DIR:?}"/*

echo "=== Cloning yoinkc (main) ==="
git clone -b main "$REPO_URL" "$REPO_DIR"

echo "=== Building container image ==="
podman build -t "${IMAGE_NAME}:latest" "$REPO_DIR"

echo "=== Running yoinkc ==="
sudo podman run --rm \
  --pid=host \
  --privileged \
  --security-opt label=disable \
  -e YOINKC_DEBUG=1 \
  -v /:/host:ro \
  -v "${OUTPUT_DIR}:/output:z" \
  "$IMAGE_NAME:latest" --output-dir /output

echo "=== Packaging results ==="
TARBALL="/home/mark/yoinkc-output-$(date +%Y%m%d-%H%M%S).tar.gz"
tar -czf "$TARBALL" -C "$OUTPUT_DIR" .
echo "=== Done. Output in ${OUTPUT_DIR}, tarball at ${TARBALL} ==="
