# RPM Preflight Check: Package Availability Validation Before Rendering

**Date:** 2026-04-09
**Status:** Proposed
**Author:** Kiwi (orchestrator, synthesizing team input)
**Contributors:** Thorn (build failure analysis), Fern (UX/interaction design), Ember (product strategy)
**Revision:** 3 (round-2 tightenings 2026-04-09)

---

## Overview

Validate that all packages in the generated install list actually exist in the target repos before rendering the Containerfile. Surface unavailable packages, direct-install RPMs, and unreachable repos as structured diagnostics so users can resolve migration gaps before burning a build cycle.

**Scope:** CLI preflight check (single-system) and architect fleet aggregation. No replacement mapping or fix suggestions — inspectah reports what doesn't resolve and gets out of the way.

## Context

### The problem

inspectah inspects a source system, collects installed packages, and renders a Containerfile with `dnf install` lines. Today, there is no validation that those packages exist in the target base image's repos. The first signal a user gets is a failed `podman build` — potentially minutes into the build after pulling images and installing dozens of packages.

Real-world example: a Containerfile included `mcelog`, which was deprecated and removed from Fedora. The `dnf install` step failed, wasting the entire build. The fix was trivial (remove one package), but the feedback loop was slow and frustrating.

### What exists today

- **RPM inspector** (`inspectors/rpm.py`): collects packages from the source system, diffs against a baseline from the target base image via `BaselineResolver`. Produces `packages_added`, leaf/auto classification, version locks.
- **Packages renderer** (`renderers/containerfile/packages.py`): emits `dnf install` lines from the inspector output. Validates shell safety of package names but does not check repo availability.
- **Build validation** (`validate.py`): runs `podman build` against the rendered Containerfile and captures errors. This is post-render, post-fact validation.
- **No pre-render availability check exists.** This is the gap.

### Strategic context

This feature shifts inspectah from a translation tool ("here's your Containerfile") to a migration advisor ("here's your Containerfile, and here's what won't survive the transition"). Understanding the destination, not just the source, is where the tool earns trust with operators running real migrations.

---

## Decisions Log

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Standalone preflight module called from inspection, not inline in the renderer | Clean separation of concerns. Preflight logic is testable in isolation. Results land in the snapshot for renderer and architect to consume. Could be exposed as a standalone subcommand later without refactoring. |
| 2 | Check runs by default during inspection | Optional validation steps are discovered after the first failure, not before. Run by default; let users skip it. (Fern) |
| 3 | Warn-and-continue default, not hard fail | The user asked for a Containerfile; the tool should hand them one that works. Unavailable packages are excluded with loud warnings. No modes to choose between. (Fern) |
| 4 | Single flag: `--skip-unavailable` | Matches `dnf`'s own vocabulary — sysadmins already have this term in muscle memory. Describes the user-facing behavior, not implementation internals. Override flags (`--strict`, `--fail-unavailable`, `--keep-unavailable`) were considered and dropped — they create decision points that don't earn their complexity. (Fern) |
| 5 | No replacement map or fix suggestions | "We can't program in advice for every situation." A curated replacement map is maintenance-heavy and one wrong suggestion undermines trust in the whole tool. inspectah reports what doesn't resolve; the user knows their systems. (Mark) |
| 6 | Diagnostics go to stderr, never into the Containerfile | The Containerfile is a build artifact. Diagnostics stay in the diagnostic block. (Fern) |
| 7 | Detect and flag direct-install RPMs as a separate category | Packages installed via `rpm -i` (not through dnf) have no repo metadata. inspectah can't reproduce the install from a repo. This is a harder problem than "unavailable" and requires different user action. (Mark) |
| 8 | Detect package-added repos (e.g., `epel-release`) | Common pattern: installing a package drops repo files into `/etc/yum.repos.d/`. Preflight must install repo-providing packages first, then check availability against the complete repo set. (Mark) |
| 9 | Custom repo configs from the snapshot are mounted into the preflight container | Ensures preflight queries the same repo set the Containerfile will use. `$releasever` resolves correctly inside the target base image container. (Design discussion) |
| 10 | Machine-parseable output via snapshot, not a separate JSON flag | Preflight results are stored in the `InspectionSnapshot`. Architect and fleet tooling already read snapshots. A separate `--output-json` flag is redundant. (Mark) |
| 11 | Architect aggregates preflight data across fleet snapshots | No new flags or CLI surface. Architect reads what's in the snapshots. Fleet-level view surfaces prevalence-sorted unavailable packages, direct-install RPMs, and unreachable repos. (Ember) |
| 12 | Preflight validates the exact install set the renderer will emit (leaf-filtered, include-filtered, safety-filtered) | Prevents diagnostic drift. Shared `resolve_install_set()` function ensures preflight and renderer operate on the same package list. (Design review finding 3) |
| 13 | `unverifiable` is a distinct outcome from `unavailable` | Repo-provider bootstrap failures are "couldn't check" not "checked and missing." Different user action, different rendering behavior (included vs. excluded). (Design review finding 2) |
| 14 | `preflight.status` replaces `null` — four states: `completed`, `partial`, `skipped`, `failed` | Downstream tooling must distinguish intentional opt-out from broken validation. `null` collapsed too many states. (Design review finding 4) |
| 15 | Architect aggregates per base image, not across mixed-base fleets | A package available in F43 repos may be unavailable in F44. Collapsing results across base images produces misleading fleet summaries. (Design review open question) |
| 16 | Snapshot trust model is explicit; environment skew is documented | Preflight mounts active config from the inspected host — same trust boundary as rendering. Entitlement, CDN, and network skew are acknowledged as best-effort limitations. (Design review findings 6, 7) |

---

## Preflight Module

### Location

New module: `src/inspectah/preflight.py`

### Interface

```python
@dataclass
class PreflightResult:
    """Result of package availability check against target repos."""
    status: str                  # "completed", "partial", "skipped", "failed"
    status_reason: str | None    # Why status is not "completed" (human-readable)
    available: list[str]         # Packages confirmed in target repos
    unavailable: list[str]       # Packages not found in any repo
    unverifiable: list[UnverifiablePackage]  # Packages that couldn't be checked
    direct_install: list[str]    # Packages installed via rpm, not from a repo
    repo_unreachable: list[RepoStatus]  # Repos that could not be queried
    base_image: str              # Base image reference used for the check
    repos_queried: list[str]     # Repo IDs successfully queried
    timestamp: str               # ISO-8601 UTC

@dataclass
class UnverifiablePackage:
    name: str
    reason: str                  # e.g., "repo-providing package epel-release unavailable"

@dataclass
class RepoStatus:
    repo_id: str
    repo_name: str
    error: str                   # Why the repo couldn't be queried
    affected_packages: list[str] # Packages only available from this repo (if known)
```

**Status values:**

| Status | Meaning |
|--------|---------|
| `completed` | All packages checked against all configured repos. Results are definitive. **Invariant:** `unverifiable` is empty and `repo_unreachable` is empty. Downstream tooling can trust this state without re-deriving it. |
| `partial` | Some repos were unreachable or some repo-providing packages couldn't be installed. Results are best-effort — `unverifiable` and `repo_unreachable` list what couldn't be checked. **Invariant:** at least one of `unverifiable` or `repo_unreachable` is non-empty. |
| `failed` | Preflight could not run at all (base image not pullable, no container runtime, etc.). `status_reason` explains why. No package-level results. |

When `--skip-unavailable` is set, the snapshot's preflight field uses a distinct representation (see Snapshot Schema Addition) so downstream tooling can distinguish intentional opt-out from failure.

### Mechanism

The preflight module takes a package list, a base image reference, and custom repo configs from the snapshot. It runs a two-phase check inside a temporary container from the target base image.

**Phase 1: Bootstrap repo-providing packages**

During inspection, the RPM inspector detects which repo files on the source system are owned by packages (via `rpm -qf /etc/yum.repos.d/*.repo`). These are tagged as repo-providing packages in the snapshot (e.g., `epel-release`, `rpmfusion-free-release`).

The preflight module installs these packages first in the temporary container, bootstrapping the repos they provide. If a repo-providing package itself is unavailable in the target base image repos, it is flagged and packages from its repo are marked as unverifiable.

**Phase 2: Check availability**

With all repos in place (base image repos + custom repos mounted from snapshot + repos bootstrapped by repo-providing packages), the preflight module runs:

```
dnf repoquery --available <packages>
```

The input list is diffed against the results to identify unavailable packages.

**Custom repo injection:**

Custom repo files and GPG keys from the snapshot are copied into the container's standard paths — matching what the rendered Containerfile does. The renderer emits `COPY config/etc/yum.repos.d/ /etc/yum.repos.d/` (not a subdirectory), so the preflight container must mirror this. DNF loads `.repo` files from `/etc/yum.repos.d/` directly; it does not recurse into subdirectories.

```
podman run \
  -v /path/to/snapshot/config/etc/yum.repos.d/:/etc/yum.repos.d/:Z \
  -v /path/to/snapshot/config/etc/pki/rpm-gpg/:/etc/pki/rpm-gpg/:Z \
  -v /path/to/snapshot/config/etc/dnf/:/etc/dnf/:Z \
  <base> dnf repoquery --available <packages>
```

The `etc/dnf/` mount is required for parity with the renderer, which can emit `COPY config/etc/dnf/ /etc/dnf/`. DNF vars (`/etc/dnf/vars/`), plugin configuration, and module defaults in this tree can affect package resolution. Without it, preflight may disagree with the real build. The same merge semantics apply — base image defaults must not be silently shadowed. If the snapshot has no `etc/dnf/` content, the mount is skipped.

Note: this bind-mount replaces the base image's repo directory entirely. If the base image ships repos in `/etc/yum.repos.d/` that the snapshot does not include, they will be shadowed. The preflight module must merge: copy base image repo files into the snapshot's staging directory before mounting, or use individual file mounts. Implementation should match the renderer's actual `COPY` semantics — the Containerfile layers the snapshot's repo files on top of the base image's, so the preflight must do the same.

This ensures `$releasever` and `$basearch` resolve against the target, not the source. The preflight container sees the same repo set the rendered Containerfile will configure.

**Trust model:** The snapshot's `.repo` files and GPG keys are active configuration from the inspected host — not inert data. This design assumes the inspected host and its snapshot are trusted. Preflight mounts these files into a temporary container and executes `dnf` against them. If the snapshot is compromised, malicious repo configs could point `dnf` at hostile repos. This is acceptable because inspectah already trusts the snapshot for rendering — the Containerfile `COPY`s the same files into the built image.

**Environment skew:** Preflight runs in the current host's network and auth context, which may differ from the build environment. Key skew scenarios:
- **RHEL entitlements:** The preflight host may not have the same RHEL subscription entitlements as the build host. Entitled repos may appear unreachable.
- **CDN vs. internal mirrors:** Production builds may use internal mirrors or Satellite servers that the preflight host can't reach.
- **VPN/network segmentation:** Internal repos may only be accessible from certain networks.
- **Container registry auth:** The base image pull may require credentials the preflight host doesn't have.

Preflight is a best-effort check in the current environment, not a build guarantee. The `status: partial` + `repo_unreachable` mechanisms handle these cases gracefully — the diagnostic block tells the user which repos couldn't be reached, and affected packages are included (not excluded) with a warning. The spec does not attempt to simulate the build environment — it checks what it can and is transparent about what it can't.

### Direct-install RPM detection

During inspection, the RPM inspector checks the `from_repo` metadata for each package (from `rpm -qi` or equivalent). Packages with no repo origin (showing `(none)`, `commandline`, or equivalent) are classified as direct-install RPMs. These are excluded from the `dnf repoquery` check and flagged separately — inspectah cannot reproduce their installation from a repo.

### Input set: preflight checks what the renderer will emit

The preflight module must validate the **exact package set the renderer will emit** in the `dnf install` line — not the raw `packages_added` list. The renderer applies three filters before emitting:

1. **`p.include` filter:** Excludes packages marked as excluded (e.g., by user edits in the architect UI).
2. **Leaf package filter:** When a baseline exists, only leaf (explicitly installed) packages are emitted. Auto/dependency packages are omitted — `dnf` resolves them at install time.
3. **Shell safety filter (`_sanitize_shell_value`):** Packages with unsafe shell characters are silently excluded and replaced with `# FIXME` comments.

The preflight input must be the post-filter list. This means one of:
- The preflight module applies the same filtering logic, or
- The filtering is extracted into a shared function that both preflight and the renderer call.

The second option is preferred — a single `resolve_install_set(snapshot) -> list[str]` function that both the preflight module and the renderer use as their package input. This eliminates drift between what's checked and what's rendered.

### Integration point

The preflight module is called after the RPM inspector has built its full output (including `packages_added`, `leaf_packages`, and the include/exclude state). The call site must have access to the same filtering context the renderer uses. The `PreflightResult` is stored in the `InspectionSnapshot`, making it available downstream to the renderer and architect.

If `--skip-unavailable` is set, the preflight module is not called. The snapshot's preflight field uses the `skipped` representation (see Snapshot Schema Addition), and the renderer includes all packages without availability filtering.

---

## Renderer Behavior

The packages renderer (`renderers/containerfile/packages.py`) consumes preflight results from the snapshot.

### Default behavior (preflight enabled)

1. Unavailable packages are **excluded** from the `dnf install` line
2. Direct-install RPMs are **excluded** from the `dnf install` line
3. A structured diagnostic block is emitted to **stderr**
4. The rendered Containerfile is buildable as-is

### With `--skip-unavailable`

1. All packages are included in the `dnf install` line regardless of availability
2. No diagnostic block is emitted
3. The user is responsible for ensuring repos are configured at build time

### Diagnostic block format

Emitted to stderr, human-readable, consolidated (not scattered through output):

```
=== Package Availability Report ===

NOT IN ANY REPO (installed directly via rpm — cannot be installed from repos):
  custom-agent-1.2.3
  internal-monitoring-4.0

UNAVAILABLE in target repos:
  mcelog
  compat-libstdc++-33
  legacy-tool-2.1

UNVERIFIABLE (could not check — included in Containerfile but not validated):
  some-epel-pkg (repo-providing package epel-release unavailable)

REPO UNREACHABLE (could not verify — packages from these repos not validated):
  internal-mirror (error: connection timed out)
    Packages from this repo: internal-app, internal-lib

3 packages excluded from Containerfile.
1 package unverifiable (included but not validated).
2 packages from unreachable repos (included but not validated).
Preflight status: PARTIAL — use --skip-unavailable to skip all checks.
===
```

**Four categories:**

| Category | Meaning | Included in Containerfile? | User action |
|----------|---------|---------------------------|-------------|
| NOT IN ANY REPO | Installed via `rpm -i`, no repo metadata | No | Find a repo, or bundle the RPM and `COPY` + `rpm -i` in the Containerfile |
| UNAVAILABLE | Not found in any configured target repo | No | Determine if the package is still needed; find in another repo or remove |
| UNVERIFIABLE | Repo-provider bootstrap failed; couldn't check | Yes | Ensure the repo-providing package and its repo are available at build time |
| REPO UNREACHABLE | Repo exists but could not be queried | Yes | Check network/auth; packages from this repo are included but unverified |

**UNVERIFIABLE** packages (from failed repo-provider bootstrap) are included in the Containerfile but listed in the diagnostic block with their reason. They are distinct from UNAVAILABLE — the tool couldn't check, not "checked and missing."

**REPO UNREACHABLE** packages are also included in the Containerfile. The preflight can't confirm or deny availability, so it includes them and warns. This avoids false positives when the build environment has different network access than the preflight host.

**Completeness signal:** The diagnostic block footer uses the exact format shown in the sample block above. The canonical footer format is:

```
<N> packages excluded from Containerfile.
<N> package(s) unverifiable (included but not validated).
<N> packages from unreachable repos (included but not validated).
Preflight status: <STATUS> — use --skip-unavailable to skip all checks.
```

Lines with zero counts are omitted. Machine-readable completeness is in `snapshot.json` via the `preflight.status` field. Downstream tooling should not treat `exit 0 + rendered Containerfile` as "all packages validated" — it must check `preflight.status` to distinguish `completed` from `partial`, `failed`, or `skipped`.

---

## CLI Interface

### Flags

| Flag | Effect |
|------|--------|
| `--skip-unavailable` | Skip the preflight check entirely. All packages included in the Containerfile without validation. No diagnostic block. |

No other flags. The preflight check runs by default during `inspectah inspect`. The default behavior (warn-and-continue) is the only mode.

### Exit codes

The preflight check does **not** change the exit code. inspectah exits 0 even if unavailable packages are found — the Containerfile was rendered successfully (with those packages excluded). The diagnostic block on stderr provides the signal.

If the user needs to detect unavailable packages programmatically, they read the `preflight` field in `snapshot.json`.

---

## Snapshot Schema Addition

The `InspectionSnapshot` gains a `preflight` field. The field is always present (never `null`) so downstream tooling can always determine the preflight state without guessing.

### Completed or partial check

```json
{
  "preflight": {
    "status": "completed",
    "status_reason": null,
    "available": ["pkg-a", "pkg-b"],
    "unavailable": ["mcelog", "compat-lib"],
    "unverifiable": [],
    "direct_install": ["custom-agent-1.2.3"],
    "repo_unreachable": [],
    "base_image": "quay.io/fedora/fedora-bootc:44",
    "repos_queried": ["fedora", "updates", "epel"],
    "timestamp": "2026-04-09T17:00:00Z"
  }
}
```

### Partial check (repo-provider bootstrap failure)

```json
{
  "preflight": {
    "status": "partial",
    "status_reason": "repo-providing package epel-release unavailable; EPEL packages unverifiable",
    "available": ["pkg-a"],
    "unavailable": ["mcelog"],
    "unverifiable": [
      {"name": "some-epel-pkg", "reason": "repo-providing package epel-release unavailable"}
    ],
    "direct_install": [],
    "repo_unreachable": [],
    "base_image": "quay.io/fedora/fedora-bootc:44",
    "repos_queried": ["fedora", "updates"],
    "timestamp": "2026-04-09T17:00:00Z"
  }
}
```

### Skipped by user

```json
{
  "preflight": {
    "status": "skipped",
    "status_reason": "user passed --skip-unavailable"
  }
}
```

### Failed (could not run)

```json
{
  "preflight": {
    "status": "failed",
    "status_reason": "base image quay.io/fedora/fedora-bootc:44 could not be pulled"
  }
}
```

**Status discrimination:** Downstream tooling uses `status` to determine how to interpret the field:
- `completed` — results are definitive, safe to act on.
- `partial` — results are best-effort. Check `unverifiable` and `repo_unreachable` for gaps.
- `skipped` — user opted out. No package-level data. Do not treat as "all packages verified."
- `failed` — preflight broke. No package-level data. Equivalent to skipped for rendering, but architect should flag these systems.

---

## Architect / Fleet Aggregation

The architect web UI reads `InspectionSnapshot` data across multiple systems. Since preflight results are stored in each snapshot, fleet aggregation requires no new data collection.

### Fleet-level views

- **Unavailable packages by prevalence:** "These 8 packages are unavailable, affecting N of M systems." Sorted by prevalence so the biggest migration decisions surface first.
- **Direct-install RPMs by prevalence:** Same aggregation. If 30 of 50 systems have `custom-agent` installed via `rpm -i`, that's one fleet-wide decision, not 30 individual ones.
- **Unverifiable packages by prevalence:** Same aggregation, distinct from unavailable. Shows which repo-provider gaps affect the most systems.
- **Unreachable repos:** Deduplicated per base image group, not per-system. If `internal-mirror` is unreachable for all F44-targeting systems, that's one environment issue for that base image group, not N individual problems.
- **Layer decomposition awareness:** When architect recommends base vs. derived layer splits, it factors in availability. No point putting an unavailable package in the shared base layer. Unverifiable packages should be flagged if placed in a base layer — they're higher risk for a shared layer.

### Aggregation by base image

Mixed-base fleets (e.g., some systems targeting `fedora-bootc:43`, others targeting `fedora-bootc:44`) must not collapse availability results across different base images. A package available in F43 repos may be unavailable in F44. Architect aggregates preflight data **per base image**, so each target environment gets its own availability picture. Fleet-wide summaries note which base image each result applies to.

### Preflight status in fleet view

Architect uses the `preflight.status` field to categorize systems:
- `completed` — full results, included in aggregation.
- `partial` — included in aggregation with a caveat flag.
- `skipped` — excluded from availability aggregation, noted as "no preflight data."
- `failed` — excluded from availability aggregation, flagged as "preflight failed."

The fleet view shows the count of systems in each status so the user knows the completeness of the aggregate picture.

---

## Error Handling

| Scenario | Status | Behavior |
|----------|--------|----------|
| Base image not pullable | `failed` | Warns that preflight could not run, continues without it. Renderer includes all packages. `status_reason` records the pull error. |
| Container runtime not available | `failed` | Same as above. |
| `dnf repoquery` times out | `partial` | Warns about timeout, records unreachable repos in `repo_unreachable`, continues with partial results. Affected packages are included in Containerfile. |
| Repo-providing package unavailable | `partial` | Flags the package. Packages from its repo go into `unverifiable` with reason. Included in Containerfile but not validated. |
| Individual repo unreachable | `partial` | Records in `repo_unreachable`. Other repos still checked normally. |
| All repos unreachable | `failed` | No meaningful results. Equivalent to `--skip-unavailable` in rendering behavior, but `status: failed` in the snapshot so architect can distinguish from intentional skip. |

The preflight check is best-effort. It never blocks rendering. A failed preflight is strictly better than no preflight — even partial results help.

---

## What This Spec Does NOT Cover

- **Replacement suggestions or mapping.** inspectah does not suggest what to use instead of an unavailable package. The user knows their systems.
- **Version compatibility checking.** The preflight checks name-level availability, not whether a specific version exists. Version lock conflicts are a separate concern.
- **Multi-step Containerfile simulation.** Repos added via `RUN` commands during the build (e.g., `dnf install epel-release && dnf install epel-pkg`) are handled via the repo-providing package detection, but arbitrary Containerfile logic is out of scope.
- **Standalone `inspectah preflight` subcommand.** The module supports this structurally, but v1 exposes it only through `inspectah inspect`. A subcommand can be added later if there's demand.

---

## Implementation Notes

These are not design decisions — they are hygiene notes for whoever implements this spec.

- **stderr is not a machine contract.** The diagnostic block format is for human consumption. Implementation may evolve wording, spacing, and ordering without a breaking change. Machine consumers use `snapshot.json`'s `preflight` field exclusively.
- **Merge parity for partial `etc/dnf/` trees.** When the snapshot has a partial `etc/dnf/` tree (e.g., `vars/` but no `dnf.conf`), the implementation must merge with the base image's existing `etc/dnf/` contents, not replace the entire directory. Same merge semantics as `etc/yum.repos.d/`.
- **argv-based subprocess invocation.** All `podman` and `dnf` commands must use argv lists (`subprocess.run(["podman", "run", ...])`) — never shell strings. This avoids injection risks from package names or repo paths containing shell metacharacters.

---
