#!/usr/bin/env bash
# Run all driftify profiles, yoinkc each, produce fleet-ready tarballs.
# Self-contained: fetches all scripts from GitHub; no local checkout required.
set -euo pipefail

PROFILES=(minimal standard kitchen-sink)
HOSTNAMES=(web-01 web-02 web-03)

DRIFTIFY_SCRIPT="$(mktemp)"
YOINKC_SCRIPT="$(mktemp)"
FLEET_DIR="$(mktemp -d -t fleet-aggregate.XXXXXX)"
curl -fsSL https://raw.githubusercontent.com/marrusl/driftify/refs/heads/main/driftify.py -o "$DRIFTIFY_SCRIPT"
curl -fsSL https://raw.githubusercontent.com/marrusl/yoinkc/refs/heads/main/run-yoinkc.sh -o "$YOINKC_SCRIPT"
chmod +x "$DRIFTIFY_SCRIPT" "$YOINKC_SCRIPT"
trap 'rm -f "$DRIFTIFY_SCRIPT" "$YOINKC_SCRIPT"; rm -rf "$FLEET_DIR"' EXIT

# Start from a clean slate (undo any previous driftify run)
echo "=== Undoing previous driftify state ==="
sudo "$DRIFTIFY_SCRIPT" --undo -yq

for i in "${!PROFILES[@]}"; do
    profile="${PROFILES[$i]}"
    hostname="${HOSTNAMES[$i]}"
    echo "=== Profile: $profile (hostname: $hostname) ==="
    sudo "$DRIFTIFY_SCRIPT" -yq --profile "$profile"
    YOINKC_HOSTNAME="$hostname" bash "$YOINKC_SCRIPT"
done

echo ""
echo "=== Aggregating fleet ==="
# shellcheck disable=SC2012
ls -1t *.tar.gz | head -3 | xargs -I{} cp {} "$FLEET_DIR/"
bash "$YOINKC_SCRIPT" fleet "$FLEET_DIR" -p 66

echo ""
echo "=== Fleet tarball ==="
realpath -- "$(ls -1t ./*.tar.gz | head -1)"
