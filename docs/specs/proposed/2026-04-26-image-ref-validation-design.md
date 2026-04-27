# Image Reference Validation

**Date:** 2026-04-26
**Status:** Proposed (revised round 3, 2026-04-26)

## Problem

`inspectah image use <image-ref>` and several other CLI surfaces accept
image references as raw strings with no validation. A typo like
`registery.redhat.io/rhel9/rhel-bootc:9.6` (note the misspelling) is
silently accepted and only fails later at `podman pull` time with an
opaque DNS error. The CLI should catch obviously invalid references
early and give actionable feedback.

## Scope

### Surfaces that accept image references

Audit of the Go CLI source (`cmd/inspectah/internal/cli/`):

| Surface | Variable | Current validation |
|---------|----------|--------------------|
| `--image` global flag | `root.go:48` `opts.Image` | None |
| `image use <ref>` | `image.go:93` `args[0]` | None |
| `build --tag` / `-t` | `build.go:244` `tag` | Empty check only |
| `--target-image` passthrough | `passthrough.go:12` | None (passed to container) |

The `--target-image` flag is a passthrough to the inner inspectah
container, which does its own validation. It is out of scope here --
the Go wrapper doesn't interpret it.

**In scope:** `--image`, `image use`, `build --tag`, `INSPECTAH_IMAGE`
env var, pinned config.

**Out of scope:** `--target-image` (passthrough to the inner container).

### Non-goals

- Registry reachability checks (that's what `podman pull` does)
- Tag existence validation
- Digest content verification

## Image Reference Grammar

Container image references follow the OCI distribution spec grammar.
The canonical form is:

```
[registry[:port]/]repository[:tag][@digest]
```

Tag and digest may both be present (`name:tag@digest`). This matches
podman and buildah behavior.

### Registry vs. repository disambiguation

This is a subtle area with many edge cases (bare `localhost`, single-
component names, dotted version strings, port-only hostnames). Rather
than hand-rolling a heuristic, inspectah delegates this entirely to
`github.com/distribution/reference`, which is the canonical parser used
by Docker, containerd, Podman, and buildah. See "Implementation
approach" below.

### Overall reference length

Maximum 4096 characters as a garbage-input guard. This protects against
pathological inputs and is checked **before** passing to
`reference.Parse`. It is not a semantic maximum -- the distribution
library enforces its own component-level limits (e.g., 128-char tags,
255-char repository names).

### Valid examples

- `registry.redhat.io/rhel9/rhel-bootc:9.6` -- standard registry/repo:tag
- `quay.io/centos-bootc/centos-bootc:stream9` -- standard
- `localhost:5000/myimage:latest` -- localhost with port
- `localhost/myimage:dev` -- localhost without port
- `myimage:v1.0` -- no registry
- `myimage` -- bare name, no tag
- `0.5.1` -- version-style bare name (used by `image use`, see below)
- `repo@sha256:` + 64 hex chars -- digest reference
- `docker.io/library/ubuntu:22.04` -- dots in tag
- `my-registry.example.com:8080/org/sub/repo:tag` -- deep path with port
- `image:v1.0-beta.1` -- hyphens and dots in tag
- `name:tag@sha256:` + 64 hex chars -- tag + digest coexistence
- `REGISTRY.COM/repo:tag` -- uppercase hostname (DNS is case-insensitive)

### Invalid examples

- `my image:tag` (spaces)
- `:just-a-tag` (no name)
- `repo:` (empty tag)
- `repo@` (empty digest)
- `https://registry.redhat.io/rhel9/rhel-bootc:9.6` (URL, not image ref)

Note: the full set of rejection rules (uppercase in repository
components, malformed digests, invalid characters, etc.) is defined by
`reference.Parse`. We do not enumerate them here because they would
drift from the library. The test suite covers the important edge cases.

## Design Decision: Validation Level

**Recommendation: Moderate** -- validate structural grammar without
network calls.

### Why not minimal (reject spaces/control chars)?

Too permissive. `not-a-registry!!!` passes minimal validation but is
never a valid image reference. Users pasting shell variables or
malformed strings would still get confusing podman errors downstream.

### Why not strict (pull test)?

Too slow and network-dependent. `image use` is a config operation that
should work offline. `build --tag` names the *output* image -- it
doesn't exist yet, so a pull test is nonsensical. And `--image` is
validated at pull time anyway via `EnsureImage`.

### Why moderate?

Catches the real user errors (typos in structure, stray characters,
missing components) without requiring network access. The resolved
image string is later handed to `podman image exists` and `podman pull`
in `EnsureImage` (see `container/ensure.go`). Using
`distribution/reference` -- the same parser those tools use internally
-- eliminates drift between what inspectah accepts and what podman
accepts.

## Design Decision: `image use` shorthand with canonicalization

The `image use` command accepts two input forms:

1. **Version shorthand:** `0.5.1`, `v0.5.1` -- expanded to a full
   image ref before persistence
2. **Full image ref:** `quay.io/custom/image:latest` -- saved as-is

### Detection

A version-like input is detected when the string:
- Has no slashes (no registry/repo path)
- Has no `@` (no digest)
- Matches a semver-ish pattern: optional `v` prefix followed by digits
  and dots (e.g., `0.5.1`, `v2.0.0-rc.1`)

This is a simple heuristic, not a strict semver parse. The key
invariant is: anything with a `/` is treated as a full ref. Anything
without a `/` that looks like a version number gets expanded.

### Canonicalization

Version inputs are expanded using the existing logic in
`version/check.go`:

```go
// version.NormalizeTag("v0.5.1") -> "0.5.1"  (strips v prefix)
// version.NormalizeTag("0.5.1")  -> "0.5.1"  (no-op)
// version.ToImageRef("ghcr.io", "marrusl/inspectah", "v0.5.1")
//   -> "ghcr.io/marrusl/inspectah:0.5.1"
```

The expanded ref is then validated and persisted. The raw shorthand
is never saved to config.

### Feedback

The command echoes the resolved ref so the user sees exactly what
was pinned:

```
$ inspectah image use 0.5.1
Pinned image: ghcr.io/marrusl/inspectah:0.5.1

$ inspectah image use v0.5.1
Pinned image: ghcr.io/marrusl/inspectah:0.5.1

$ inspectah image use quay.io/custom/image:latest
Pinned image: quay.io/custom/image:latest
```

### Ordering

The `image use` flow is: detect shorthand -> expand if needed ->
validate the final ref -> save to config. Validation always runs on
the canonical form, never on the raw shorthand.

## Design Decision: Bare digests and image IDs

Bare digests (`sha256:abcdef...`) and short image IDs (`abcdef123456`)
are NOT valid `reference.Parse` inputs -- they are not named references.
This is correct for inspectah's use case:

- `EnsureImage` calls `podman pull <ref>`, which requires a pullable
  reference (named, optionally with tag/digest). Bare digests and IDs
  are not pullable.
- `image use` pins a reference for future pulls. A bare digest is not
  useful as a pin because it has no registry context.

If a future need arises for ID-based lookups (e.g., `podman image
exists` accepts short IDs), that would be a separate validation
contract, not a loosening of `Validate()`.

## Validation Functions

### Package location

New package: `cmd/inspectah/internal/imageref/`

**Rationale:** This is a self-contained parsing concern. It doesn't
belong in `container/` (which handles podman execution) or `cli/`
(which wires up cobra commands). A dedicated package makes it
independently testable and importable from any CLI surface.

### API

```go
package imageref

import "github.com/distribution/reference"

// Validate checks whether ref is a structurally valid container image
// reference suitable for use with podman pull / podman image exists.
// It accepts any valid named reference: with or without registry,
// with or without tag, with or without digest, or both tag and digest.
//
// It does not accept bare digests or image IDs -- those are not named
// references and cannot be pulled.
//
// Returns the canonicalized ref (whitespace stripped, normalized) and
// nil error if valid, or ("", error) describing the structural problem.
// Callers MUST use the returned string, not the original input.
func Validate(ref string) (string, error)

// ValidateBuildTarget checks whether ref is a valid image name for use
// as a build output tag (podman build -t / buildah tag). It accepts
// named references with an optional tag but rejects digests.
//
// A digest in a build target is nonsensical -- you're naming an image
// that doesn't exist yet, so you can't know its digest. If no tag is
// provided, a warning is returned (not an error) suggesting the user
// add one for reproducibility.
//
// Returns the canonicalized ref and nil error if valid, plus an
// optional warning string. Callers MUST use the returned string.
func ValidateBuildTarget(ref string) (canonical string, warning string, err error)
```

Two entry points because the contracts differ:

- **`Validate`** -- for `--image` and `image use`. Accepts any named
  reference that `reference.Parse` considers valid. This is the
  "anything podman would accept as a pull target" contract.

- **`ValidateBuildTarget`** -- for `build --tag`. Accepts named
  references with an optional tag. Rejects digests (you can't
  digest-pin a build output). Returns a warning (not error) when no
  tag is present, to encourage explicit tagging without blocking valid
  usage like `inspectah build ... -t myimage`.

### Implementation approach

Use `reference.Parse` from `github.com/distribution/reference` as the
core parser. This is the canonical OCI reference parser used by Docker,
containerd, Podman, and buildah. It handles all edge cases correctly:

- Registry vs. repository disambiguation (dots, colons, slashes)
- Bare `localhost` as a hostname
- Single-component names
- Port parsing
- Tag and digest coexistence
- Component-level character and length validation

**Why not a bespoke regex?**

The resolved image string is later passed to `podman pull` and
`podman image exists` (see `container/ensure.go`). Those tools use
`distribution/reference` internally. Any parser drift between
inspectah's validator and the runtime parser becomes user-visible:
inspectah rejects something podman accepts, or (worse) inspectah
accepts something podman rejects with a confusing error. Using the
same library eliminates this class of bugs entirely.

inspectah already vendors dependencies (cobra, testify). Adding one
more well-scoped, purpose-built library is a reasonable tradeoff for
correctness.

The validator wraps `reference.Parse` with:

1. Strip leading/trailing whitespace (paste artifacts from config
   files and scripts -- strip silently, don't warn). The stripped
   string becomes the canonical ref returned to callers.
2. Reject empty strings (after stripping) -- return `("", error)`
3. Reject strings exceeding 4096 characters (garbage-input guard,
   checked before `reference.Parse` to avoid feeding it pathological
   input)
4. Detect URL-shaped input (`http://` or `https://` prefix) and
   return a targeted error suggesting the user remove the protocol
5. Call `reference.Parse(ref)` -- if it returns an error, wrap it
   in inspectah's error format with actionable guidance
6. For `Validate`: verify the result is a `reference.Named` (rejects
   bare digests/IDs). Return `(canonicalRef, nil)`.
7. For `ValidateBuildTarget`: additionally check that the result does
   NOT contain a digest component, and warn if no tag is present.
   Return `(canonicalRef, warning, nil)` or `(canonicalRef, "", nil)`.

## Error Messages

All surfaces use the same prefix: `invalid image reference`. The reason
line differentiates what's wrong. This keeps the user's mental model
simple -- there's one concept (image reference) and the CLI tells you
exactly what's wrong with yours.

```
Error: invalid image reference "my image:tag"
  image references cannot contain spaces

  Expected format: [registry/]name[:tag]
  Examples:
    registry.redhat.io/rhel9/rhel-bootc:9.6
    quay.io/centos-bootc/centos-bootc:stream9
    localhost:5000/myimage:latest
```

For `build --tag` when a digest is provided:

```
Error: invalid build target "myimage@sha256:abc..."
  --tag names a build output and cannot include a digest
  (the image doesn't exist yet, so its digest is unknown)

  Example: inspectah build output.tar.gz -t myimage:v1.0
```

For `build --tag` when no tag is provided (warning, not error):

```
Warning: build target "myimage" has no tag
  Consider adding a tag for reproducibility: myimage:v1.0
```

For URL-shaped input:

```
Error: invalid image reference "https://registry.redhat.io/rhel9/rhel-bootc:9.6"
  image references are not URLs -- remove the https:// prefix

  Did you mean: registry.redhat.io/rhel9/rhel-bootc:9.6
```

For empty input (e.g., from unset shell variable `$MY_IMAGE`):

```
Error: no image reference provided
  Set the image with: inspectah image use <registry/name:tag>
```

Keep the example list short (3 examples max). Don't dump the full
grammar -- users don't need to know the regex.

## Integration Points

### `image use`

The `image use` command detects version shorthand, expands it, then
validates and persists the canonical ref.

```go
// In newImageUseCmd(), before SavePinnedImage:

// 1. Detect version shorthand and expand
if looksLikeVersion(ref) {
    ref = version.ToImageRef(version.DefaultRegistry, version.DefaultRepo, ref)
}

// 2. Validate and canonicalize
ref, err := imageref.Validate(ref)
if err != nil {
    return err
}

// 3. Persist the canonical ref (never raw input)
if err := container.SavePinnedImage(ref); err != nil {
    return fmt.Errorf("failed to save pin: %w", err)
}
fmt.Printf("Pinned image: %s\n", ref)
```

The `looksLikeVersion` helper detects strings with no slashes, no `@`,
and a leading digit or `v` followed by digits and dots.

### `--image` global flag (eager validation)

The `--image` flag is validated eagerly at parse time -- before
`ResolveImage`. If the user passes `--image "garbage"` but a valid pin
exists, the garbage must still be rejected. It should not be silently
swallowed by the fallback chain.

```go
// In PersistentPreRunE, BEFORE ResolveImage:
if opts.Image != "" {
    validated, err := imageref.Validate(opts.Image)
    if err != nil {
        return fmt.Errorf("--image: %w", err)
    }
    opts.Image = validated  // use canonical form
}
// Then resolve (flag -> env -> pin -> default).
// ResolveImage now returns (string, error) -- env and pin are
// validated lazily inside it.
resolved, err := container.ResolveImage(opts.Image, envValue, pinnedValue, defaultValue)
if err != nil {
    return err
}
opts.Image = resolved
```

Note: `PersistentPreRun` currently doesn't return an error (it's
`PersistentPreRun`, not `PersistentPreRunE`). This will need to change
to `PersistentPreRunE` to propagate validation errors. The root command
already has `SilenceUsage: true`, so cobra won't dump usage text on
validation errors.

### `INSPECTAH_IMAGE` env var (lazy validation)

The env var is validated lazily inside `ResolveImage`, not at parse
time. This avoids errors when the env var is set but the `--image`
flag takes precedence.

```go
func ResolveImage(flagValue, envValue, pinnedValue, defaultValue string) (string, error) {
    if flagValue != "" {
        // Already validated eagerly by PersistentPreRunE
        return flagValue, nil
    }
    if envValue != "" {
        validated, err := imageref.Validate(envValue)
        if err != nil {
            return "", fmt.Errorf("invalid image reference from INSPECTAH_IMAGE: %w", err)
        }
        return validated, nil
    }
    if pinnedValue != "" {
        validated, err := imageref.Validate(pinnedValue)
        if err != nil {
            return "", fmt.Errorf("invalid pinned image in config — run `inspectah image use` to update: %w", err)
        }
        return validated, nil
    }
    return defaultValue, nil
}
```

This is a **breaking signature change** to `ResolveImage`: it now
returns `(string, error)`. All callers must handle the error. The
`source` label is baked into each error message so the user knows
which config surface produced the bad ref.

### `build --tag`

```go
// In newBuildCmd(), after the empty check:
tag, warning, err := imageref.ValidateBuildTarget(tag)
if err != nil {
    return err
}
if warning != "" {
    fmt.Fprintln(os.Stderr, warning)
}
// Use 'tag' (canonical) from here on
```

## Validation Timing Summary

| Source | Timing | Error prefix |
|--------|--------|--------------|
| `--image` flag | Eager (PersistentPreRunE) | `--image: invalid image reference "..."` |
| `INSPECTAH_IMAGE` env | Lazy (ResolveImage) | `invalid image reference from INSPECTAH_IMAGE: ...` |
| Pinned config | Lazy (ResolveImage) | `invalid pinned image in config — run \`inspectah image use\` to update: ...` |
| `image use <ref>` | Eager (before save) | `invalid image reference "..."` |
| `build --tag` | Eager (before build) | `invalid build target "..."` |

## Test Cases

### `imageref.Validate`

**Should accept (return canonical ref, nil):**
- `registry.redhat.io/rhel9/rhel-bootc:9.6` -- standard registry/repo:tag
- `quay.io/centos-bootc/centos-bootc:stream9` -- standard
- `localhost:5000/myimage:latest` -- localhost with port
- `localhost/myimage:dev` -- localhost without port
- `myimage:v1.0` -- no registry
- `myimage` -- bare name, no tag
- `0.5.1` -- version-style input (used by `image use`)
- `v0.5.1` -- v-prefixed version (used by `image use`)
- `repo@sha256:` + 64 hex chars -- digest reference
- `docker.io/library/ubuntu:22.04` -- dots in tag
- `my-registry.example.com:8080/org/sub/repo:tag` -- deep path with port
- `image:v1.0-beta.1` -- hyphens and dots in tag
- `name:tag@sha256:` + 64 hex chars -- tag + digest coexistence
- `" myimage:tag "` -- leading/trailing whitespace (stripped; returned
  string is `myimage:tag`)

**Should reject (return "", error):**
- `""` -- empty string
- `"my image:tag"` -- embedded spaces
- `":tag"` -- no name
- `"repo:"` -- empty tag after colon (rejected by `reference.Parse`)
- `"repo@"` -- empty digest (rejected by `reference.Parse`)
- String with control characters
- String with `\n` or `\t`
- `"https://registry.redhat.io/rhel9/rhel-bootc:9.6"` -- URL-shaped input
- `"http://localhost/image:tag"` -- URL-shaped input
- String >4096 characters -- length ceiling
- Unicode homoglyphs (`"registrе.io/repo:tag"` with Cyrillic 'e') --
  `reference.Parse` rejects non-ASCII

**Delegated to `reference.Parse`** (not tested with bespoke assertions,
validated by the library's own test suite):
- Uppercase in repository path components
- Malformed digest algorithm/hex
- Component length limits (128-char tags, 255-char repo names)
- Repeated separators, invalid tokens

### `imageref.ValidateBuildTarget`

**Should accept:**
- `myimage:v1.0` -- name with tag
- `registry.example.com/repo:tag` -- full reference with tag
- `myimage` -- bare name, no tag (accepted with warning)
- `localhost:5000/myimage:latest` -- with port and tag

**Should reject:**
- Everything `Validate` rejects
- `repo@sha256:` + 64 hex chars -- digest (nonsensical for build output)
- `name:tag@sha256:` + 64 hex chars -- tag+digest (digest not allowed)

**Should warn (return warning string, nil error):**
- `myimage` -- no tag (encourage explicit tagging)
- `localhost/myimage` -- no tag

### CLI integration tests

- `image use "invalid ref"` returns non-zero exit with validation error
- `image use 0.5.1` succeeds, prints `Pinned image: ghcr.io/marrusl/inspectah:0.5.1`
- `image use v0.5.1` succeeds, prints same expanded ref (v stripped)
- `image use quay.io/custom/image:latest` succeeds, prints ref as-is
- `image use " 0.5.1 "` succeeds (whitespace stripped before expansion)
- `build -t "bad ref" input.tar.gz` returns non-zero with validation error
- `build -t myimage input.tar.gz` succeeds with warning about missing tag
- `build -t "img@sha256:abc..." input.tar.gz` returns non-zero (digest rejected)
- `--image "bad ref" scan` returns non-zero with `--image:` prefix in error
- `INSPECTAH_IMAGE="bad ref" inspectah scan` returns non-zero with
  `INSPECTAH_IMAGE` attribution in error

## Dependencies

### New dependency: `github.com/distribution/reference`

**What:** The canonical OCI container image reference parser. Used by
Docker, containerd, Podman, and buildah.

**Why:** Eliminates parser drift between inspectah's validation and the
container runtimes that consume the validated reference. See
"Implementation approach" above.

**Size:** Small, purpose-built library with minimal transitive
dependencies. inspectah already vendors cobra and testify -- this is a
lighter addition than either of those.

**Version:** Use the latest stable release (v0.6.x as of this writing).

## Implementation Notes

- The `PersistentPreRun` -> `PersistentPreRunE` change is safe; cobra
  supports both and the existing function body doesn't return errors
  that need suppressing.
- `ResolveImage` signature changes from `(string)` to `(string, error)`.
  All callers must handle the new error return. This is the mechanism
  for lazy validation of env var and pinned config sources.
- Existing pinned images in `~/.config/inspectah/config.json` are now
  validated lazily inside `ResolveImage`. A bad pinned ref produces a
  clear error: `invalid pinned image in config — run 'inspectah image
  use' to update`. This is a hard error (not a warning) because the
  ref would fail at `podman pull` time anyway -- failing early with
  actionable guidance is better.
- The old `ValidateTag` function name is retired. The new
  `ValidateBuildTarget` name makes the contract explicit: this
  validates a build output name, not a tag component.
- `build.go` help text currently says the tag is required. Since
  `inspectah build -t myimage` (no tag component) is now valid (warning,
  not error), the help text should be updated to reflect this. Not part
  of this commit -- note for implementation.

## Revision History

- **Round 1 (2026-04-26):** Initial draft with bespoke regex approach.
- **Round 2 (2026-04-26):** Major revision based on review feedback:
  - Replaced hand-rolled regex with `github.com/distribution/reference`
    to eliminate parser drift with container runtimes.
  - Fixed `build --tag` contract: renamed `ValidateTag` to
    `ValidateBuildTarget`, reject digests, make missing tag a warning
    instead of an error.
  - Removed incorrect registry/repo disambiguation heuristic (the
    "contains `.` or `:`" rule). `reference.Parse` handles this
    correctly.
  - Added `0.5.1` and `v0.5.1` to accepted test cases per `image use`
    help text.
  - Resolved bare digest / image ID question: intentionally out of
    scope (not pullable references).
  - Removed self-contained regex implementation details that would
    have drifted from the library.
- **Round 3 (2026-04-26):** Three blocker fixes plus implementation note:
  - `Validate` and `ValidateBuildTarget` now return `(string, error)` --
    the canonicalized ref (whitespace stripped, normalized) must be used
    by all callers, not the raw input.
  - `image use` shorthand: version-like inputs (`0.5.1`, `v0.5.1`) are
    expanded to full image refs (`ghcr.io/marrusl/inspectah:0.5.1`)
    before validation and persistence. Full refs pass through unchanged.
  - Hybrid validation timing: `--image` flag validated eagerly at parse
    time; `INSPECTAH_IMAGE` env var and pinned config validated lazily
    inside `ResolveImage` with source-attributed error messages.
    `ResolveImage` signature changes to `(string, error)`.
  - Noted that `build.go` help text should be updated (tag not strictly
    required) -- deferred to implementation.
