#!/usr/bin/env bash
# Architect demo pipeline:
#   driftify topology → bundle fleet tarballs → (optional) launch architect
# Self-contained: fetches driftify from GitHub; no local checkout required.
# No sudo, no containers — topology generation is pure file output.
set -euo pipefail

TOPOLOGY="${1:-three-role-overlap}"

DRIFTIFY_SCRIPT="$(mktemp)"
TMPDIR="$(mktemp -d -t architect-test.XXXXXX)"
FLEET_DIR="$TMPDIR/fleets"

curl -fsSL https://raw.githubusercontent.com/marrusl/driftify/refs/heads/main/driftify.py -o "$DRIFTIFY_SCRIPT"
chmod +x "$DRIFTIFY_SCRIPT"

trap 'rm -f "$DRIFTIFY_SCRIPT"; rm -rf "$TMPDIR"' EXIT

# ── Step 1: Generate fleet-ready tarballs ─────────────────────────────────

echo "=== Step 1: Generate topology '$TOPOLOGY' ==="
python3 "$DRIFTIFY_SCRIPT" topology "$TOPOLOGY" "$FLEET_DIR"
echo "  Fleet tarballs:"
ls -1 "$FLEET_DIR"/*.tar.gz
echo ""

# ── Step 2: Bundle fleet tarballs ─────────────────────────────────────────

echo "=== Step 2: Bundle fleet tarballs ==="
BUNDLE="$TMPDIR/architect-demo-bundle.tar.gz"

cat > "$TMPDIR/README.txt" <<'READMEEOF'
Architect Demo Bundle
=====================

This archive contains fleet tarballs produced by driftify topology.
Each .tar.gz holds one fleet's inspection-snapshot.json, ready for
inspectah architect directly.

Usage:

  # Pass the bundle directly:
  inspectah architect architect-demo-bundle.tar.gz

  # Or extract first and pass the directory:
  tar xzf architect-demo-bundle.tar.gz -C ./fleet-tarballs/
  inspectah architect ./fleet-tarballs/
READMEEOF

tar czf "$BUNDLE" -C "$FLEET_DIR" . -C "$TMPDIR" README.txt
echo "Bundle: $BUNDLE"
echo "Contents:"
tar tzf "$BUNDLE"
echo ""

# ── Step 3: Launch architect (if inspectah is available) ─────────────────────

if command -v inspectah &>/dev/null; then
    echo "=== Step 3: Launch architect (from bundle) ==="

    # Keep tmpdir alive while architect runs
    trap - EXIT

    echo "Starting architect UI (Ctrl-C to stop)..."
    echo "  URL: http://127.0.0.1:8643"
    echo ""
    inspectah architect "$BUNDLE" --no-browser
else
    # Copy bundle to cwd so it survives temp cleanup
    cp "$BUNDLE" ./architect-demo-bundle.tar.gz
    echo "=== Done ==="
    echo "Bundle saved to: $(pwd)/architect-demo-bundle.tar.gz"
    echo ""
    echo "Copy to your workstation and run:"
    echo "  scp $(hostname):$(pwd)/architect-demo-bundle.tar.gz ."
    echo "  inspectah architect architect-demo-bundle.tar.gz"
fi
