#!/usr/bin/env bash
# run-tier1-verification.sh -- Tier 1 orchestrator
#
# Runs Python scan, Go scan, then all Tier 1 comparison scripts.
# Prints a final summary with PASS/FAIL per check.
#
# Usage:
#   sudo ./scripts/verify/run-tier1-verification.sh [options]
#
# Options:
#   --python-cmd CMD     Python inspectah command (default: inspectah)
#   --go-cmd CMD         Go binary path (default: ./inspectah)
#   --python-dir DIR     Use existing Python output (skip Python scan)
#   --go-dir DIR         Use existing Go output (skip Go scan)
#   --fleet-dir DIR      Directory with fleet input tarballs (skip fleet if absent)
#   --scan-only          Run scan comparison only, skip fleet
#   --skip-build         Skip Go binary build step
#   -h, --help           Show this help
#
# Requires: jq, diff, sudo (for scans)
#
# The script must be run from the inspectah repository root.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

require_jq

# ── Defaults ────────────────────────────────────────────────────────────────
PY_CMD="inspectah"
GO_CMD="./inspectah"
PY_DIR=""
GO_DIR=""
FLEET_DIR=""
SCAN_ONLY=false
SKIP_BUILD=false

# ── Parse args ──────────────────────────────────────────────────────────────
while [ $# -gt 0 ]; do
  case "$1" in
    --python-cmd)  PY_CMD="$2"; shift 2 ;;
    --go-cmd)      GO_CMD="$2"; shift 2 ;;
    --python-dir)  PY_DIR="$2"; shift 2 ;;
    --go-dir)      GO_DIR="$2"; shift 2 ;;
    --fleet-dir)   FLEET_DIR="$2"; shift 2 ;;
    --scan-only)   SCAN_ONLY=true; shift ;;
    --skip-build)  SKIP_BUILD=true; shift ;;
    -h|--help)
      head -20 "$0" | grep '^#' | sed 's/^# \?//'
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

# ── Banner ──────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}================================================================${RESET}"
echo -e "${BOLD}  inspectah Go Port - Tier 1 Verification${RESET}"
echo -e "${BOLD}================================================================${RESET}"
echo ""
echo "  Date:       $(date '+%Y-%m-%d %H:%M:%S')"
echo "  Hostname:   $(hostname)"
echo "  Python cmd: $PY_CMD"
echo "  Go cmd:     $GO_CMD"
echo ""

RESULTS=()  # array of "CHECK_NAME:PASS" or "CHECK_NAME:FAIL"

record_result() {
  local name="$1" status="$2"
  RESULTS+=("$name:$status")
}

# ── Step 0: Build Go binary ────────────────────────────────────────────────
if [ "$SKIP_BUILD" = false ]; then
  header "Building Go binary"
  if command -v go &>/dev/null; then
    if go build -o inspectah ./cmd/inspectah 2>&1; then
      pass "Go binary built: ./inspectah"
    else
      fail "Go build failed"
      exit 1
    fi
  else
    warn "Go not installed -- assuming binary already exists"
    if [ ! -f "$GO_CMD" ]; then
      fail "Go binary not found at $GO_CMD"
      exit 1
    fi
  fi
fi

# ── Step 1: Python scan ────────────────────────────────────────────────────
if [ -z "$PY_DIR" ]; then
  PY_DIR="/tmp/parity-python-$(date +%s)"
  header "Running Python scan"
  info "Output: $PY_DIR"

  if ! command -v "$PY_CMD" &>/dev/null && [ ! -f "$PY_CMD" ]; then
    fail "Python inspectah not found: $PY_CMD"
    echo "Install with: pip install inspectah  (or use --python-dir to skip)"
    exit 1
  fi

  if $PY_CMD scan --inspect-only --output-dir "$PY_DIR" 2>&1; then
    pass "Python scan completed"
  else
    fail "Python scan failed"
    exit 1
  fi
else
  info "Using existing Python output: $PY_DIR"
fi

# ── Step 2: Go scan ────────────────────────────────────────────────────────
if [ -z "$GO_DIR" ]; then
  GO_DIR="/tmp/parity-go-$(date +%s)"
  header "Running Go scan"
  info "Output: $GO_DIR"

  if $GO_CMD scan --inspect-only --output-dir "$GO_DIR" 2>&1; then
    pass "Go scan completed"
  else
    fail "Go scan failed"
    exit 1
  fi
else
  info "Using existing Go output: $GO_DIR"
fi

# ── Step 3: Scan parity (sections 1.3 + 1.5) ──────────────────────────────
header "Running scan parity check (Tier 1.3 + 1.5)"
echo ""
if "$SCRIPT_DIR/verify-scan-parity.sh" "$PY_DIR" "$GO_DIR"; then
  record_result "Scan Parity" "PASS"
else
  record_result "Scan Parity" "FAIL"
fi

# ── Step 4: Redaction parity (section 1.4) ─────────────────────────────────
header "Running redaction parity check (Tier 1.4)"
echo ""
if "$SCRIPT_DIR/verify-redaction-parity.sh" "$PY_DIR" "$GO_DIR"; then
  record_result "Redaction Parity" "PASS"
else
  record_result "Redaction Parity" "FAIL"
fi

# ── Step 5: Fleet parity (section 1.6) ─────────────────────────────────────
if [ "$SCAN_ONLY" = false ]; then
  if [ -n "$FLEET_DIR" ] && [ -d "$FLEET_DIR" ]; then
    header "Running fleet parity check (Tier 1.6)"
    echo ""
    if "$SCRIPT_DIR/verify-fleet-parity.sh" "$FLEET_DIR" "$PY_CMD" "$GO_CMD"; then
      record_result "Fleet Parity" "PASS"
    else
      record_result "Fleet Parity" "FAIL"
    fi
  else
    header "Fleet parity (Tier 1.6)"
    warn "No --fleet-dir provided, skipping fleet comparison"
    info "To run: collect 2+ scan tarballs in a directory, then:"
    info "  $0 --fleet-dir /path/to/tarballs --python-dir $PY_DIR --go-dir $GO_DIR"
    record_result "Fleet Parity" "SKIP"
  fi
else
  record_result "Fleet Parity" "SKIP"
fi

# ── Final Summary ──────────────────────────────────────────────────────────
echo ""
echo ""
echo -e "${BOLD}================================================================${RESET}"
echo -e "${BOLD}  Tier 1 Verification Summary${RESET}"
echo -e "${BOLD}================================================================${RESET}"
echo ""

ALL_PASS=true
for result in "${RESULTS[@]}"; do
  name="${result%%:*}"
  status="${result##*:}"
  case "$status" in
    PASS) pass "$name" ;;
    FAIL) fail "$name"; ALL_PASS=false ;;
    SKIP) warn "$name (skipped)" ;;
  esac
done

echo ""
echo -e "  Python output: $PY_DIR"
echo -e "  Go output:     $GO_DIR"
echo ""

if [ "$ALL_PASS" = true ]; then
  echo -e "${GREEN}${BOLD}  Tier 1 PASSED -- safe to proceed to Tier 2 functional verification.${RESET}"
  echo ""
  exit 0
else
  echo -e "${RED}${BOLD}  Tier 1 FAILED -- resolve data parity issues before proceeding.${RESET}"
  echo ""
  exit 1
fi
