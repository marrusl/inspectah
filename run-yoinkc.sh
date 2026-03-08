#!/bin/sh
set -eu

HOST_ROOT="/"
IMAGE="${YOINKC_IMAGE:-ghcr.io/marrusl/yoinkc:latest}"
OUTPUT_DIR="${1:-${YOINKC_OUTPUT:-./yoinkc-output}}"
shift 2>/dev/null || true

# Strip --no-entitlement from pass-through args before forwarding to yoinkc.
_skip_entitlement=false
_args=""
for _arg in "$@"; do
    case "$_arg" in
        --no-entitlement) _skip_entitlement=true ;;
        *) _args="${_args} ${_arg}" ;;
    esac
done
set -- ${_args}

_need_install=""
for cmd in podman tar; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        _need_install="${_need_install} ${cmd}"
    fi
done
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

# Track whether podman itself was just installed so the registry login check
# below can offer interactive login instead of immediately erroring out.
_podman_just_installed=false
case " ${_need_install} " in
  *" podman "*) _podman_just_installed=true ;;
esac

# Expose the list of just-installed tools to the yoinkc container so it can
# exclude them from its RPM output (they're tool prerequisites, not operator
# additions to the migration target).
if [ -n "$_need_install" ]; then
  YOINKC_EXCLUDE_PREREQS="${_need_install# }"
  export YOINKC_EXCLUDE_PREREQS
fi

mkdir -p "$OUTPUT_DIR"
OUTPUT_DIR="$(cd "$OUTPUT_DIR" && pwd)"

echo "Image:  $IMAGE"
echo "Output: $OUTPUT_DIR"

# On RHEL hosts, yoinkc needs to pull a base image from registry.redhat.io at
# build time (or at inspection time if using an RH-based YOINKC_IMAGE).
# Check for a stored login credential before hitting the registry.
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

# Called when podman was JUST installed by this script and has no stored
# credentials. If stdin is a TTY we can prompt interactively; otherwise
# we give actionable instructions and exit so the user can log in first.
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

case "$IMAGE" in
  registry.redhat.io/*)
    # Image itself is from the Red Hat registry
    if $_podman_just_installed && ! podman login --get-login registry.redhat.io >/dev/null 2>&1; then
      _prompt_rh_login_fresh
    else
      _check_rh_login
    fi
    ;;
  *)
    # For any image on a RHEL host, yoinkc will pull a rhel-bootc base image
    # from registry.redhat.io to compute the package baseline.
    if [ -f /etc/redhat-release ] && grep -qi "red hat" /etc/redhat-release 2>/dev/null; then
      if $_podman_just_installed && ! podman login --get-login registry.redhat.io >/dev/null 2>&1; then
        _prompt_rh_login_fresh
      else
        _check_rh_login
      fi
    fi
    ;;
esac

echo "=== Pulling yoinkc image ==="
podman pull "$IMAGE"

echo "=== Running yoinkc ==="
podman run --rm \
  --pid=host \
  --privileged \
  --security-opt label=disable \
  ${YOINKC_DEBUG:+-e YOINKC_DEBUG=1} \
  ${YOINKC_EXCLUDE_PREREQS:+--env YOINKC_EXCLUDE_PREREQS} \
  -v "${HOST_ROOT}:/host:ro" \
  -v "${OUTPUT_DIR}:/output:z" \
  "$IMAGE" --output-dir /output "$@"

# Bundle RHEL subscription certs so the tarball can be used for podman build
# on non-RHEL workstations.  --no-entitlement suppresses this (e.g. when
# sharing the tarball publicly).
if ! $_skip_entitlement; then
    if [ -d /etc/pki/entitlement ] && ls /etc/pki/entitlement/*.pem >/dev/null 2>&1; then
        mkdir -p "$OUTPUT_DIR/entitlement"
        cp /etc/pki/entitlement/*.pem "$OUTPUT_DIR/entitlement/"
        echo "Bundled $(ls "$OUTPUT_DIR/entitlement/"*.pem | wc -l | tr -d ' ') entitlement cert(s) for non-RHEL builds"
    fi
    if [ -d /etc/rhsm ]; then
        cp -a /etc/rhsm "$OUTPUT_DIR/rhsm"
        echo "Bundled /etc/rhsm for non-RHEL builds"
    fi
fi

echo "=== Packaging results ==="
HOSTNAME="$(hostname -s 2>/dev/null || cat /etc/hostname 2>/dev/null || echo unknown)"
STAMP="${HOSTNAME}-$(date +%Y%m%d-%H%M%S)"
TARBALL="${OUTPUT_DIR}/../${STAMP}.tar.gz"
tar -czf "$TARBALL" -C "$OUTPUT_DIR" --transform "s,^\.,$STAMP," .
echo "=== Done. Output in ${OUTPUT_DIR}, tarball at ${TARBALL} ==="
