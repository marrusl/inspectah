#!/usr/bin/env bash
# Run all driftify profiles, inspectah each, produce fleet-ready tarballs.
# Self-contained: fetches all scripts from GitHub; no local checkout required.
set -euo pipefail

PROFILES=(minimal standard kitchen-sink)
HOSTNAMES=(web-01 web-02 web-03)

DRIFTIFY_SCRIPT="$(mktemp)"
INSPECTAH_SCRIPT="$(mktemp)"
FLEET_DIR="$(mktemp -d -t fleet-aggregate.XXXXXX)"
curl -fsSL https://raw.githubusercontent.com/marrusl/driftify/refs/heads/main/driftify.py -o "$DRIFTIFY_SCRIPT"
curl -fsSL https://raw.githubusercontent.com/marrusl/inspectah/refs/heads/main/run-inspectah.sh -o "$INSPECTAH_SCRIPT"
chmod +x "$DRIFTIFY_SCRIPT" "$INSPECTAH_SCRIPT"
trap 'rm -f "$DRIFTIFY_SCRIPT" "$INSPECTAH_SCRIPT"; rm -rf "$FLEET_DIR"' EXIT

# Start from a clean slate (undo any previous driftify run)
echo "=== Undoing previous driftify state ==="
sudo "$DRIFTIFY_SCRIPT" --undo -yq

for i in "${!PROFILES[@]}"; do
    profile="${PROFILES[$i]}"
    hostname="${HOSTNAMES[$i]}"
    echo "=== Profile: $profile (hostname: $hostname) ==="
    sudo "$DRIFTIFY_SCRIPT" -yq --profile "$profile"
    INSPECTAH_HOSTNAME="$hostname" bash "$INSPECTAH_SCRIPT"
done

echo ""
echo "=== Aggregating fleet ==="
# shellcheck disable=SC2012
ls -1t *.tar.gz | head -3 | xargs -I{} cp {} "$FLEET_DIR/"
bash "$INSPECTAH_SCRIPT" fleet "$FLEET_DIR" -p 66

echo ""
echo "=== Fleet tarball ==="
realpath -- "$(ls -1t ./*.tar.gz | head -1)"
