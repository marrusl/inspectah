#!/usr/bin/env bash
# End-to-end architect demo pipeline:
#   driftify topology → yoinkc inspect → fleet → architect
# Self-contained: fetches all scripts from GitHub; no local checkout required.
set -euo pipefail

TOPOLOGY="three-role-overlap"

DRIFTIFY_SCRIPT="$(mktemp)"
YOINKC_SCRIPT="$(mktemp)"
TMPDIR="$(mktemp -d -t architect-test.XXXXXX)"
RAW_DIR="$TMPDIR/raw"
INSPECT_DIR="$TMPDIR/inspected"
FLEET_DIR="$TMPDIR/fleets"

curl -fsSL https://raw.githubusercontent.com/marrusl/driftify/refs/heads/main/driftify.py -o "$DRIFTIFY_SCRIPT"
curl -fsSL https://raw.githubusercontent.com/marrusl/yoinkc/refs/heads/main/run-yoinkc.sh -o "$YOINKC_SCRIPT"
chmod +x "$DRIFTIFY_SCRIPT" "$YOINKC_SCRIPT"

trap 'rm -f "$DRIFTIFY_SCRIPT" "$YOINKC_SCRIPT"; rm -rf "$TMPDIR"' EXIT

# ── Step 1: Generate topology fixtures ──────────────────────────────────────

echo "=== Step 1: Generate topology '$TOPOLOGY' ==="
python3 "$DRIFTIFY_SCRIPT" topology "$TOPOLOGY" "$RAW_DIR"
echo ""

# ── Step 2: Inspect each host snapshot ──────────────────────────────────────

echo "=== Step 2: Inspect host snapshots ==="
mkdir -p "$INSPECT_DIR"

for fleet_dir in "$RAW_DIR"/*/; do
    fleet_name="$(basename "$fleet_dir")"
    fleet_inspect_dir="$INSPECT_DIR/$fleet_name"
    mkdir -p "$fleet_inspect_dir"

    for host_json in "$fleet_dir"*.json; do
        host_name="$(basename "$host_json" .json)"
        echo "  Inspecting $fleet_name/$host_name..."
        bash "$YOINKC_SCRIPT" inspect --from-snapshot "$host_json" -o "$fleet_inspect_dir/${host_name}.tar.gz"
    done
done
echo ""

# ── Step 3: Build fleet tarballs ────────────────────────────────────────────

echo "=== Step 3: Build fleet tarballs ==="
mkdir -p "$FLEET_DIR"

for fleet_inspect_dir in "$INSPECT_DIR"/*/; do
    fleet_name="$(basename "$fleet_inspect_dir")"
    echo "  Building fleet: $fleet_name..."
    bash "$YOINKC_SCRIPT" fleet "$fleet_inspect_dir" -o "$FLEET_DIR/${fleet_name}.tar.gz"
done
echo ""

# ── Step 4: Bundle fleet tarballs ──────────────────────────────────────────

echo "=== Step 4: Bundle fleet tarballs ==="
BUNDLE="$TMPDIR/architect-demo-bundle.tar.gz"

# Create a README for the bundle
cat > "$TMPDIR/README.txt" <<'READMEEOF'
Architect Demo Bundle
=====================

This archive contains refined fleet tarballs produced by the yoinkc
inspect + fleet pipeline.  Each .tar.gz inside holds one fleet's
inspection-snapshot.json.

Usage:

  # Pass the bundle directly:
  yoinkc architect architect-demo-bundle.tar.gz

  # Or extract first and pass the directory:
  tar xzf architect-demo-bundle.tar.gz -C ./fleet-tarballs/
  yoinkc architect ./fleet-tarballs/
READMEEOF

# Build the bundle (fleet tarballs + README)
tar czf "$BUNDLE" -C "$FLEET_DIR" . -C "$TMPDIR" README.txt
echo "Bundle: $BUNDLE"
echo "Contents:"
tar tzf "$BUNDLE"
echo ""

# ── Step 5: Launch architect against bundle ────────────────────────────────

echo "=== Step 5: Launch architect (from bundle) ==="

# Disable cleanup trap so tmpdir survives while architect runs
trap - EXIT

echo "Starting architect UI (Ctrl-C to stop)..."
echo "  URL: http://127.0.0.1:8643"
echo ""
bash "$YOINKC_SCRIPT" architect "$BUNDLE" --no-browser
