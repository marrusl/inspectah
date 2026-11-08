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
`README.md`). `podman image exists` predates that floor, so the two-command
form carries no version risk. Revisit only with the floor
verified on a real EL8 host.

## What it costs

A stored image is used as-is. A connected host that pulled the base image
weeks ago scans against that copy, not the registry's current build. So the
operator can tell, `extract_baseline` returns an `ImageSource` (`Local` or
`Pulled`) next to the `BaselineData`, and `scan.rs` prints
`Using local image <ref> (<digest>)` in place of `Pulled <ref> (...)`,
followed by a line naming `podman pull <ref>` and `podman rmi <ref>` as the
two ways to refresh. The line is built by `baseline_summary_line` in
`pull_progress.rs` and printed after the pull viewport is cleared, so it
survives in the interactive terminal. Do not route it through the
pull-progress callback: in the default terminal mode that callback draws a
transient viewport that is erased when extraction finishes. The source is
not a field on `BaselineData` because that type is serialized into the
snapshot.

## Digest of a loaded image

The local branch makes `capture_image_digest` reachable for an image that
was never pulled. It reads `{{.Digest}}`, falls back to
`{{index .RepoDigests 0}}`, and returns `DigestFailed` (exit 3) when both
are empty. Measured on a RHEL 9.8 host with podman 5.8.2, using
`quay.io/fedora/fedora-minimal:42` and the default `podman save` format:

```
# after podman pull
{{.Digest}}               sha256:cfc6a342f07e650527ab8ac56288b390d9442f98c85c20d42ae51b20b0447f74
{{index .RepoDigests 0}}  quay.io/fedora/fedora-minimal@sha256:09a2061e2cfb85ac8e7fa7f2234d0ace6ad4f2b7dfdf0f257c90405e4f07577d
{{.Id}}                   d6393d1ed6dbc71bf2516a8851a36a0378369498487ace1ff214f84009a8a75f

# after podman save -o img.tar, podman rmi, podman load -i img.tar
podman image exists       0
{{.Digest}}               sha256:8908e471a695a5f0ec293a762486fad0dbc8794cb19e126997e16f5d046a563d
{{index .RepoDigests 0}}  quay.io/fedora/fedora-minimal@sha256:8908e471a695a5f0ec293a762486fad0dbc8794cb19e126997e16f5d046a563d
{{.Id}}                   d6393d1ed6dbc71bf2516a8851a36a0378369498487ace1ff214f84009a8a75f
```

So a loaded image does carry a digest and the scan does not stop at
`DigestFailed`. The digest is not the registry's, though: `podman load`
writes a new manifest, so the recorded value matches neither registry
digest. Only the image ID survives the round trip. Every current consumer
of `BaselineData.image_digest` is display-only (the baseline table in the
audit report and README, the HTML report's baseline panel, the refine
baseline summary and the web banner's 12-character prefix, and the fleet
aggregate, which copies it). The Containerfile's `FROM` uses the image
reference, not the digest. Do not start using the digest as a registry pin
without handling loaded images. Not measured: `podman save --format
oci-archive`, and podman releases older than 5.8.2.

`baseline_local_image_without_digest_returns_digest_failed` pins the
empty-digest contract for the local branch with a mock.

## Test consequence

`MockExecutor` returns exit 127 for an unregistered command (see
[mock-executor-key-format](mock-executor-key-format.md)). Any non-zero exit
from `image exists` is treated as absent, so a mock with no
`podman image exists` entry still exercises the pull path. That keeps old
fixtures valid, and it also means a test meant to cover the local-image path
must register the key explicitly, or it silently tests the pull instead.
Command-log index assertions shift by one: `image exists` sits between
`which podman` and `podman pull`.
