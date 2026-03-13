#!/bin/sh
set -eu

IMAGE="${YOINKC_IMAGE:-ghcr.io/marrusl/yoinkc:latest}"

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

# Track whether podman was just installed for the registry login check.
_podman_just_installed=false
case " ${_need_install} " in
  *" podman "*) _podman_just_installed=true ;;
esac

# Expose just-installed tools to yoinkc so it can exclude them from
# the RPM output (they're tool prerequisites, not operator additions).
if [ -n "$_need_install" ]; then
  YOINKC_EXCLUDE_PREREQS="${_need_install# }"
  export YOINKC_EXCLUDE_PREREQS
fi

echo "Image: $IMAGE"

# Registry login checks for registry.redhat.io
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
  -e YOINKC_HOSTNAME="$(hostname -s)" \
  -v /:/host:ro \
  -v "$(pwd):/output" \
  "$IMAGE" "$@"
echo "=== Done ==="
