#!/usr/bin/env bash
# verify-redaction-parity.sh -- Tier 1.4: Redaction engine parity
#
# Compares the snapshot.redactions arrays from Python and Go scans.
# Verifies:
#   - Same files are redacted
#   - Same secret types detected
#   - Same detection methods used
#   - Zero secrets missed by Go that Python caught (hard requirement)
#
# Counter token numbers (REDACTED_<TYPE>_<N>) may differ due to
# processing order -- that is acceptable.
#
# Usage:
#   ./scripts/verify/verify-redaction-parity.sh <python-output-dir> <go-output-dir>
#
# Requires: jq, diff

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

require_jq

# ── Args ────────────────────────────────────────────────────────────────────
if [ $# -ne 2 ]; then
  echo "Usage: $0 <python-output-dir> <go-output-dir>"
  exit 1
fi

PY_DIR="$1"
GO_DIR="$2"

PY_SNAP="$PY_DIR/inspection-snapshot.json"
GO_SNAP="$GO_DIR/inspection-snapshot.json"

for f in "$PY_SNAP" "$GO_SNAP"; do
  if [ ! -f "$f" ]; then
    echo "ERROR: snapshot not found: $f" >&2
    exit 1
  fi
done

WORK_DIR=$(mktemp -d)
trap "rm -rf '$WORK_DIR'" EXIT

OVERALL=0  # 0 = pass, 1 = fail

# ── Extract redactions ──────────────────────────────────────────────────────
header "Redaction Engine Parity (Tier 1.4)"

PY_REDACTIONS="$WORK_DIR/py-redactions.json"
GO_REDACTIONS="$WORK_DIR/go-redactions.json"

jq '.redactions // []' "$PY_SNAP" > "$PY_REDACTIONS"
jq '.redactions // []' "$GO_SNAP" > "$GO_REDACTIONS"

PY_COUNT=$(jq 'length' "$PY_REDACTIONS")
GO_COUNT=$(jq 'length' "$GO_REDACTIONS")

info "Python redactions: $PY_COUNT findings"
info "Go redactions:     $GO_COUNT findings"

# ── Check 1: Same files redacted ────────────────────────────────────────────
header "Check 1: Redacted file paths"

jq -r '.[].path // empty' "$PY_REDACTIONS" | sort -u > "$WORK_DIR/py-paths.txt"
jq -r '.[].path // empty' "$GO_REDACTIONS" | sort -u > "$WORK_DIR/go-paths.txt"

PY_PATH_COUNT=$(wc -l < "$WORK_DIR/py-paths.txt" | tr -d ' ')
GO_PATH_COUNT=$(wc -l < "$WORK_DIR/go-paths.txt" | tr -d ' ')

# Files Python found but Go missed (CRITICAL)
MISSED=$(comm -23 "$WORK_DIR/py-paths.txt" "$WORK_DIR/go-paths.txt")
# Files Go found but Python missed (informational, not a failure)
EXTRA=$(comm -13 "$WORK_DIR/py-paths.txt" "$WORK_DIR/go-paths.txt")

if [ -z "$MISSED" ]; then
  pass "No files missed by Go (Python: $PY_PATH_COUNT, Go: $GO_PATH_COUNT paths)"
else
  fail "Go missed files that Python redacted:"
  echo "$MISSED" | sed 's/^/    /'
  OVERALL=1
fi

if [ -n "$EXTRA" ]; then
  info "Go found additional files not in Python (not a failure):"
  echo "$EXTRA" | sed 's/^/    /'
fi

# ── Check 2: Same types detected ───────────────────────────────────────────
header "Check 2: Secret types detected"

jq -r '.[].kind // .[].type // empty' "$PY_REDACTIONS" | sort -u > "$WORK_DIR/py-types.txt"
jq -r '.[].kind // .[].type // empty' "$GO_REDACTIONS" | sort -u > "$WORK_DIR/go-types.txt"

MISSED_TYPES=$(comm -23 "$WORK_DIR/py-types.txt" "$WORK_DIR/go-types.txt")
EXTRA_TYPES=$(comm -13 "$WORK_DIR/py-types.txt" "$WORK_DIR/go-types.txt")

if [ -z "$MISSED_TYPES" ]; then
  pass "All secret types covered"
  info "Types: $(paste -sd', ' "$WORK_DIR/py-types.txt")"
else
  fail "Go missed secret types:"
  echo "$MISSED_TYPES" | sed 's/^/    /'
  OVERALL=1
fi

if [ -n "$EXTRA_TYPES" ]; then
  info "Go detected additional types: $(echo "$EXTRA_TYPES" | paste -sd', ')"
fi

# ── Check 3: Per-file type coverage ────────────────────────────────────────
header "Check 3: Per-file secret detection (hard requirement)"
info "Verifying every (path, kind) pair from Python exists in Go output"

# Build path+kind pairs for comparison
jq -r '.[] | "\(.path // "")|\(.kind // .type // "")"' "$PY_REDACTIONS" | sort -u > "$WORK_DIR/py-pairs.txt"
jq -r '.[] | "\(.path // "")|\(.kind // .type // "")"' "$GO_REDACTIONS" | sort -u > "$WORK_DIR/go-pairs.txt"

MISSED_PAIRS=$(comm -23 "$WORK_DIR/py-pairs.txt" "$WORK_DIR/go-pairs.txt")

if [ -z "$MISSED_PAIRS" ]; then
  PAIR_COUNT=$(wc -l < "$WORK_DIR/py-pairs.txt" | tr -d ' ')
  pass "All $PAIR_COUNT (path, kind) pairs matched"
else
  MISSED_PAIR_COUNT=$(echo "$MISSED_PAIRS" | wc -l | tr -d ' ')
  fail "Go missed $MISSED_PAIR_COUNT (path, kind) pairs that Python detected:"
  echo "$MISSED_PAIRS" | head -20 | sed 's/|/ -> /; s/^/    /'
  if [ "$MISSED_PAIR_COUNT" -gt 20 ]; then
    echo "    ... and $((MISSED_PAIR_COUNT - 20)) more"
  fi
  OVERALL=1
fi

# ── Check 4: Detection methods ─────────────────────────────────────────────
header "Check 4: Detection methods (informational)"

jq -r '.[].detection_method // empty' "$PY_REDACTIONS" | sort -u > "$WORK_DIR/py-methods.txt"
jq -r '.[].detection_method // empty' "$GO_REDACTIONS" | sort -u > "$WORK_DIR/go-methods.txt"

if [ -s "$WORK_DIR/py-methods.txt" ] || [ -s "$WORK_DIR/go-methods.txt" ]; then
  info "Python methods: $(paste -sd', ' "$WORK_DIR/py-methods.txt" 2>/dev/null || echo "none")"
  info "Go methods:     $(paste -sd', ' "$WORK_DIR/go-methods.txt" 2>/dev/null || echo "none")"
else
  info "No detection_method field found in either snapshot (may use a different field name)"
fi

# ── Summary ─────────────────────────────────────────────────────────────────
header "Redaction Parity Summary"
echo ""
echo -e "  Python findings:  $PY_COUNT"
echo -e "  Go findings:      $GO_COUNT"
echo ""

if [ "$OVERALL" -eq 0 ]; then
  pass "Redaction parity: zero secrets missed by Go"
  exit 0
else
  fail "Redaction parity: Go missed secrets that Python detected"
  exit 1
fi
