#!/usr/bin/env bash
# verify-fleet-parity.sh -- Tier 1.6: Fleet merge parity
#
# Compares Python and Go fleet merge output. Runs both fleet commands
# against the same input directory and diffs the merged snapshots.
#
# Usage:
#   ./scripts/verify/verify-fleet-parity.sh <fleet-input-dir> [python-cmd] [go-cmd]
#
# Arguments:
#   fleet-input-dir   Directory containing 2+ scan tarballs or JSON snapshots
#   python-cmd        Python inspectah command (default: "inspectah")
#   go-cmd            Go inspectah binary path (default: "./inspectah")
#
# Example:
#   ./scripts/verify/verify-fleet-parity.sh /tmp/fleet-inputs inspectah ./inspectah
#
# The script will:
#   1. Run Python fleet merge with --json-only
#   2. Run Go fleet merge with --json-only
#   3. Normalize and diff the fleet-snapshot.json files
#
# Requires: jq, diff

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

require_jq

# ── Args ────────────────────────────────────────────────────────────────────
if [ $# -lt 1 ]; then
  echo "Usage: $0 <fleet-input-dir> [python-cmd] [go-cmd]"
  echo ""
  echo "  fleet-input-dir  Directory with 2+ scan tarballs or JSON snapshots"
  echo "  python-cmd       Python inspectah command (default: inspectah)"
  echo "  go-cmd           Go binary path (default: ./inspectah)"
  exit 1
fi

FLEET_INPUT="$1"
PY_CMD="${2:-inspectah}"
GO_CMD="${3:-./inspectah}"

if [ ! -d "$FLEET_INPUT" ]; then
  echo "ERROR: fleet input directory not found: $FLEET_INPUT" >&2
  exit 1
fi

# Count inputs
INPUT_COUNT=$(find "$FLEET_INPUT" -maxdepth 1 \( -name '*.tar.gz' -o -name '*.tgz' -o -name '*.json' \) | wc -l | tr -d ' ')
if [ "$INPUT_COUNT" -lt 2 ]; then
  echo "ERROR: need at least 2 scan tarballs/snapshots in $FLEET_INPUT (found $INPUT_COUNT)" >&2
  exit 1
fi
info "Found $INPUT_COUNT scan inputs in $FLEET_INPUT"

WORK_DIR=$(mktemp -d)
trap "rm -rf '$WORK_DIR'" EXIT

PY_OUT="$WORK_DIR/fleet-python"
GO_OUT="$WORK_DIR/fleet-go"
mkdir -p "$PY_OUT" "$GO_OUT"

OVERALL=0

# ── Run fleet commands ──────────────────────────────────────────────────────
header "Running fleet merges"

info "Python: $PY_CMD fleet $FLEET_INPUT --json-only --output-dir $PY_OUT"
if ! $PY_CMD fleet "$FLEET_INPUT" --json-only --output-dir "$PY_OUT" 2>"$WORK_DIR/py-stderr.log"; then
  fail "Python fleet command failed"
  cat "$WORK_DIR/py-stderr.log" | head -10
  OVERALL=1
else
  pass "Python fleet merge completed"
fi

info "Go: $GO_CMD fleet $FLEET_INPUT --json-only --output-dir $GO_OUT"
if ! $GO_CMD fleet "$FLEET_INPUT" --json-only --output-dir "$GO_OUT" 2>"$WORK_DIR/go-stderr.log"; then
  fail "Go fleet command failed"
  cat "$WORK_DIR/go-stderr.log" | head -10
  OVERALL=1
else
  pass "Go fleet merge completed"
fi

if [ "$OVERALL" -ne 0 ]; then
  fail "Cannot compare: one or both fleet commands failed"
  exit 1
fi

# ── Locate fleet snapshots ──────────────────────────────────────────────────
# Fleet output may be fleet-snapshot.json or inspection-snapshot.json
PY_SNAP=""
GO_SNAP=""

for name in fleet-snapshot.json inspection-snapshot.json; do
  [ -z "$PY_SNAP" ] && [ -f "$PY_OUT/$name" ] && PY_SNAP="$PY_OUT/$name"
  [ -z "$GO_SNAP" ] && [ -f "$GO_OUT/$name" ] && GO_SNAP="$GO_OUT/$name"
done

if [ -z "$PY_SNAP" ]; then
  fail "Python fleet snapshot not found in $PY_OUT"
  ls -la "$PY_OUT"
  exit 1
fi

if [ -z "$GO_SNAP" ]; then
  fail "Go fleet snapshot not found in $GO_OUT"
  ls -la "$GO_OUT"
  exit 1
fi

info "Python snapshot: $PY_SNAP"
info "Go snapshot:     $GO_SNAP"

# ── Normalize ───────────────────────────────────────────────────────────────
header "Normalizing fleet snapshots"

PY_NORM="$WORK_DIR/py-fleet-norm.json"
GO_NORM="$WORK_DIR/go-fleet-norm.json"

normalize_snapshot "$PY_SNAP" "$PY_NORM"
normalize_snapshot "$GO_SNAP" "$GO_NORM"

# ── Section comparison ──────────────────────────────────────────────────────
header "Fleet snapshot section comparison"
TOTAL=0
PASSED=0
FAILED=0
FAILED_SECTIONS=()

for section in "${SECTIONS[@]}"; do
  # Skip sections that might not exist in fleet snapshots
  py_has=$(jq --arg s "$section" 'has($s)' "$PY_NORM")
  go_has=$(jq --arg s "$section" 'has($s)' "$GO_NORM")

  if [ "$py_has" = "false" ] && [ "$go_has" = "false" ]; then
    continue
  fi

  TOTAL=$((TOTAL + 1))
  if compare_section "$PY_NORM" "$GO_NORM" "$section"; then
    PASSED=$((PASSED + 1))
  else
    FAILED=$((FAILED + 1))
    FAILED_SECTIONS+=("$section")
  fi
done

# ── Fleet metadata comparison ──────────────────────────────────────────────
header "Fleet metadata comparison"

# Compare fleet-specific metadata (host count, hostnames)
PY_META=$(jq '.meta.fleet // empty' "$PY_NORM" 2>/dev/null || echo "null")
GO_META=$(jq '.meta.fleet // empty' "$GO_NORM" 2>/dev/null || echo "null")

if [ "$PY_META" != "null" ] || [ "$GO_META" != "null" ]; then
  # Compare host count
  PY_HOSTS=$(echo "$PY_META" | jq '.host_count // .total // 0' 2>/dev/null || echo "?")
  GO_HOSTS=$(echo "$GO_META" | jq '.host_count // .total // 0' 2>/dev/null || echo "?")

  if [ "$PY_HOSTS" = "$GO_HOSTS" ]; then
    pass "Fleet host count: $PY_HOSTS"
  else
    fail "Fleet host count: Python=$PY_HOSTS, Go=$GO_HOSTS"
    OVERALL=1
  fi

  # Compare hostnames (sorted)
  PY_NAMES=$(echo "$PY_META" | jq -r '.hostnames // .hosts // [] | sort | .[]' 2>/dev/null || true)
  GO_NAMES=$(echo "$GO_META" | jq -r '.hostnames // .hosts // [] | sort | .[]' 2>/dev/null || true)

  if [ "$PY_NAMES" = "$GO_NAMES" ]; then
    pass "Fleet hostnames match"
  else
    warn "Fleet hostnames differ (may be display-name formatting)"
    info "Python: $PY_NAMES"
    info "Go:     $GO_NAMES"
  fi
else
  info "No fleet metadata block found (may be in a different structure)"
fi

# ── Summary ─────────────────────────────────────────────────────────────────
header "Fleet Parity Summary"
echo ""
echo -e "  Sections tested:  $TOTAL"
echo -e "  ${GREEN}Passed:${RESET}           $PASSED"
echo -e "  ${RED}Failed:${RESET}           $FAILED"

if [ "$FAILED" -gt 0 ]; then
  echo ""
  echo -e "  ${RED}Failed sections:${RESET} ${FAILED_SECTIONS[*]}"
  OVERALL=1
fi

echo ""
if [ "$OVERALL" -eq 0 ]; then
  pass "Fleet parity: all sections match"
  exit 0
else
  fail "Fleet parity: differences detected"
  exit 1
fi
