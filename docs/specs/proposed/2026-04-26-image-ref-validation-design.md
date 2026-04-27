# Image Reference Validation

**Date:** 2026-04-26
**Status:** Proposed

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
[registry[:port]/]repository[:tag|@digest]
```

Decomposed:

| Component | Rules |
|-----------|-------|
| **Registry** | Optional. Hostname or IP, optionally with `:port`. Default depends on container engine config (`docker.io`, `registry.redhat.io`, etc.) |
| **Repository** | One or more `/`-separated path components. Each component: `[a-z0-9]+(?:[._-][a-z0-9]+)*` |
| **Tag** | Optional after `:`. `[a-zA-Z0-9_.-]+`, max 128 chars. Cannot start with `.` or `-` |
| **Digest** | Optional after `@`. Format: `algorithm:hex` (e.g., `sha256:abc123...`). Mutually exclusive with tag in strict parsing, but some tools allow both |

### Valid examples

- `registry.redhat.io/rhel9/rhel-bootc:9.6`
- `quay.io/centos-bootc/centos-bootc:stream9`
- `localhost:5000/myimage:latest`
- `localhost/test:dev`
- `myimage:v1.0`
- `myimage` (bare name, no tag)
- `registry.example.com/org/repo@sha256:abcdef1234567890...`
- `docker.io/library/ubuntu:22.04`

### Invalid examples

- `my image:tag` (spaces)
- `REGISTRY.COM/Repo:Tag` (uppercase in repository path)
- `:just-a-tag` (no name)
- `repo:` (empty tag)
- `repo@` (empty digest)
- `repo@md5:abc` (non-OCI digest algorithm for strict mode)

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
missing components) without requiring network access. Matches what
`podman tag` and `buildah tag` do internally. Gives clear error
messages pointing to the specific structural problem.

## Validation Function

### Package location

New package: `cmd/inspectah/internal/imageref/`

**Rationale:** This is a self-contained parsing concern. It doesn't
belong in `container/` (which handles podman execution) or `cli/`
(which wires up cobra commands). A dedicated package makes it
independently testable and importable from any CLI surface.

### API

```go
package imageref

// Validate checks whether ref is a structurally valid container image
// reference. It does not check registry reachability or image existence.
// Returns nil if valid, or an error describing the structural problem.
func Validate(ref string) error

// ValidateTag checks whether ref is a valid image name:tag suitable
// for use as a build output tag. Same grammar as Validate but tag
// component is required.
func ValidateTag(ref string) error
```

Two entry points because the validation differs:
- `Validate` -- for `--image` and `image use`, where a bare name or
  digest-only reference is acceptable
- `ValidateTag` -- for `build --tag`, where a tag is required (you
  need to name your output)

### Implementation approach

Use `reference.Parse` from the
`github.com/distribution/reference` library if we want to add the
dependency, or implement a ~60-line regex-based validator matching the
OCI grammar. Given that inspectah already has minimal deps (cobra +
testify only), a self-contained implementation is preferable.

The validator should:

1. Reject empty strings
2. Reject strings containing whitespace or control characters
3. Split on `@` to separate optional digest
4. Split remainder on `:` to separate optional tag (being careful
   about registry ports -- `host:5000/repo:tag` has two colons)
5. Validate each component against its character class
6. Validate tag length (max 128 chars)
7. Validate digest format (`algorithm:hex`, algorithm is `[a-z0-9]+`,
   hex is `[a-f0-9]+` with minimum length)

## Error Messages

Errors should name the problem and show the expected format:

```
Error: invalid image reference "my image:tag"
  image references cannot contain spaces

  Expected format: [registry/]name[:tag]
  Examples:
    registry.redhat.io/rhel9/rhel-bootc:9.6
    quay.io/centos-bootc/centos-bootc:stream9
    localhost:5000/myimage:latest
```

For `build --tag` when no tag is provided:

```
Error: invalid build tag "myimage"
  --tag requires a name:tag format

  Example: inspectah build output.tar.gz -t myimage:v1.0
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

```go
// In PersistentPreRun, after ResolveImage:
if opts.Image != "" {
    if err := imageref.Validate(opts.Image); err != nil {
        return err
    }
}
```

Note: `PersistentPreRun` currently doesn't return an error (it's
`PersistentPreRun`, not `PersistentPreRunE`). This will need to change
to `PersistentPreRunE` to propagate validation errors.

### `build --tag`

```go
// In newBuildCmd(), after the empty check:
if err := imageref.ValidateTag(tag); err != nil {
    return err
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
- `repo@sha256:` + 64 hex chars -- digest reference
- `docker.io/library/ubuntu:22.04` -- dots in tag
- `my-registry.example.com:8080/org/sub/repo:tag` -- deep path with port
- `image:v1.0-beta.1` -- hyphens and dots in tag

**Should reject:**
- `""` -- empty string
- `"my image:tag"` -- spaces
- `"repo:"` -- empty tag
- `"repo@"` -- empty digest
- `":tag"` -- no name
- `"repo@notadigest"` -- malformed digest (no algorithm separator)
- `"REPO:tag"` -- uppercase in repository component
- String with control characters
- String with `\n` or `\t`

### `imageref.ValidateTag`

**Should accept:** everything `Validate` accepts that includes a tag.

**Should reject:** everything `Validate` rejects, plus:
- `myimage` -- no tag (required for build output)
- `repo@sha256:...` -- digest-only (not useful as build output tag)

### CLI integration tests

- `image use "invalid ref"` returns non-zero exit with validation error
- `build -t "bad ref" input.tar.gz` returns non-zero with validation error
- `--image "bad ref" scan` returns non-zero with validation error

## Migration Notes

- The `PersistentPreRun` -> `PersistentPreRunE` change is safe; cobra
  supports both and the existing function body doesn't return errors
  that need suppressing.
- Existing pinned images in `~/.config/inspectah/config.json` are not
  re-validated. If a user has an invalid ref pinned from before this
  change, it will fail at pull time as before. This is acceptable --
  re-validating stored config on every run adds complexity for a case
  that essentially doesn't exist.
- No new dependencies required. The validator is self-contained.
