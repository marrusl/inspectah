# CLI Output UX Specification

**Author:** Fern (UX Specialist)
**Date:** 2026-04-24
**Status:** Proposal for Mark's review

---

## Problem Statement

`inspectah scan` involves two distinct container image pulls controlled by
different layers of the stack. Users — particularly sysadmins who are not
container-native — see raw podman output for both and have no way to understand
what is happening or why. The first-run experience feels chaotic; subsequent
runs feel noisy for no reason.

## Architecture Constraints

| Pull | Controller | Progress format | Controllable? |
|------|-----------|----------------|---------------|
| **Tool image** (ghcr.io/marrusl/inspectah:TAG) | Go wrapper `EnsureImage()` | `StreamPullProgress` on stderr | Yes — full control |
| **Base image** (registry.redhat.io/rhel9/rhel-bootc:9.6) | Python `BaselineResolver.pull_image()` via nsenter | Raw `podman pull` stderr inherited | Partially — Python prints the intro line; podman renders its own layer progress |

The Go wrapper cannot modify the Python-side output without changing the
Python code. Both sides need coordinated changes.

## Design Principles

1. **Phase labels, not image refs.** Users care about *what the tool is doing*,
   not which OCI blob is transferring. Lead with intent; put the image ref
   in dim/parenthetical.
2. **No surprises on repeat runs.** If a pull is skipped (cached), say so in
   one line. Never silently swallow a phase the user saw last time.
3. **Progressive disclosure.** The default is compact. `--verbose` shows image
   refs and podman commands. `INSPECTAH_DEBUG=1` shows everything.
4. **One visual language.** Both Go and Python phases use the same prefix
   style so the experience reads as one tool, not two glued together.
5. **Errors are actionable.** Every error message includes a "Fix:" or
   "Alternative:" line. No naked stack traces in normal mode.

---

## Visual Language

### Prefix system

```
Phase markers:   ── [phase]  Title
Step counters:   ── [n/N]    Title
Status:            ok  message
Cached:            --  message (skipped, cached)
Error prefix:    Error:  message
Hint indented:     Fix:  actionable step
```

All phase/step markers go to **stderr**. Final output summary goes to
**stdout** (as today).

### Color usage (TTY only)

| Element | Color | ANSI |
|---------|-------|------|
| Phase marker dashes `──` | cyan | `\033[36m` |
| Step counter `[n/N]` | dim | `\033[2m` |
| Phase/step title | bold | `\033[1m` |
| ok checkmark | green | `\033[32m` |
| Cached `--` | dim | `\033[2m` |
| Error keyword | bold (no color — already stands out) | `\033[1m` |
| Image refs in parens | dim | `\033[2m` |
| Podman layer progress | unmodified (podman controls) | — |

Non-TTY: all ANSI codes suppressed. Phase markers and step counters use
plain text brackets.

---

## Scenario Mockups

### Scenario 1: First Run (Both Images Need Pulling)

```
inspectah v0.5.1

── [setup]  Pulling migration tool...
            (ghcr.io/marrusl/inspectah:0.5.1 — cached after first pull)
  Copying blob sha256:a1b2c3d4...
  Copying blob sha256:e5f6a7b8...
  Writing manifest
   ok  Migration tool ready.

── [scan]   Scanning host...

── [1/12]   Packages
            Pulling baseline image (registry.redhat.io/rhel9/rhel-bootc:9.6)...
  Copying blob sha256:1a2b3c4d...
  Copying blob sha256:5e6f7a8b...
  Writing manifest
── [2/12]   Config files
── [3/12]   Services
── [4/12]   Network
── [5/12]   Storage
── [6/12]   Scheduled tasks
── [7/12]   Containers
── [8/12]   Non-RPM software
── [9/12]   Kernel / boot
── [10/12]  SELinux / security
── [11/12]  Users / groups
── [12/12]  Package preflight
   ok  Inspection complete.

Output: my-host-2026-04-24.tar.gz

Next steps:
  Copy to workstation:    scp my-host:my-host-2026-04-24.tar.gz .
  Interactive refinement: inspectah refine my-host-2026-04-24.tar.gz
  Build the image:        inspectah build my-host-2026-04-24.tar.gz
```

**Notes:**
- The `(ghcr.io/... — cached after first pull)` line tells the user this
  one-time cost will not repeat. Dim text, not shouty.
- The base image pull appears *inside* the `[1/12] Packages` step because
  that is where it actually happens in the code (baseline resolution is
  part of the RPM inspector). This is truthful — we are not hiding it.
- The step count changes from `[1/11]` to `[1/12]` (the 12th step is
  the "Package preflight" step that conditionally runs).

### Scenario 2: Subsequent Run (Tool Cached, Base Image Cached)

```
inspectah v0.5.1

── [setup]  Migration tool ready. (cached)
── [scan]   Scanning host...

── [1/12]   Packages
── [2/12]   Config files
── [3/12]   Services
── [4/12]   Network
── [5/12]   Storage
── [6/12]   Scheduled tasks
── [7/12]   Containers
── [8/12]   Non-RPM software
── [9/12]   Kernel / boot
── [10/12]  SELinux / security
── [11/12]  Users / groups
── [12/12]  Package preflight
   ok  Inspection complete.

Output: my-host-2026-04-24.tar.gz

Next steps:
  Copy to workstation:    scp my-host:my-host-2026-04-24.tar.gz .
  Interactive refinement: inspectah refine my-host-2026-04-24.tar.gz
  Build the image:        inspectah build my-host-2026-04-24.tar.gz
```

**Notes:**
- "Migration tool ready. (cached)" — one line, done. The user knows the
  tool checked and moved on.
- The base image is also cached, so `[1/12] Packages` proceeds directly
  with no pull output.
- This is the common case. It should feel fast and clean.

### Scenario 3: Subsequent Run (Tool Cached, Base Image Needs Pulling)

```
inspectah v0.5.1

── [setup]  Migration tool ready. (cached)
── [scan]   Scanning host...

── [1/12]   Packages
            Pulling baseline image (registry.redhat.io/rhel9/rhel-bootc:9.6)...
  Copying blob sha256:1a2b3c4d...
  Writing manifest
── [2/12]   Config files
...
```

**Notes:**
- This happens when the user targets a different version, or the base
  image cache was pruned. The pull appears under `[1/12]` because that
  is where it actually occurs.

### Scenario 4: Pinned Version Run

```
inspectah v0.5.1

── [setup]  Migration tool ready. (pinned: ghcr.io/marrusl/inspectah:0.4.0)
── [scan]   Scanning host...
...
```

**Notes:**
- If the pinned image was not yet pulled, the full pull output appears
  under `[setup]` as in Scenario 1.
- The `(pinned: ...)` suffix tells the user they are on a specific
  version, not the default. This matters for debugging.

### Scenario 5: Air-Gapped / Pull Failure

#### 5a. Tool image missing, `--pull=never`

```
inspectah v0.5.1

Error: Migration tool container not found locally.
  Image: ghcr.io/marrusl/inspectah:0.5.1
  Fix:   Transfer the image to this host and import it:
           podman load -i inspectah-0.5.1.tar
         Or allow pulling: inspectah scan --pull=missing
```

#### 5b. Base image pull fails (no registry auth)

```
inspectah v0.5.1

── [setup]  Migration tool ready. (cached)
── [scan]   Scanning host...

── [1/12]   Packages
  ERROR: No credentials for registry.redhat.io.
         The base image cannot be pulled without authentication.
  Fix:   Run 'sudo podman login registry.redhat.io' on the host first.
  Alt:   Provide a pre-exported package list:
           inspectah scan --baseline-packages packages.txt
  Alt:   Run without baseline comparison (not recommended):
           inspectah scan --no-baseline
```

#### 5c. Network timeout

```
inspectah v0.5.1

── [setup]  Pulling migration tool...
            (ghcr.io/marrusl/inspectah:0.5.1)

Error: Image pull timed out. Check your network connection.
  For offline use: inspectah scan --pull=never
  To import a pre-pulled image: podman load -i inspectah-0.5.1.tar
```

#### 5d. Base image pull timeout

```
── [1/12]   Packages
            Pulling baseline image (registry.redhat.io/rhel9/rhel-bootc:9.6)...
  ERROR: podman pull timed out after 300s.
  Fix:   Check network connectivity to registry.redhat.io.
  Alt:   Provide a pre-exported package list:
           inspectah scan --baseline-packages packages.txt
```

### Scenario 6: Dry Run

```
inspectah v0.5.1

── [setup]  Migration tool ready. (cached)
── [dry-run] Would execute:

podman run --rm --pid=host --privileged \
  --security-opt label=disable \
  -v /:/host:ro \
  -v /root/output:/output \
  -e INSPECTAH_HOST_CWD=/root/output \
  -e INSPECTAH_HOSTNAME=my-host \
  ghcr.io/marrusl/inspectah:0.5.1 scan

No changes made.
```

**Notes:**
- Dry-run still checks the tool image (and pulls if needed with
  `--pull=missing`) because the user may want to verify the pull works.
  To skip even that: `--pull=never --dry-run`.
- The podman command is formatted for readability with line continuations.
- "No changes made." is explicit reassurance.

---

## Implementation Changes

### Go wrapper changes (`cmd/inspectah/`)

#### 1. Add first-run detection to `EnsureImage`

In `ensure.go`, detect whether the image existed before the pull attempt.
Pass a `firstRun bool` back to the caller (or detect via the `missing`
policy path — if the image-exists check fails and we proceed to pull,
it is a first run for this image).

```go
// In pullImage(), change the opening message:
fmt.Fprintf(w, "── [setup]  Pulling migration tool...\n")
fmt.Fprintf(w, "            (%s — cached after first pull)\n", image)
// ... existing pull progress ...
fmt.Fprintf(w, "   ok  Migration tool ready.\n")
```

When image is already cached (the `missing` path where `checkImageExists`
succeeds):

```go
fmt.Fprintf(w, "── [setup]  Migration tool ready. (cached)\n")
```

#### 2. Add pinned-image indicator

When a pinned image is in use, append it:

```go
fmt.Fprintf(w, "── [setup]  Migration tool ready. (pinned: %s)\n", image)
```

#### 3. Add version line at scan start

In `scan.go`, before calling `EnsureImage`:

```go
fmt.Fprintf(os.Stderr, "inspectah %s\n\n", version)
```

The version string comes from the build-time ldflags already wired to
`NewRootCmd`.

#### 4. Dry-run formatting

In `scan.go`, when `dryRun` is true, format the podman command with
line continuations:

```go
fmt.Fprintf(os.Stderr, "── [dry-run] Would execute:\n\n")
// print formatted command
fmt.Fprintf(os.Stderr, "\nNo changes made.\n")
```

#### 5. Update `StreamPullProgress` formatting

In `progress.go`, indent pull progress lines consistently:

```go
fmt.Fprintf(w, "  %s\n", line)  // already does this — keep it
```

#### 6. Add scan-phase marker

After `EnsureImage` succeeds, before `runner.Exec`:

```go
fmt.Fprintf(os.Stderr, "── [scan]   Scanning host...\n\n")
```

### Python changes (`src/inspectah/`)

#### 7. Align `section_banner` format

In `_util.py`, update `section_banner` to match the Go wrapper style:

```python
def section_banner(title: str, step: int, total: int) -> None:
    counter = f"{_C.DIM}[{step}/{total}]{_C.RESET}"
    print(f"{_C.CYAN}──{_C.RESET} {counter:<8s} {_C.BOLD}{title}{_C.RESET}",
          file=sys.stderr)
```

Key change: drop the trailing rule (`───────`) — it adds visual noise
for no information. The step counter and bold title are sufficient
landmarks.

#### 8. Adjust total step count

In `inspectors/__init__.py`, change `_TOTAL_STEPS = 11` to `12` and
renumber the package-preflight step from `_TOTAL_STEPS` to `12`
explicitly (it is currently `_TOTAL_STEPS, _TOTAL_STEPS` which was a
workaround).

Alternatively, keep the dynamic count but make the preflight step
always show (even when skipped — just print the banner and move on).
This prevents the counter from being `[1/11]` on some runs and
`[1/12]` on others, which looks like a bug.

**Recommendation:** Always show 12 steps. When preflight is skipped,
print the banner and immediately print `-- skipped` in dim. Consistency
is more important than shaving one line.

#### 9. Update base-image pull message

In `baseline.py`, `pull_image()`:

```python
# Before:
print(f"  Pulling baseline image {base_image}...", file=sys.stderr)

# After:
print(f"            Pulling baseline image ({base_image})...", file=sys.stderr)
```

The 12-space indent aligns the message under the `[1/12]   Packages`
banner. The image ref goes in parentheses per our dim-ref convention.

#### 10. Update `status()` function

```python
def status(msg: str) -> None:
    print(f"   {_C.GREEN}ok{_C.RESET}  {msg}", file=sys.stderr)
```

Change from the checkmark Unicode character to plain `ok`. Rationale:
not all terminals render Unicode Nerd Font glyphs. The current ``
is a Nerd Font codepoint that renders as a box on stock RHEL terminals.
`ok` is universal.

---

## Step Count: Always 12

| Step | Inspector |
|------|-----------|
| 1 | Packages |
| 2 | Config files |
| 3 | Services |
| 4 | Network |
| 5 | Storage |
| 6 | Scheduled tasks |
| 7 | Containers |
| 8 | Non-RPM software |
| 9 | Kernel / boot |
| 10 | SELinux / security |
| 11 | Users / groups |
| 12 | Package preflight |

When preflight is skipped (no baseline available, or `--skip-unavailable`):

```
── [12/12]  Package preflight (skipped)
```

---

## What This Spec Does NOT Change

- **Output format.** The tarball structure, JSON schema, and rendered
  artifacts are untouched.
- **The "Next steps" block.** Stays exactly as-is. It is already well
  designed.
- **Secrets/redaction warnings.** These are important and already well
  formatted. No changes.
- **Fleet/refine/architect/build subcommands.** This spec covers `scan`
  only. Those subcommands can adopt the same visual language later in a
  follow-up pass.
- **The `--verbose` flag behavior.** Currently prints the raw podman
  command. That stays. We just wrap it better for `--dry-run`.

---

## Open Questions for Mark

1. **Should `--pull=always` runs show "(refreshing)" instead of
   "(cached)"?** Currently `--pull=always` forces a pull even if cached.
   The user should know the tool is re-pulling intentionally. Proposed:
   `── [setup]  Refreshing migration tool... (--pull=always)`

2. **Version string source.** The Go wrapper has build-time version via
   ldflags. Should the Python tool also print its version? The Python
   version is baked into the container image and could differ from the
   wrapper version. Proposed: show only the wrapper version at the top
   (it controls the experience), and include the Python/image version
   in `inspectah version` output only.

3. **The Nerd Font checkmark.** The current `status()` function uses
   `` which requires Nerd Font. Should we switch to `ok` (this
   spec's recommendation), plain ASCII `[ok]`, or Unicode `✓` (U+2713,
   more widely supported than Nerd Font but still not universal)?
