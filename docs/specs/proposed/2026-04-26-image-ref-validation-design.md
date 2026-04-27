# Image Reference Validation

**Date:** 2026-04-26
**Status:** Proposed (revised round 2, 2026-04-26)

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

**In scope:** `--image`, `image use`, `build --tag`.

**Out of scope:** `--target-image` (passthrough), `INSPECTAH_IMAGE` env
var (validated when it hits `ResolveImage` -> same path as `--image`).

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

## Design Decision: `image use` and version-style inputs

The `image use` help text advertises:

> Accepts both v-prefixed (v0.5.1) and bare (0.5.1) versions

`0.5.1` is a valid input per `reference.Parse` -- the distribution
library treats it as a bare repository name containing dots and digits.
This is important: the validator must NOT reject it. The `image use`
command's version-to-tag expansion logic (stripping the `v` prefix and
constructing a full registry reference) runs **after** validation, so
the validator sees the raw user input.

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
// Returns nil if valid, or an error describing the structural problem.
func Validate(ref string) error

// ValidateBuildTarget checks whether ref is a valid image name for use
// as a build output tag (podman build -t / buildah tag). It accepts
// named references with an optional tag but rejects digests.
//
// A digest in a build target is nonsensical -- you're naming an image
// that doesn't exist yet, so you can't know its digest. If no tag is
// provided, a warning is returned (not an error) suggesting the user
// add one for reproducibility.
func ValidateBuildTarget(ref string) (warning string, err error)
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
   files and scripts -- strip silently, don't warn)
2. Reject empty strings (after stripping)
3. Reject strings exceeding 4096 characters (garbage-input guard,
   checked before `reference.Parse` to avoid feeding it pathological
   input)
4. Detect URL-shaped input (`http://` or `https://` prefix) and
   return a targeted error suggesting the user remove the protocol
5. Call `reference.Parse(ref)` -- if it returns an error, wrap it
   in inspectah's error format with actionable guidance
6. For `Validate`: verify the result is a `reference.Named` (rejects
   bare digests/IDs)
7. For `ValidateBuildTarget`: additionally check that the result does
   NOT contain a digest component, and warn if no tag is present

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

```go
// In newImageUseCmd(), before SavePinnedImage:
if err := imageref.Validate(ref); err != nil {
    return err
}
```

### `--image` global flag

Validation runs on the **raw flag value before resolution**. If the
user passes `--image "garbage"` but a valid pin exists, the garbage
must still be rejected -- it should not be silently swallowed by the
fallback chain in `ResolveImage`.

```go
// In PersistentPreRunE, BEFORE ResolveImage:
if opts.Image != "" {
    if err := imageref.Validate(opts.Image); err != nil {
        return err
    }
}
// Then resolve (flag -> env -> pin -> default):
opts.Image = container.ResolveImage(opts.Image, ...)
```

Note: `PersistentPreRun` currently doesn't return an error (it's
`PersistentPreRun`, not `PersistentPreRunE`). This will need to change
to `PersistentPreRunE` to propagate validation errors. The root command
already has `SilenceUsage: true`, so cobra won't dump usage text on
validation errors.

### `build --tag`

```go
// In newBuildCmd(), after the empty check:
warning, err := imageref.ValidateBuildTarget(tag)
if err != nil {
    return err
}
if warning != "" {
    fmt.Fprintln(os.Stderr, warning)
}
```

## Test Cases

### `imageref.Validate`

**Should accept:**
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
- `" myimage:tag "` -- leading/trailing whitespace (stripped silently)

**Should reject:**
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
- `image use 0.5.1` succeeds (version-style input must not be rejected)
- `image use v0.5.1` succeeds (v-prefixed version)
- `build -t "bad ref" input.tar.gz` returns non-zero with validation error
- `build -t myimage input.tar.gz` succeeds with warning about missing tag
- `build -t "img@sha256:abc..." input.tar.gz` returns non-zero (digest rejected)
- `--image "bad ref" scan` returns non-zero with validation error

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

## Migration Notes

- The `PersistentPreRun` -> `PersistentPreRunE` change is safe; cobra
  supports both and the existing function body doesn't return errors
  that need suppressing.
- Existing pinned images in `~/.config/inspectah/config.json` are
  validated on load with a **warning** (not a hard error). If a stored
  ref fails `Validate`, emit:
  ```
  Warning: pinned image "registery.redhat.io/..." may be invalid
    Use 'inspectah image use <ref>' to update your pinned image.
  ```
  This doesn't block the user but surfaces the problem at the right
  moment, preventing confusion when the same ref would be rejected
  by `image use`.
- The old `ValidateTag` function name is retired. The new
  `ValidateBuildTarget` name makes the contract explicit: this
  validates a build output name, not a tag component.

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
