---
name: baseline-image-local-first
description: podman pull contacts the registry even when the image is stored, so baseline extraction checks local storage with `podman image exists` first; mocks without that key take the pull path.
---

# Baseline Extraction Uses the Local Image First

`crates/collect/src/baseline.rs::extract_baseline` runs
`podman image exists <ref>` before anything else touches the image, and
skips `podman pull` when it exits 0.

## Why the pull cannot simply run every time

`podman pull` defaults to `--policy always`: it contacts the registry even
when the image is already in local storage. On a disconnected host that has
the base image from `podman load`, an unconditional pull fails, every caller
in `scan.rs` turns that into `exit(3)`, and the documented air-gap workflow
cannot complete.

## Why not `podman pull --policy missing`

It is one command instead of two, but the release that added `--policy` to
`podman pull` was not confirmed against the documented floor (podman 4.4,
`README.md`). `podman image exists` has been in podman since 1.x, so the
two-command form carries no version risk. Revisit only with the floor
verified on a real EL8 host.

## What it costs

A stored image is used as-is. A connected host that pulled the base image
weeks ago scans against that copy, not the registry's current build. The
callback prints `Using local image <ref>; not pulling...` so the operator
can tell, and the docs tell them to `podman rmi` or re-pull to refresh.

## Test consequence

`MockExecutor` returns exit 127 for an unregistered command (see
[mock-executor-key-format](mock-executor-key-format.md)). Any non-zero exit
from `image exists` is treated as absent, so a mock with no
`podman image exists` entry still exercises the pull path. That keeps old
fixtures valid, and it also means a test meant to cover the local-image path
must register the key explicitly, or it silently tests the pull instead.
Command-log index assertions shift by one: `image exists` sits between
`which podman` and `podman pull`.
