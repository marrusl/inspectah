# Image Reference Validation

**Date:** 2026-04-26
**Status:** Proposed (revised per team review 2026-04-26)

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

Decomposed:

| Component | Rules | Case sensitivity |
|-----------|-------|------------------|
| **Registry** | Optional. Hostname or IP, optionally with `:port`. Default depends on container engine config (`docker.io`, `registry.redhat.io`, etc.) | Case-insensitive (DNS per RFC 4343). `REGISTRY.COM/repo:tag` is valid. |
| **Repository** | One or more `/`-separated path components. Each component: `[a-z0-9]+(?:[._-][a-z0-9]+)*` | Lowercase only. `REPO:tag` is invalid. |
| **Tag** | Optional after `:`. `[a-zA-Z0-9_.-]+`, max 128 chars. Cannot start with `.` or `-` | Mixed case allowed. |
| **Digest** | Optional after `@`. Format: `algorithm:hex` (e.g., `sha256:abc123...`). May coexist with tag. | Lowercase hex only (`[a-f0-9]+`). |

### Registry vs. repository disambiguation

The first component (before the first `/`, or the entire string if no
`/`) could be a hostname or a bare repository name. The rule:

- Contains a `.` or `:` → hostname (uppercase OK)
- Otherwise → bare repository name (lowercase only)

This matches how `docker.io/library/ubuntu` is parsed by the
distribution/reference library.

### Colon disambiguation

`host:5000/repo:tag` contains two colons. The parsing rule:

- A colon in the portion **before the first `/`**, followed by digits
  only, is a **port separator**.
- The last colon **after the final `/`-separated component** (and before
  any `@`) is the **tag separator**.

### Overall reference length

Maximum 4096 characters. Anything longer is rejected as likely garbage
input. This also protects against regex backtracking on pathological
strings.

### Valid examples

- `registry.redhat.io/rhel9/rhel-bootc:9.6`
- `quay.io/centos-bootc/centos-bootc:stream9`
- `localhost:5000/myimage:latest`
- `localhost/test:dev`
- `myimage:v1.0`
- `myimage` (bare name, no tag)
- `registry.example.com/org/repo@sha256:abcdef1234567890...`
- `docker.io/library/ubuntu:22.04`
- `registry.redhat.io/rhel9/rhel-bootc:9.6@sha256:abcdef...` (tag + digest)
- `REGISTRY.COM/repo:tag` (uppercase hostname is valid per DNS)

### Invalid examples

- `my image:tag` (spaces)
- `REGISTRY.COM/Repo:Tag` (uppercase in repository path component after `/`)
- `:just-a-tag` (no name)
- `repo:` (empty tag)
- `repo@` (empty digest)
- `repo@md5:abc` (non-OCI digest algorithm for strict mode)
- `https://registry.redhat.io/rhel9/rhel-bootc:9.6` (URL, not image ref)

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

1. Strip leading/trailing whitespace (paste artifacts from config
   files and scripts -- strip silently, don't warn)
2. Reject empty strings (after stripping)
3. Reject strings exceeding 4096 characters
4. Detect URL-shaped input (`http://` or `https://` prefix) and
   return a targeted error suggesting the user remove the protocol
5. Reject strings containing embedded whitespace or control characters
6. Split on `@` to separate optional digest (tag and digest may
   coexist -- `name:tag@digest` is valid)
7. Split remainder on `:` to separate optional tag, using the colon
   disambiguation rule (see grammar section above)
8. Validate the hostname/repository split: if the first component
   contains `.` or `:`, treat it as a hostname (uppercase OK);
   otherwise treat as a bare repo name (lowercase only). Path
   components after the first `/` are always lowercase only.
9. Validate tag format and length (max 128 chars)
10. Validate digest format (`algorithm:hex`, algorithm is `[a-z0-9]+`,
    hex is `[a-f0-9]+` lowercase only, minimum 32 chars for sha256)

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

For `build --tag` when no tag is provided:

```
Error: invalid image reference "myimage"
  --tag requires a name:tag format (tag is missing)

  Example: inspectah build output.tar.gz -t myimage:v1.0
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
- `name:tag@sha256:` + 64 hex chars -- tag + digest coexistence
- `REGISTRY.COM/repo:tag` -- uppercase hostname (DNS is case-insensitive)
- `" myimage:tag "` -- leading/trailing whitespace (stripped silently)

**Should reject:**
- `""` -- empty string
- `"my image:tag"` -- embedded spaces
- `"repo:"` -- empty tag after colon
- `"repo@"` -- empty digest
- `"repo@sha256:"` -- empty hex after algorithm
- `":tag"` -- no name
- `"repo@notadigest"` -- malformed digest (no algorithm separator)
- `"REPO:tag"` -- uppercase in bare repository name (no `.` or `:`, so not a hostname)
- `"REGISTRY.COM/Repo:tag"` -- uppercase in path component after `/`
- `"repo@sha256:ABCDEF..."` -- uppercase hex in digest (reject; lowercase only)
- String with control characters
- String with `\n` or `\t`
- `"https://registry.redhat.io/rhel9/rhel-bootc:9.6"` -- URL-shaped input
- `"http://localhost/image:tag"` -- URL-shaped input
- String >4096 characters -- length ceiling
- Unicode homoglyphs (`"rеgistry.io/repo:tag"` with Cyrillic 'е') -- the `[a-z0-9]` class rejects this, but an explicit test proves it

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
- No new dependencies required. The validator is self-contained.
