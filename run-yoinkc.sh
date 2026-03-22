#!/bin/sh
set -eu

IMAGE="${YOINKC_IMAGE:-ghcr.io/marrusl/yoinkc:latest}"
OUTPUT_DIR="${YOINKC_OUTPUT_DIR:-$(pwd)}"

# --- Detect subcommand ---
_mode="inspect"
case "${1:-}" in
  fleet|refine) _mode="$1"; shift ;;
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
if [ -n "$_need_install" ] && [ "$_mode" = "inspect" ]; then
  YOINKC_EXCLUDE_PREREQS="${_need_install# }"
  export YOINKC_EXCLUDE_PREREQS
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
    printf '\nyoinkc needs access to registry.redhat.io for the RHEL base image.\nLet'\''s log in now:\n\n' >&2
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
  inspect)
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

    echo "=== Running yoinkc ==="
    podman run --rm --pull=always \
      --pid=host \
      --privileged \
      --security-opt label=disable \
      -w /output \
      ${YOINKC_DEBUG:+-e YOINKC_DEBUG=1} \
      ${YOINKC_EXCLUDE_PREREQS:+--env YOINKC_EXCLUDE_PREREQS} \
      -e YOINKC_HOST_CWD="$(pwd)" \
      -e YOINKC_HOSTNAME="${YOINKC_HOSTNAME:-$(hostname -s)}" \
      -v /:/host:ro \
      -v "$(pwd):/output" \
      "$IMAGE" "$@"
    echo "=== Done ==="
    ;;

  fleet)
    if [ $# -eq 0 ]; then
      echo "Usage: $(basename "$0") fleet <input-dir> [flags...]" >&2
      echo "" >&2
      echo "Runs yoinkc fleet inside the yoinkc container." >&2
      echo "" >&2
      echo "  <input-dir>  Directory containing yoinkc tarballs or JSON snapshots" >&2
      exit 1
    fi

    INPUT_DIR="$(cd "$1" && pwd)"
    DIR_NAME="$(basename "$INPUT_DIR")"
    shift

    TEMP_OUT="$(mktemp -d)"
    trap 'rm -rf "$TEMP_OUT"' EXIT

    echo "=== Running yoinkc fleet ==="
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

    TARBALL="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
    TARBALL_NAME="$(basename "$TARBALL")"
    shift

    echo "=== Running yoinkc refine ==="
    echo "  Report will be at: http://localhost:8642"
    podman run --rm --pull=always \
      --security-opt label=disable \
      -p 8642:8642 \
      -v "$TARBALL":/input/"$TARBALL_NAME":ro \
      "$IMAGE" refine --port 8642 --bind 0.0.0.0 --no-browser "/input/$TARBALL_NAME" "$@"
    ;;
esac
