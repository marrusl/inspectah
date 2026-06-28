# Registry Auth Prompting

**Status:** Pre-spec (brainstorm seed)
**Date:** 2026-06-24

## Problem

When inspectah scan needs to pull a RHEL base image for baseline subtraction
and the user isn't logged in to registry.redhat.io (under sudo), the scan
fails with a pull error. The user has to figure out what went wrong, run
`sudo podman login registry.redhat.io` themselves, and re-run the scan.

## Idea

Instead of bailing out on a 401, inspectah should detect the auth failure
and prompt the user for registry credentials interactively. Either:

- Prompt for username/password directly and pass them to the registry API
- Offer to run `sudo podman login registry.redhat.io` inline and retry

## Context

- inspectah scan runs as root (sudo), so credentials need to exist in the
  root user's podman auth context
- This is a common first-run stumble for RHEL users — the docs tell you to
  log in, but it's easy to forget or miss the sudo
- Fedora/CentOS Stream users don't hit this since those registries are public

## Open Questions

- Should this be interactive-only, or also support a `--registry-user` /
  `--registry-password` flag for CI/scripted use?
- Should inspectah check auth status proactively before attempting the pull,
  or only react to the 401?
- Does this belong in the Rust CLI layer or deeper in the scan engine?
