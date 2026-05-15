#!/usr/bin/env bash
set -euo pipefail

# Host Validation Script for inspectah Rust Phase 2 (Slice 2c — all 11 sections)
#
# Runs both Go and Rust inspectah on a live package-mode system and
# compares section-level output to establish parity evidence.
# Covers all 11 inspector sections: RPM, services, storage, kernelboot,
# network, containers, users_groups, scheduled_tasks, config, selinux,
# non_rpm_software.
#
# Produces a tarball in the working directory with all evidence.
#
# If running from the inspectah source tree, the script will build from
# source using system Rust (dnf install rust cargo) when no pre-built
# binary is provided.
#
# Prerequisites:
#   - Go inspectah installed and in PATH (inspectah command)
#   - Rust inspectah binary at ./inspectah-rust (or pass path as $1)
#   - OR: Rust toolchain installed (dnf install rust cargo) to build from source
#   - jq installed
#   - Run as root (inspectah needs access to system state)
#
# Usage:
#   chmod +x host-validation.sh
#   sudo ./host-validation.sh [rust-binary-path] [go-binary-name]
#
# Examples:
#   sudo ./host-validation.sh                        # defaults: ./inspectah-rust, inspectah
#   sudo ./host-validation.sh /tmp/inspectah-rust    # custom Rust binary path
#   sudo ./host-validation.sh ./inspectah-rust inspectah-go  # custom Go binary name

RUST_BIN="${1:-./inspectah-rust}"
GO_BIN="${2:-inspectah}"
WORKDIR="/tmp/inspectah-host-validation-$(date +%Y%m%d-%H%M%S)"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# If no pre-built binary provided, build from source
if [ ! -f "$RUST_BIN" ]; then
    echo ">>> No pre-built binary found at $RUST_BIN, building from source..."
    if ! command -v cargo >/dev/null 2>&1; then
        echo "Installing Rust toolchain via dnf..."
        sudo dnf install -y rust cargo gcc 2>/dev/null || sudo yum install -y rust cargo gcc
    fi
    echo "Building inspectah-cli (release)..."
    export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
    cargo build --release -p inspectah-cli --manifest-path "$REPO_ROOT/Cargo.toml"
    RUST_BIN="$REPO_ROOT/target/release/inspectah"
fi

echo "=== inspectah Host Validation ==="
echo "Rust binary: $RUST_BIN"
echo "Go binary:   $GO_BIN"
echo "Working dir: $WORKDIR"
echo ""

# --- Pre-flight checks ---

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: Must run as root (sudo). inspectah needs access to system state."
    exit 1
fi

command -v "$GO_BIN" >/dev/null 2>&1 || {
    echo "ERROR: Go inspectah not found: $GO_BIN"
    echo "  Install it or pass the correct binary name as the second argument."
    exit 1
}

[ -x "$RUST_BIN" ] || {
    echo "ERROR: Rust binary not found or not executable: $RUST_BIN"
    echo "  Make sure you scp'd the binary and ran: chmod +x $RUST_BIN"
    exit 1
}

command -v jq >/dev/null 2>&1 || {
    echo "ERROR: jq is required but not installed."
    echo "  Install it: sudo dnf install -y jq"
    exit 1
}

mkdir -p "$WORKDIR"/{go-scan,rust-scan,golden,evidence}

# --- Step 1: Run Go scan ---

echo ">>> Step 1: Running Go inspectah scan..."
"$GO_BIN" scan --output "$WORKDIR/go-scan" 2>&1 | tee "$WORKDIR/evidence/go-scan.log"
echo "Go scan complete."
echo ""

# --- Step 2: Run Rust scan ---

echo ">>> Step 2: Running Rust inspectah scan..."
"$RUST_BIN" scan --inspect-only --output "$WORKDIR/rust-scan/inspection-snapshot.json" 2>&1 | tee "$WORKDIR/evidence/rust-scan.log"
echo "Rust scan complete."
echo ""

# --- Step 3: Extract Go section golden files ---

echo ">>> Step 3: Extracting Go golden files..."
GO_SNAPSHOT="$WORKDIR/go-scan/inspection-snapshot.json"
if [ ! -f "$GO_SNAPSHOT" ]; then
    echo "ERROR: Go snapshot not found at $GO_SNAPSHOT"
    echo "  Check go-scan.log for errors."
    exit 1
fi

jq '.services' "$GO_SNAPSHOT" > "$WORKDIR/golden/go-v13-services-section.json"
jq '.storage' "$GO_SNAPSHOT" > "$WORKDIR/golden/go-v13-storage-section.json"
jq '.kernel_boot' "$GO_SNAPSHOT" > "$WORKDIR/golden/go-v13-kernelboot-section.json"
jq '.network' "$GO_SNAPSHOT" > "$WORKDIR/golden/go-v13-network-section.json"
jq '.containers' "$GO_SNAPSHOT" > "$WORKDIR/golden/go-v13-containers-section.json"
jq '.users_groups' "$GO_SNAPSHOT" > "$WORKDIR/golden/go-v13-users-groups-section.json"
jq '.scheduled_tasks' "$GO_SNAPSHOT" > "$WORKDIR/golden/go-v13-scheduled-tasks-section.json"
jq '.config' "$GO_SNAPSHOT" > "$WORKDIR/golden/go-v13-config-section.json"
jq '.selinux' "$GO_SNAPSHOT" > "$WORKDIR/golden/go-v13-selinux-section.json"
jq '.non_rpm_software' "$GO_SNAPSHOT" > "$WORKDIR/golden/go-v13-non-rpm-software-section.json"
echo "Golden files extracted (all 11 sections)."
echo ""

# --- Step 4: Extract Rust sections ---

echo ">>> Step 4: Extracting Rust sections for comparison..."
RUST_SNAPSHOT="$WORKDIR/rust-scan/inspection-snapshot.json"
if [ ! -f "$RUST_SNAPSHOT" ]; then
    echo "ERROR: Rust snapshot not found at $RUST_SNAPSHOT"
    echo "  Check rust-scan.log for errors."
    exit 1
fi

jq '.services' "$RUST_SNAPSHOT" > "$WORKDIR/evidence/rust-services.json"
jq '.storage' "$RUST_SNAPSHOT" > "$WORKDIR/evidence/rust-storage.json"
jq '.kernel_boot' "$RUST_SNAPSHOT" > "$WORKDIR/evidence/rust-kernelboot.json"
jq '.network' "$RUST_SNAPSHOT" > "$WORKDIR/evidence/rust-network.json"
jq '.containers' "$RUST_SNAPSHOT" > "$WORKDIR/evidence/rust-containers.json"
jq '.users_groups' "$RUST_SNAPSHOT" > "$WORKDIR/evidence/rust-users-groups.json"
jq '.scheduled_tasks' "$RUST_SNAPSHOT" > "$WORKDIR/evidence/rust-scheduled-tasks.json"
jq '.config' "$RUST_SNAPSHOT" > "$WORKDIR/evidence/rust-config.json"
jq '.selinux' "$RUST_SNAPSHOT" > "$WORKDIR/evidence/rust-selinux.json"
jq '.non_rpm_software' "$RUST_SNAPSHOT" > "$WORKDIR/evidence/rust-non-rpm-software.json"
echo "Rust sections extracted (all 11)."
echo ""

# --- Step 5: Section-level diff ---

echo "=== Section Parity Comparison ==="
PASS_COUNT=0
FAIL_COUNT=0

for section in services storage kernelboot network containers users-groups scheduled-tasks config selinux non-rpm-software; do
    go_file="$WORKDIR/golden/go-v13-${section}-section.json"
    rust_file="$WORKDIR/evidence/rust-${section}.json"
    diff_file="$WORKDIR/evidence/diff-${section}.txt"

    echo ""
    echo "--- $section ---"

    if diff <(jq -S . "$go_file") <(jq -S . "$rust_file") > "$diff_file" 2>&1; then
        echo "  MATCH: sections are identical"
        PASS_COUNT=$((PASS_COUNT + 1))
    else
        DIFF_LINES=$(wc -l < "$diff_file" | tr -d ' ')
        echo "  DIVERGENCE: sections differ ($DIFF_LINES lines of diff)"
        echo "  See: $diff_file"
        FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
done

echo ""
echo "Results: $PASS_COUNT matched, $FAIL_COUNT diverged"
echo ""

# --- Step 6: Collect host info for evidence ---

echo "=== Collecting host evidence ==="

OS_PRETTY=$(grep PRETTY_NAME /etc/os-release 2>/dev/null | cut -d= -f2 | tr -d '"' || echo "unknown")
GO_VERSION=$("$GO_BIN" version 2>/dev/null || echo "unknown")
RUST_VERSION=$("$RUST_BIN" version 2>/dev/null || echo "0.8.0-alpha.1")

{
    echo "# Host Validation Evidence - Slice 2c (all 11 sections)"
    echo ""
    echo "**Date:** $(date -Iseconds)"
    echo "**Hostname:** $(hostname)"
    echo ""
    echo "## Host Details"
    echo ""
    echo "- **OS:** $OS_PRETTY"
    echo "- **Kernel:** $(uname -r)"
    echo "- **Architecture:** $(uname -m)"
    echo "- **Go inspectah version:** $GO_VERSION"
    echo "- **Rust inspectah version:** $RUST_VERSION"
    echo ""
    echo "## Scan Results"
    echo ""
    echo "### Go scan output"
    echo '```'
    ls -la "$WORKDIR/go-scan/"
    echo '```'
    echo ""
    echo "### Rust scan output"
    echo '```'
    ls -la "$WORKDIR/rust-scan/"
    echo '```'
    echo ""
    echo "## Section Parity"
    echo ""
    for section in services storage kernelboot network containers users-groups scheduled-tasks config selinux non-rpm-software; do
        diff_file="$WORKDIR/evidence/diff-${section}.txt"
        if [ ! -s "$diff_file" ]; then
            echo "- **$section:** MATCH"
        else
            echo "- **$section:** DIVERGENCE ($(wc -l < "$diff_file" | tr -d ' ') lines)"
        fi
    done
    echo ""
    echo "## Conclusion"
    echo ""
    if [ "$FAIL_COUNT" -eq 0 ]; then
        echo "All sections match. Parity validated."
    else
        echo "[Review diffs above and fill in assessment]"
    fi
} > "$WORKDIR/evidence/host-validation.md"

echo ""

# --- Step 7: Create tarball ---

TARBALL_NAME="host-validation-$(hostname)-$(date +%Y%m%d-%H%M%S).tar.gz"
TARBALL_PATH="$REPO_ROOT/$TARBALL_NAME"

echo ">>> Step 7: Creating evidence tarball..."
tar czf "$TARBALL_PATH" -C "$WORKDIR" golden evidence
echo "Tarball: $TARBALL_PATH"
echo ""

# --- Step 8: Copy to repo tree ---

echo ">>> Step 8: Copying golden files and evidence to repo..."
mkdir -p "$REPO_ROOT/testdata/evidence"
cp "$WORKDIR/golden/"* "$REPO_ROOT/testdata/golden/"
cp "$WORKDIR/evidence/host-validation.md" "$REPO_ROOT/testdata/evidence/"
echo ""

echo "=== Done ==="
echo ""
echo "Tarball (all evidence + goldens):"
echo "  $TARBALL_PATH"
echo ""
echo "Repo files updated:"
echo "  testdata/golden/go-v13-*-section.json  (11 sections)"
echo "  testdata/evidence/host-validation.md"
echo ""
echo "Next steps:"
echo "  1. Review the evidence: tar tzf $TARBALL_NAME"
echo "  2. Commit the updated golden files and evidence"
echo ""
