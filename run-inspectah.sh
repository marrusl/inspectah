#!/bin/sh
set -eu

IMAGE="${INSPECTAH_IMAGE:-ghcr.io/marrusl/inspectah:latest}"
OUTPUT_DIR="${INSPECTAH_OUTPUT_DIR:-$(pwd)}"

# --- Browser launch helper ---
_open_browser() {
  if command -v open >/dev/null 2>&1; then
    open "$1"
  elif command -v xdg-open >/dev/null 2>&1; then
    xdg-open "$1" >/dev/null 2>&1
  else
    echo "  Open $1 in your browser"
  fi
}

# --- Detect subcommand ---
_mode="scan"
case "${1:-}" in
  fleet|refine|architect) _mode="$1"; shift ;;
esac

# --- Ensure podman is available ---
_need_install=""
if ! command -v podman >/dev/null 2>&1; then
    _need_install="podman"
fi
if [ -n "$_need_install" ]; then
    echo "Installing missing tools:${_need_install}" >&2
    if command -v dnf >/dev/null 2>&1; then
        dnf install -y $_need_install
    elif command -v yum >/dev/null 2>&1; then
        yum install -y $_need_install
    else
        echo "ERROR: missing${_need_install} and no supported package manager found." >&2
        exit 1
    fi
fi

_podman_just_installed=false
case " ${_need_install} " in
  *" podman "*) _podman_just_installed=true ;;
esac

# Only relevant for inspect mode — tool prerequisites shouldn't appear
# in the RPM output.
if [ -n "$_need_install" ] && [ "$_mode" = "scan" ]; then
  INSPECTAH_EXCLUDE_PREREQS="${_need_install# }"
  export INSPECTAH_EXCLUDE_PREREQS
fi

echo "Image: $IMAGE"

# --- Registry login checks for registry.redhat.io ---
# Only needed for inspect mode (pulls base images). Fleet and refine
# consume existing tarballs and don't need registry access.
_check_rh_login() {
  if ! podman login --get-login registry.redhat.io >/dev/null 2>&1; then
    echo "" >&2
    echo "ERROR: You are not logged in to registry.redhat.io." >&2
    echo "" >&2
    echo "  Run:  sudo podman login registry.redhat.io" >&2
    echo "" >&2
    echo "  Use your Red Hat account (https://access.redhat.com)." >&2
    echo "  Free developer account: https://developers.redhat.com" >&2
    echo "" >&2
    exit 1
  fi
}

_prompt_rh_login_fresh() {
  if [ -t 0 ]; then
    printf '\ninspectah needs access to registry.redhat.io for the RHEL base image.\nLet'\''s log in now:\n\n' >&2
    if podman login registry.redhat.io; then
      return 0
    fi
    echo "" >&2
    echo "ERROR: podman login failed." >&2
    echo "  Use your Red Hat account (https://access.redhat.com)." >&2
    exit 1
  else
    echo "" >&2
    echo "ERROR: podman was just installed and has no credentials for registry.redhat.io." >&2
    echo "" >&2
    echo "  Run:  sudo podman login registry.redhat.io" >&2
    echo "  Then re-run this script." >&2
    echo "" >&2
    echo "  Use your Red Hat account (https://access.redhat.com)." >&2
    echo "  Free developer account: https://developers.redhat.com" >&2
    echo "" >&2
    exit 1
  fi
}

# --- Run the appropriate mode ---
case "$_mode" in
  scan)
    case "$IMAGE" in
      registry.redhat.io/*)
        if $_podman_just_installed && ! podman login --get-login registry.redhat.io >/dev/null 2>&1; then
          _prompt_rh_login_fresh
        else
          _check_rh_login
        fi
        ;;
      *)
        if [ -f /etc/redhat-release ] && grep -qi "red hat" /etc/redhat-release 2>/dev/null; then
          if $_podman_just_installed && ! podman login --get-login registry.redhat.io >/dev/null 2>&1; then
            _prompt_rh_login_fresh
          else
            _check_rh_login
          fi
        fi
        ;;
    esac

    echo "=== Running inspectah ==="
    podman run --rm --pull=always \
      --pid=host \
      --privileged \
      --security-opt label=disable \
      -w /output \
      ${INSPECTAH_DEBUG:+-e INSPECTAH_DEBUG=1} \
      ${INSPECTAH_EXCLUDE_PREREQS:+--env INSPECTAH_EXCLUDE_PREREQS} \
      -e INSPECTAH_HOST_CWD="$(pwd)" \
      -e INSPECTAH_HOSTNAME="${INSPECTAH_HOSTNAME:-$(hostnamectl hostname 2>/dev/null || hostname -f)}" \
      -v /:/host:ro \
      -v "$(pwd):/output" \
      "$IMAGE" "$@"
    echo "=== Done ==="
    ;;

  fleet)
    if [ $# -eq 0 ]; then
      echo "Usage: $(basename "$0") fleet <input-dir> [flags...]" >&2
      echo "" >&2
      echo "Runs inspectah fleet inside the inspectah container." >&2
      echo "" >&2
      echo "  <input-dir>  Directory containing inspectah tarballs or JSON snapshots" >&2
      exit 1
    fi

    INPUT_DIR="$(cd "$1" && pwd)"
    DIR_NAME="$(basename "$INPUT_DIR")"
    shift

    TEMP_OUT="$(mktemp -d)"
    trap 'rm -rf "$TEMP_OUT"' EXIT

    echo "=== Running inspectah fleet ==="
    podman run --rm --pull=always \
      --security-opt label=disable \
      -w /output \
      -v "$INPUT_DIR":/input:ro \
      -v "$TEMP_OUT":/output \
      "$IMAGE" fleet /input -o "/output/${DIR_NAME}.tar.gz" "$@"

    cp "$TEMP_OUT/${DIR_NAME}.tar.gz" "$OUTPUT_DIR/"
    echo ""
    echo "Output: $OUTPUT_DIR/${DIR_NAME}.tar.gz"
    ;;

  refine)
    if [ $# -eq 0 ]; then
      echo "Usage: $(basename "$0") refine <tarball>" >&2
      exit 1
    fi

    # Check for --no-browser before consuming the tarball arg
    _launch_browser=true
    TARBALL="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
    TARBALL_NAME="$(basename "$TARBALL")"
    shift
    for _arg in "$@"; do
      case "$_arg" in --no-browser) _launch_browser=false ;; esac
    done

    echo "=== Running inspectah refine ==="
    echo "  Report will be at: http://localhost:8642"

    # Poll for server readiness and open browser on the host
    if $_launch_browser; then
      ( _tries=0
        while ! curl -sf http://localhost:8642/api/health >/dev/null 2>&1; do
          _tries=$((_tries + 1))
          if [ "$_tries" -ge 60 ]; then break; fi
          sleep 0.5
        done
        if [ "$_tries" -lt 60 ]; then _open_browser "http://localhost:8642"; fi
      ) &
      _browser_pid=$!
      trap 'kill "$_browser_pid" 2>/dev/null || true' EXIT
    fi

    podman run --rm --pull=always \
      --security-opt label=disable \
      -p 8642:8642 \
      -v "$TARBALL":/input/"$TARBALL_NAME":ro \
      "$IMAGE" refine --port 8642 --bind 0.0.0.0 --no-browser "/input/$TARBALL_NAME" "$@"
    ;;

  architect)
    if [ $# -eq 0 ]; then
      echo "Usage: $(basename "$0") architect <tarball-or-dir> [--no-browser] [extra args...]" >&2
      exit 1
    fi

    # Check for --no-browser before consuming the input arg
    _launch_browser=true
    INPUT_PATH="$1"
    shift
    for _arg in "$@"; do
      case "$_arg" in --no-browser) _launch_browser=false ;; esac
    done

    # Handle both tarball and directory input
    if [ -f "$INPUT_PATH" ]; then
      # Tarball input
      INPUT_FULL="$(cd "$(dirname "$INPUT_PATH")" && pwd)/$(basename "$INPUT_PATH")"
      INPUT_NAME="$(basename "$INPUT_FULL")"
      MOUNT_SPEC="$INPUT_FULL:/input/$INPUT_NAME:ro"
      CMD_ARG="/input/$INPUT_NAME"
    elif [ -d "$INPUT_PATH" ]; then
      # Directory input
      INPUT_FULL="$(cd "$INPUT_PATH" && pwd)"
      MOUNT_SPEC="$INPUT_FULL:/input:ro"
      CMD_ARG="/input"
    else
      echo "ERROR: $INPUT_PATH is neither a file nor a directory" >&2
      exit 1
    fi

    echo "=== Running inspectah architect ==="
    echo "  Dashboard will be at: http://localhost:8643"

    # Poll for server readiness and open browser on the host
    if $_launch_browser; then
      ( _tries=0
        while ! curl -sf http://localhost:8643/api/health >/dev/null 2>&1; do
          _tries=$((_tries + 1))
          if [ "$_tries" -ge 60 ]; then break; fi
          sleep 0.5
        done
        if [ "$_tries" -lt 60 ]; then _open_browser "http://localhost:8643"; fi
      ) &
      _browser_pid=$!
      trap 'kill "$_browser_pid" 2>/dev/null || true' EXIT
    fi

    podman run --rm --pull=always \
      --security-opt label=disable \
      -p 8643:8643 \
      -v "$MOUNT_SPEC" \
      "$IMAGE" architect --port 8643 --bind 0.0.0.0 --no-browser "$CMD_ARG" "$@"
    ;;
esac
