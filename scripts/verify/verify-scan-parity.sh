#!/usr/bin/env bash
# verify-scan-parity.sh -- Tier 1.3 + 1.5: Side-by-side scan comparison
#
# Compares Python and Go inspection snapshots section-by-section.
# Also covers baseline resolution parity (section 1.5) since the
# baseline data is embedded in the rpm section (packages_added,
# base_image_only, leaf_packages).
#
# Usage:
#   ./scripts/verify/verify-scan-parity.sh <python-output-dir> <go-output-dir>
#
# Example:
#   sudo inspectah scan --inspect-only --output-dir /tmp/parity-python
#   sudo ./inspectah scan --inspect-only --output-dir /tmp/parity-go
#   ./scripts/verify/verify-scan-parity.sh /tmp/parity-python /tmp/parity-go
#
# Requires: jq, diff (standard on RHEL 9)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

require_jq

# ── Args ────────────────────────────────────────────────────────────────────
if [ $# -ne 2 ]; then
  echo "Usage: $0 <python-output-dir> <go-output-dir>"
  echo ""
  echo "  python-output-dir  Directory containing Python inspection-snapshot.json"
  echo "  go-output-dir      Directory containing Go inspection-snapshot.json"
  exit 1
fi

PY_DIR="$1"
GO_DIR="$2"

PY_SNAP="$PY_DIR/inspection-snapshot.json"
GO_SNAP="$GO_DIR/inspection-snapshot.json"

# ── Validate inputs ────────────────────────────────────────────────────────
for f in "$PY_SNAP" "$GO_SNAP"; do
  if [ ! -f "$f" ]; then
    echo "ERROR: snapshot not found: $f" >&2
    exit 1
  fi
done

# ── Normalize ───────────────────────────────────────────────────────────────
WORK_DIR=$(mktemp -d)
trap "rm -rf '$WORK_DIR'" EXIT

PY_NORM="$WORK_DIR/py-normalized.json"
GO_NORM="$WORK_DIR/go-normalized.json"

header "Normalizing snapshots"
normalize_snapshot "$PY_SNAP" "$PY_NORM"
info "Python snapshot normalized"
normalize_snapshot "$GO_SNAP" "$GO_NORM"
info "Go snapshot normalized"

# ── Schema version check ───────────────────────────────────────────────────
header "Schema version check"
PY_SCHEMA=$(jq -r '.meta.schema_version // "unknown"' "$PY_SNAP")
GO_SCHEMA=$(jq -r '.meta.schema_version // "unknown"' "$GO_SNAP")

if [ "$PY_SCHEMA" = "$GO_SCHEMA" ]; then
  pass "schema_version: both are $PY_SCHEMA"
else
  fail "schema_version: Python=$PY_SCHEMA, Go=$GO_SCHEMA"
fi

# ── Meta block (informational, not pass/fail) ───────────────────────────────
header "Meta block (informational)"
PY_VER=$(jq -r '.meta.tool_version // "unknown"' "$PY_SNAP")
GO_VER=$(jq -r '.meta.tool_version // "unknown"' "$GO_SNAP")
info "Python tool_version: $PY_VER"
info "Go tool_version:     $GO_VER"
info "(Version difference is expected: Python 0.6.x vs Go 0.7.x)"

# ── Section-by-section comparison ───────────────────────────────────────────
header "Inspector section comparison"
TOTAL=0
PASSED=0
FAILED=0
FAILED_SECTIONS=()

for section in "${SECTIONS[@]}"; do
  TOTAL=$((TOTAL + 1))
  if compare_section "$PY_NORM" "$GO_NORM" "$section"; then
    PASSED=$((PASSED + 1))
  else
    FAILED=$((FAILED + 1))
    FAILED_SECTIONS+=("$section")
  fi
done

# ── Baseline resolution parity (section 1.5) ───────────────────────────────
header "Baseline resolution parity (Tier 1.5)"
info "Baseline data is in the rpm section: packages_added, base_image_only, leaf_packages"

# Extract and compare baseline-specific fields
BASELINE_FIELDS=(packages_added base_image_only leaf_packages)
for field in "${BASELINE_FIELDS[@]}"; do
  py_val=$(jq --arg f "$field" '.rpm[$f] // null' "$PY_NORM" 2>/dev/null || echo "null")
  go_val=$(jq --arg f "$field" '.rpm[$f] // null' "$GO_NORM" 2>/dev/null || echo "null")

  # Count items if it is an array
  py_count=$(echo "$py_val" | jq 'if type == "array" then length else 0 end' 2>/dev/null || echo "?")
  go_count=$(echo "$go_val" | jq 'if type == "array" then length else 0 end' 2>/dev/null || echo "?")

  info "rpm.$field: Python=$py_count items, Go=$go_count items"
done

if [ "$FAILED" -eq 0 ]; then
  info "Baseline fields validated as part of rpm section PASS"
else
  # Check if rpm specifically failed
  for s in "${FAILED_SECTIONS[@]}"; do
    if [ "$s" = "rpm" ]; then
      warn "rpm section failed -- baseline resolution may be affected. Review diff above."
      break
    fi
  done
fi

# ── Summary ─────────────────────────────────────────────────────────────────
header "Scan Parity Summary"
echo ""
echo -e "  Sections tested:  $TOTAL"
echo -e "  ${GREEN}Passed:${RESET}           $PASSED"
echo -e "  ${RED}Failed:${RESET}           $FAILED"

if [ "$FAILED" -gt 0 ]; then
  echo ""
  echo -e "  ${RED}Failed sections:${RESET} ${FAILED_SECTIONS[*]}"
  echo ""
  fail "Scan parity: $FAILED section(s) differ"
  exit 1
else
  echo ""
  pass "Scan parity: all $TOTAL sections match"
  exit 0
fi
