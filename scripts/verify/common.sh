#!/usr/bin/env bash
# common.sh -- shared helpers for Tier 1 verification scripts
# Source this file; do not run directly.
#
# Provides:
#   Colors and formatting
#   normalize_snapshot()  -- strip volatile fields, sort keys
#   compare_section()     -- diff a single JSON section
#   SECTIONS array        -- the canonical inspector section list

set -euo pipefail

# ── Colors ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

pass()  { echo -e "${GREEN}${BOLD}  PASS${RESET}  $*"; }
fail()  { echo -e "${RED}${BOLD}  FAIL${RESET}  $*"; }
warn()  { echo -e "${YELLOW}${BOLD}  WARN${RESET}  $*"; }
info()  { echo -e "${CYAN}  INFO${RESET}  $*"; }
header(){ echo -e "\n${BOLD}── $* ──${RESET}"; }

# ── Inspector sections (canonical list from verification plan) ──────────────
SECTIONS=(
  rpm
  config
  services
  network
  storage
  scheduled_tasks
  containers
  non_rpm_software
  kernel_boot
  selinux
  users_groups
  os_release
  system_type
  preflight
  warnings
)

# ── Dependency check ────────────────────────────────────────────────────────
require_jq() {
  if ! command -v jq &>/dev/null; then
    echo "ERROR: jq is required but not found. Install with: sudo dnf install -y jq" >&2
    exit 1
  fi
}

# ── normalize_snapshot ──────────────────────────────────────────────────────
# Usage: normalize_snapshot <input.json> <output.json>
#
# Strips volatile meta fields (timestamp, duration_seconds, tool_version),
# sorts all keys recursively. The result is suitable for deterministic diff.
normalize_snapshot() {
  local input="$1" output="$2"
  jq --sort-keys '
    del(.meta.timestamp, .meta.duration_seconds, .meta.tool_version)
  ' "$input" > "$output"
}

# ── sort_arrays_in_section ──────────────────────────────────────────────────
# Some list fields may have items in different order (processing order
# differences between Python and Go). This helper sorts arrays of objects
# by a deterministic key when possible.
#
# Usage: sort_arrays_in_section <file.json> <section> > sorted.json
sort_arrays_in_section() {
  local file="$1" section="$2"
  jq --arg sec "$section" '
    # Recursively sort arrays of objects by a stable key.
    def sort_arrays:
      if type == "array" then
        if length > 0 and (.[0] | type) == "object" then
          sort_by(
            if .name then .name
            elif .path then .path
            elif .uuid then .uuid
            elif .unit then .unit
            elif .key then .key
            else tojson
            end
          ) | map(sort_arrays)
        else
          sort | map(sort_arrays)
        end
      elif type == "object" then
        to_entries | map(.value = (.value | sort_arrays)) | from_entries
      else .
      end;
    .[$sec] | sort_arrays
  ' "$file"
}

# ── compare_section ─────────────────────────────────────────────────────────
# Usage: compare_section <py_normalized.json> <go_normalized.json> <section>
# Returns 0 on match, 1 on difference.
# Prints PASS/FAIL with context.
compare_section() {
  local py_file="$1" go_file="$2" section="$3"
  local tmp_py tmp_go

  tmp_py=$(mktemp)
  tmp_go=$(mktemp)

  # Extract and sort the section
  sort_arrays_in_section "$py_file" "$section" > "$tmp_py" 2>/dev/null || echo "null" > "$tmp_py"
  sort_arrays_in_section "$go_file" "$section" > "$tmp_go" 2>/dev/null || echo "null" > "$tmp_go"

  # Handle null-vs-empty-list equivalence (known divergence).
  # Recursively replace [] with null so the diff ignores this.
  local py_norm go_norm
  py_norm=$(jq '
    def empty_to_null:
      if type == "array" and length == 0 then null
      elif type == "array" then map(empty_to_null)
      elif type == "object" then to_entries | map(.value = (.value | empty_to_null)) | from_entries
      else .
      end;
    empty_to_null
  ' "$tmp_py" 2>/dev/null || cat "$tmp_py")

  go_norm=$(jq '
    def empty_to_null:
      if type == "array" and length == 0 then null
      elif type == "array" then map(empty_to_null)
      elif type == "object" then to_entries | map(.value = (.value | empty_to_null)) | from_entries
      else .
      end;
    empty_to_null
  ' "$tmp_go" 2>/dev/null || cat "$tmp_go")

  echo "$py_norm" > "$tmp_py"
  echo "$go_norm" > "$tmp_go"

  if diff -q "$tmp_py" "$tmp_go" &>/dev/null; then
    pass "$section"
    rm -f "$tmp_py" "$tmp_go"
    return 0
  fi

  # Show diff for failures
  fail "$section"
  diff --unified=3 --label="python/$section" --label="go/$section" \
    "$tmp_py" "$tmp_go" | head -50
  echo ""
  rm -f "$tmp_py" "$tmp_go"
  return 1
}
