# Subscription Preservation and Image Build

**Date:** 2026-05-29
**Status:** Proposed

## Overview

Two features that close the end-to-end workflow from scanning a RHEL host to
producing a bootc image on a non-RHEL machine:

1. **`--preserve-subscription`** on `inspectah scan` — collects RHEL entitlement
   certs from scanned hosts so they can be mounted during `podman build` on
   non-RHEL hosts (Mac, Fedora). On RHEL, subscription passes through
   automatically via `containers/common`. On non-RHEL hosts, explicit `-v` mounts
   are required.

   **Platform support (v1):** Linux and macOS (via podman machine with
   shared-path extraction). Windows is a future target, not validated for v1 —
   expected to need a similar VM/shared-path model but not yet tested.

2. **`inspectah build`** — extracts an inspectah tarball and runs `podman build`,
   automatically mounting subscription certs when present. Closes the last-mile
   gap between "I have a Containerfile" and "I have a bootc image."

A shared change — renaming `--acknowledge-sensitive` to `--ack-sensitive` — applies
across both features and the existing sensitive-data flags.

## Decisions Log

1. **Collect and mount `redhat.repo` for non-RHEL builds.** `redhat.repo` is
   a `%ghost` file in the `subscription-manager` RPM — not present in
   unregistered base images. Without it, `dnf` has no repo definitions even
   with valid entitlement certs mounted. Collected from the scanned host and
   mounted at `/run/secrets/redhat.repo` alongside entitlement certs and
   rhsm config. (2026-05-29)

2. **Trust the tarball at build time.** inspectah builds self-generated
   tarballs. The user who scanned is the user who builds. No build-time
   consent gate beyond scan-time `--ack-sensitive` — the user chose to
   build from this tarball, and that IS the consent decision. Archive
   safety checks (path traversal, symlink escape, hard links, duplicate
   paths, file-type replacement, special file types) are defense-in-depth
   against malformed archives, not a trust boundary. This is an
   accepted-risk product decision for a pre-1.0 migration tool — the trust
   model will be tightened based on real-world usage patterns if needed.
   (2026-05-29)

3. **Fleet subscription: sufficient-for-target.** The winning cert set must
   work on the target base image. For standard RHEL subscriptions and SCA,
   any valid cert set satisfies this — entitlement certs grant access to the
   same content pool regardless of which host they came from. Edge cases
   (EUS, specialized entitlements) self-correct at pull time: the build
   fails early with a clear error if the certs don't cover the base image's
   repos. Fleet merge picks the latest-expiry cert set from any contributing
   host. No cross-org or cross-version warnings — inspectah has no reliable
   way to determine org membership from cert metadata, and the failure mode
   is self-correcting. (2026-05-29)

4. **RHEL ambient preference.** On RHEL hosts, ambient subscription
   pass-through is preferred over tarball-carried subscription data by
   default. If the tarball has certs and the host also has pass-through,
   pass-through wins. This is an explicit product choice — the host's
   live subscription is fresher and managed by subscription-manager, making
   it more reliable than a potentially stale snapshot. (2026-05-29)

5. **`--ack-sensitive` is the single consent gate.** `--ack-sensitive` is
   required at scan time when any `--preserve-*` flag is set. The flag's
   meaning is: "I understand this tarball will contain sensitive material."
   This single gate covers scan, fleet merge (which inherits
   `sensitive_snapshot` from contributing hosts), and export. There is no
   separate build-time gate. (2026-05-29)

## Context

### RHEL version compatibility

The subscription directory structure, file naming conventions, `rhsm.conf`
format, and Podman mount mechanism are identical across RHEL 9 and RHEL 10.
Verified on real RHEL 9.7 and 10.2 hosts (2026-05-28) — both present the
same paths under `/etc/pki/entitlement/`, `/etc/rhsm/`, and `/etc/rhsm/ca/`.
The `subscription-manager` codebase hardcodes these paths as constants,
and the `containers/common` mounting logic
(`pkg/subscriptions/subscriptions.go`) is unchanged between versions.
No version-specific handling is needed in inspectah.

### SCA vs traditional entitlement

Red Hat offers two entitlement models. **Traditional entitlement** uses
multiple certificate pairs in `/etc/pki/entitlement/`, one per attached
subscription, each encoding specific product IDs and content sets.
**Simple Content Access (SCA)** — now the default for most Red Hat orgs —
uses a single content access certificate granting access to all content
the org is entitled to, still placed in `/etc/pki/entitlement/` with the
same `<serial>.pem` + `<serial>-key.pem` naming convention. Confirmed on
real RHEL 9.7 and 10.2 hosts: both show a single entitlement cert pair
(one serial number), consistent with SCA mode.

The directory structure is identical under both models. The collect-and-mount
pattern implemented by this feature is transparent to SCA vs traditional —
inspectah does not need to distinguish between them.

### How Podman auto-mounting works on RHEL hosts

On RHEL, Podman automatically mounts subscription material into build
containers via a chain of indirection:

1. `/usr/share/containers/mounts.conf` maps
   `/usr/share/rhel/secrets` → `/run/secrets`
2. `/usr/share/rhel/secrets/` contains symlinks:
   - `etc-pki-entitlement` → `/etc/pki/entitlement`
   - `rhsm` → `/etc/rhsm`
   - `redhat.repo` → `/etc/yum.repos.d/redhat.repo`
3. Podman reads `mounts.conf`, resolves symlinks, bind-mounts into
   container at `/run/secrets/etc-pki-entitlement/`,
   `/run/secrets/rhsm/`, `/run/secrets/redhat.repo`

On non-RHEL hosts, none of this exists — explicit `-v` mounts are required,
which is what `inspectah build` provides.

### Current behavior

`inspectah scan` already explicitly *excludes* subscription paths from
config collection via `UNOWNED_EXCLUDE_GLOBS`:
- `/etc/pki/entitlement/*`
- `/etc/pki/consumer/*`
- `/etc/pki/product-default/*`

The `--preserve-subscription` flag adds a dedicated `SubscriptionInspector`
that reads entitlement, rhsm, and CA cert paths independently of the config
inspector. Consumer and product-default paths remain excluded from both
inspectors — the consumer cert is only parsed for org metadata, not
collected.

### Why `redhat.repo` needs special handling

`redhat.repo` is a `%ghost` file in the `subscription-manager` RPM — the RPM
declares ownership of `/etc/yum.repos.d/redhat.repo` but does not ship the
file in its payload. It is created at runtime by `subscription-manager
register`, which populates it with RHEL repo definitions derived from the
host's entitlements.

Unregistered base images do not contain `redhat.repo`. The dnf
`subscription-manager` plugin creates an empty, header-only version on first
run, but this version contains no repo definitions — `dnf install` still
fails. Only the version created by `subscription-manager register` on a
registered host contains the actual repo URLs and GPG key references needed
for package installation.

On RHEL build hosts, Podman pass-through handles this transparently — the
host's real `redhat.repo` is mounted via the `/usr/share/rhel/secrets/`
symlink chain. On non-RHEL build hosts (Mac, Fedora), there is no
pass-through, and the base image has no `redhat.repo`, so it must be
collected from the scanned host and mounted explicitly.

### Target paths

The following paths are collected when `--preserve-subscription` is set:

| Path | Content | Needed for build | Always present |
|------|---------|-----------------|----------------|
| `/etc/pki/entitlement/*.pem` | Entitlement cert + key pairs | YES — authenticates to RHEL CDN | Yes |
| `/etc/rhsm/rhsm.conf` | RHSM server config | YES — TLS endpoint config | Yes |
| `/etc/rhsm/ca/*.pem` | CDN CA certs | YES — TLS verification | Yes |
| `/etc/yum.repos.d/redhat.repo` | RHEL repo definitions (generated by subscription-manager) | YES — repo definitions for dnf | Yes (on registered hosts) |
| `/etc/pki/consumer/cert.pem` | Consumer identity (parsed for org metadata only — cert not collected) | NO | Yes |

`redhat.repo` is a `%ghost` file in the `subscription-manager` RPM — the RPM
owns the path but does not ship the file. It is created at runtime by
`subscription-manager register`. Unregistered base images do not contain it.
On RHEL build hosts, pass-through mounts the host's populated copy. On
non-RHEL build hosts, nobody provides it — without repo definitions, `dnf
install` fails even with valid entitlement certs mounted. It must be
collected from the scanned host and mounted for non-RHEL builds.

Missing optional paths are silently skipped. Permission-denied on accessible
directories produces a warning, not a silent skip.

---

## Feature 1: `--preserve-subscription`

### Data flow

```
inspectah scan --preserve-subscription --ack-sensitive host1 host2
  → SSH to each host
  → SubscriptionInspector reads /etc/pki/entitlement/*, /etc/rhsm/*, etc.
  → Base64-encode file contents → SubscriptionSection in snapshot
  → Parse X.509 certs for expiry dates
  → Stage subscription files in tarball under subscription/
  → Containerfile renderer adds mount instructions as comments
```

### New inspector: `SubscriptionInspector`

A standalone inspector — no dependency on other inspectors.

**Collection logic:**

1. Read `/etc/pki/entitlement/` — collect all `*.pem` files (cert + key pairs)
2. Read `/etc/rhsm/rhsm.conf`
3. Read `/etc/rhsm/ca/` — collect all `*.pem` files
4. Read `/etc/yum.repos.d/redhat.repo` (if present)
5. Parse X.509 certs from step 1 to extract `earliest_expiry`
6. Parse consumer cert at `/etc/pki/consumer/cert.pem` for reference metadata
   (org ID from Subject O, system UUID from Subject CN, RHSM server from
   Issuer). Do not collect the cert file itself. If the consumer cert is
   unreadable or missing, these fields are simply `None` — no warning needed
   (not build-required)
7. Evaluate bundle completeness (see below) — set `incomplete: true` with
   a specific warning if any required component is missing

**Symlink handling during collection:**

Only follow symlinks within the known subscription paths
(`/etc/pki/entitlement/`, `/etc/rhsm/`, `/etc/yum.repos.d/redhat.repo`).
Reject symlinks that resolve outside these directories. Subscription paths
are well-known, RPM-managed locations. Symlinks within them are created by
subscription-manager and are trusted. Symlinks resolving outside the
subscription tree are rejected with a warning naming the path and its
target.

**Build-usable bundle definition:**

A complete, build-usable subscription bundle requires all of:

1. At least one entitlement cert + its matching key (same serial number:
   `<serial>.pem` + `<serial>-key.pem`)
2. `rhsm.conf` from `/etc/rhsm/`
3. At least one CA cert from `/etc/rhsm/ca/`
4. `redhat.repo` from `/etc/yum.repos.d/` — repo definitions for dnf

If any of these are missing, the bundle is `incomplete: true` with a warning
naming the missing component(s):
- `[warn] Incomplete subscription bundle: no matching key for entitlement cert <serial>.pem`
- `[warn] Incomplete subscription bundle: missing /etc/rhsm/rhsm.conf`
- `[warn] Incomplete subscription bundle: no CA certs found in /etc/rhsm/ca/`
- `[warn] Incomplete subscription bundle: missing /etc/yum.repos.d/redhat.repo — dnf will have no repo definitions on non-RHEL build hosts`

Consumer certs and product-default certs are not collected. Org metadata
is parsed from the consumer cert at scan time (see `org_id`, `system_uuid`,
`rhsm_server` fields) but the cert file itself is not included in the
tarball.

**Error handling:**
- Permission-denied on existing directories: warning with path
- Invalid/corrupted PEM (truncated, DER-encoded, empty): warning with
  filename, file skipped, not a panic
- Symlinks within subscription paths: follow valid symlinks, warn on
  dangling, handle loops safely
- Symlinks resolving outside subscription paths: rejected with warning
  naming the path and its target
- Consumer cert unreadable/missing: org metadata fields set to `None`,
  no warning (not build-required)
- Files > 1 MB: reject with warning (safety valve; real PEM certs are < 10 KB)

### Schema changes

New type `SubscriptionSection`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionFile {
    pub path: String,
    pub content: String, // base64-encoded
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_expiry: Option<String>, // ISO 8601
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionSection {
    pub entitlement_certs: Vec<SubscriptionFile>,
    pub ca_certs: Vec<SubscriptionFile>,
    pub config_files: Vec<SubscriptionFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earliest_expiry: Option<String>, // ISO 8601, parsed from X.509
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub incomplete: bool, // true when any of: entitlement cert+key, rhsm.conf, CA cert, redhat.repo is missing
    /// Org ID parsed from consumer cert subject (if available)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
    /// System UUID parsed from consumer cert CN (if available)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_uuid: Option<String>,
    /// RHSM server (cdn vs Satellite) parsed from consumer cert issuer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhsm_server: Option<String>,
}
```

New fields on `InspectionSnapshot`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub subscription: Option<SubscriptionSection>,

#[serde(default, skip_serializing_if = "crate::is_false")]
pub preserved_subscription: bool,
```

`sensitive_snapshot` is OR'd:
`preserve_password_hashes || preserve_ssh_keys || preserve_subscription`

Schema version: bump both `SCHEMA_VERSION` and `MIN_SCHEMA` to 18 (clean
break). Old v17 snapshots are rejected with a "re-scan required" message.
Pre-1.0 tool, Rust rewrite just shipped — old snapshots are re-scanned, not
migrated. The `subscription` field is still `Option<SubscriptionSection>`
(it is `None` when `--preserve-subscription` is not used), but no backward
compatibility with v17 loaders is maintained.

### CLI interface

#### New flag

```rust
#[arg(long)]
pub preserve_subscription: bool,
```

#### Interaction with other flags

- `--preserve-subscription` requires `--ack-sensitive` (same gate as
  `--preserve-password-hashes` and `--preserve-ssh-keys`)
- `--ack-sensitive` missing when any `--preserve-*` flag is set produces a
  dynamic error message listing which sensitive data is actually present
  (subscription certs, password hashes, SSH keys), not hardcoded text
- `--preserve-subscription` without `--ack-sensitive` is a hard error

#### CLI output

When `--preserve-subscription` is active:

```
[info] Collecting subscription material...
[info] Found N entitlement cert pair(s), expiry: YYYY-MM-DD
[warn] Entitlement cert expires in N days (YYYY-MM-DD): subscription/entitlement/<serial>.pem
```

When subscription paths are empty or inaccessible:

```
[warn] --preserve-subscription set but no entitlement certs found at /etc/pki/entitlement/
```

### Single-host behavior

**Tarball layout:**

```
hostname-timestamp/
├── Containerfile
├── subscription/
│   ├── entitlement/
│   │   ├── 1663571108710627452.pem
│   │   └── 1663571108710627452-key.pem
│   ├── redhat.repo
│   └── rhsm/
│       ├── rhsm.conf
│       └── ca/
│           ├── redhat-entitlement-authority.pem
│           └── redhat-uep.pem
├── config/
│   └── ...
└── ...
```

The `rhsm/` subtree mirrors the host layout so a single `-v` mount provides
config + CA certs. Only build-required files are included — consumer certs
and product-default certs are not collected.

### Containerfile integration

When subscription data is present, the Containerfile renderer adds mount
instructions as comments in a dedicated block:

```dockerfile
# === RHEL Subscription ===
# This build requires RHEL entitlement certificates for repo access.
# Mount them at build time (do NOT copy into the image):
#
#   podman build \
#     -v ./subscription/entitlement:/run/secrets/etc-pki-entitlement:z \
#     -v ./subscription/rhsm:/run/secrets/rhsm:z \
#     -v ./subscription/redhat.repo:/run/secrets/redhat.repo:z \
#     -f Containerfile .
#
# Certificate expiry: 2026-08-15
```

When subscription data is not present, this block is omitted entirely.

**What is NOT generated:**
- No `--mount=type=secret` instructions (Podman-specific, less portable)
- No `COPY` of cert files into the image (security anti-pattern)
- No `RUN subscription-manager` commands
- No `ENV` variables related to subscription

### Fleet behavior

#### Deduplication strategy

Fleet merge picks the host with the latest `earliest_expiry` as the
canonical subscription source. One cert set in the merged tarball.
Deterministic hostname tiebreak on same expiry. Hosts without subscription
data are skipped. The fleet aggregate tarball is structurally identical to
a single-host tarball — always one `subscription/` directory.

**Why any valid cert set works:** The winning cert set must work on the
target base image. For standard RHEL subscriptions and SCA, any valid cert
set satisfies this — entitlement certs grant access to the same content
pool regardless of which host they came from. Edge cases (EUS, specialized
entitlements) self-correct at pull time: the build fails early with a clear
error if the certs don't cover the base image's repos. Picking the
latest-expiry cert set is sufficient for the target without claiming
universal fungibility.

#### Fleet merge adapter

The subscription merge adapter:

1. Collects `SubscriptionSection` from all hosts
2. Filters to hosts where `subscription.is_some()` and `!incomplete`
3. Sorts by `earliest_expiry` descending (latest first)
4. Tiebreaks by hostname (lexicographic ascending)
5. Picks the winner, stages its subscription files in the merged tarball
6. Records the source hostname in fleet metadata

Fleet `sensitive_snapshot` / `preserved_subscription` are boolean OR across
all hosts. Fleet export requires `--ack-sensitive` when any host
contributed subscription data.

### Security considerations

#### Trust boundary

inspectah builds self-generated tarballs. The user who scanned is the user
who builds. `inspectah build` trusts the tarball contents — the user chose
to build from this tarball, and that is the trust decision. This is an
accepted-risk product decision for a pre-1.0 migration tool; the trust
model will be tightened based on real-world usage patterns if needed.

The trust boundary is the archive itself. Safety measures enforced during
extraction are:

- Reject entries with `../` path components (path traversal)
- Reject absolute paths in archive entries
- Reject symlinks pointing outside the extraction root
- Reject hard links pointing outside the extraction root
- Reject duplicate paths (same path appearing more than once in the archive)
- Reject file-type replacement (an entry whose path collides with an
  already-extracted entry of a different type — e.g., a regular file
  followed by a symlink with the same name)
- Reject special file types (device nodes, block devices, FIFOs, sockets)
- All extraction constrained to a private, per-build directory

These checks are the testable contract for archive safety. No additional
secret-management layer exists beyond the scan-time `--ack-sensitive` gate.

#### Threat model

Entitlement certs grant access to RHEL content repos — exposure means
unauthorized package downloads, not system compromise. Consumer certs are
not collected (org metadata is parsed at scan time but the cert file is not
included in the tarball). CA certs and `rhsm.conf` are not secrets.

Risk is bounded: certs expire (28 days to 1 year depending on subscription
type and SCA mode), and access is limited to downloading RPMs the org is
already entitled to.

#### Redaction engine interaction

`SubscriptionSection` is exempt from pattern-based redaction. When
`preserved_subscription` is true, the redaction engine skips the
subscription section. When false, the section does not exist (`None`),
nothing to redact.

#### Export gating

`--ack-sensitive` is the single consent gate (see Decisions Log item 5).
It is required at scan time when any `--preserve-*` flag is set, and its
authority carries through to fleet merge and export — no separate build-time
or export-time gate. The dynamic error message enumerates which sensitive
data types are present so the user knows what they are acknowledging.

#### `secrets-review.md` integration

When subscription data is present, `secrets-review.md` in the tarball gets
a subscription entry:

```markdown
## Subscription Material (preserved by --preserve-subscription)

| Type | Count | Expiry | Paths |
|------|-------|--------|-------|
| Entitlement certs | 2 | 2026-08-15 | subscription/entitlement/*.pem |
| CA certs | 2 | — | subscription/rhsm/ca/*.pem |
| Config files | 1 | — | subscription/rhsm/rhsm.conf |
| Repo definitions | 1 | — | subscription/redhat.repo |
```

### Cert expiry handling

- Parse X.509 expiry at collection time using `x509-parser`
- Warn at 14 days before expiry:
  `[warn] Entitlement cert expires in N days (YYYY-MM-DD): subscription/entitlement/<serial>.pem`
- Warn if already expired:
  `[warn] Entitlement cert expired (YYYY-MM-DD): subscription/entitlement/<serial>.pem — build will proceed but RHEL repos may reject expired credentials.`
- Warnings, not hard errors — build may still succeed
- Typical cert validity: 28 days to 1 year depending on subscription type
  and SCA mode

---

## Feature 2: `inspectah build`

### CLI interface

```
inspectah build <tarball> -t <name:tag> [--dry-run] [--keep-context] [-- podman-args...]
```

| Flag | Required | Description |
|------|----------|-------------|
| `<tarball>` | yes | Positional. Path to scan/fleet output `.tar.gz` |
| `-t, --tag` | yes | Image name and tag. Full `name:tag` required — bare names rejected |
| `--dry-run` | no | Emit the `podman build` command to stdout, don't execute. Leaves extracted context in place |
| `--keep-context` | no | Don't clean up the extracted build directory after build |
| `-- [args]` | no | Pass-through to `podman build` directly |

**Tag validation:** `-t` must be `name:tag` format. Bare names (defaulting
to `:latest`) are rejected. This is enforced by inspectah, not podman.

### Build tool

Podman only for v1. The backend is pluggable in the type system (trait or
enum) but only podman ships. No buildah fallback. Document that `--dry-run`
output can be adapted for `buildah bud` by users who need it.

### Data flow

```
inspectah build scan.tar.gz -t myimage:v1
  → Extract tarball to platform-native cache directory
  → Preflight checks:
      - podman in PATH? (exit 127 if not)
      - Containerfile in extracted dir? (exit 1 if not)
      - RHEL pass-through available? (check /usr/share/rhel/secrets/etc-pki-entitlement)
      - subscription/ directory present? (note if yes, notice if no — suppressed on RHEL)
      - cert expiry check if subscription present and not on RHEL (warn at 14 days)
  → Construct podman build command:
      cd <extracted-dir>
      podman build \
        -t myimage:v1 \
        [-v ./subscription/entitlement:/run/secrets/etc-pki-entitlement:z \]  # non-RHEL only
        [-v ./subscription/rhsm:/run/secrets/rhsm:z \]                       # non-RHEL only
        [-v ./subscription/redhat.repo:/run/secrets/redhat.repo:z \]         # non-RHEL only
        -f Containerfile \
        .
  → Print notices to stderr
  → Execute podman, stream output directly (no interleaving)
  → On completion: result to stderr
  → Clean up extracted directory (unless --keep-context)
```

### Subscription cert mounting

- Only mount material needed for build: entitlement certs, rhsm (config + CA
  certs), and `redhat.repo`
- Do NOT mount consumer certs or product-default certs — not needed for
  `podman build`
- Mount targets match `containers/common` convention:
  `/run/secrets/etc-pki-entitlement`, `/run/secrets/rhsm`, and
  `/run/secrets/redhat.repo`
- `:z` SELinux suffix on all mounts — harmless no-op on non-SELinux
  platforms (macOS)

### RHEL host detection (subscription pass-through)

On RHEL hosts, `containers/common` automatically mounts the host's
subscription material into build containers via `mounts.conf` and the
symlinks in `/usr/share/rhel/secrets/`. This pass-through makes explicit
`-v` mounts unnecessary and potentially redundant. Pass-through handles
`redhat.repo` via the host's real file — `subscription-manager register`
creates it at registration time (the `%ghost` mechanism means the RPM owns
the path but doesn't ship the file), and pass-through mounts the host's
populated copy into the build container.

On RHEL hosts, ambient subscription pass-through is preferred over
tarball-carried subscription data by default. If the tarball has certs and
the host also has pass-through, pass-through wins — the host's live
subscription is fresher and managed by subscription-manager.

`inspectah build` detects RHEL pass-through by checking for the existence
of `/usr/share/rhel/secrets/etc-pki-entitlement`. When present:

- **Skip subscription `-v` mounts entirely** — let pass-through handle it
- **Suppress the "no subscription data" notice** — irrelevant on RHEL
- **No warning, no special messaging** — it just works

The `--dry-run` output reflects this: on RHEL, the emitted command has no
`-v` flags for subscription material. On non-RHEL hosts (macOS, non-RHEL
Linux), the `-v` flags are included.

This means `inspectah build` works correctly regardless of whether the
tarball was scanned with `--preserve-subscription` or not, as long as the
build host is RHEL. The feature's value is specifically for non-RHEL build
hosts where pass-through is unavailable.

### Extraction path

Platform-native cache directory, resolved at runtime using the `dirs` crate:

- **Linux:** `$XDG_CACHE_HOME/inspectah/builds/` (defaults to
  `~/.cache/inspectah/builds/`)
- **macOS:** `~/Library/Caches/inspectah/builds/` or
  `~/.cache/inspectah/builds/`
- **Windows (future):** `%LOCALAPPDATA%\inspectah\builds\` — intended path,
  not validated for v1

On platforms where podman runs in a VM (macOS, and Windows when supported),
extraction must be within a VM-shared path. The platform cache directory
(inside the home dir) satisfies this by default.

### Consent model

No re-acknowledgment at build time. The consent decision was made at scan
time. Build-time gets:

- Informational notice (stderr):
  `note: mounting subscription entitlements for build`
- If no subscription data:
  `note: no subscription data in tarball. If the Containerfile installs RHEL packages, re-scan with --preserve-subscription to include entitlement certs.`

### `--dry-run` behavior

- Extracts the tarball (so paths are real and copy-pasteable)
- Emits the exact `podman build` command to stdout as a `cd` + multi-line
  command with `\` continuations
- Notices go to stderr (so `2>/dev/null` gives a clean, pipeable command)
- Does NOT clean up the extracted directory
- Non-zero exit if preflight detects problems (missing Containerfile, no podman)
- Expired cert warnings still appear in dry-run mode

**With subscription:**

```
cd /home/user/.cache/inspectah/builds/abc123
podman build \
  -t myimage:v1 \
  -v ./subscription/entitlement:/run/secrets/etc-pki-entitlement:z \
  -v ./subscription/rhsm:/run/secrets/rhsm:z \
  -v ./subscription/redhat.repo:/run/secrets/redhat.repo:z \
  -f Containerfile \
  .
```

**Without subscription:**

```
cd /home/user/.cache/inspectah/builds/abc123
podman build \
  -t myimage:v1 \
  -f Containerfile \
  .
```

### Passthrough args

When the user passes `-- --no-cache --build-arg FOO=bar`, these are appended
to the podman command. If passthrough includes `-t`, emit a notice to stderr:
`note: -t in passthrough args will add a second image tag in podman.`

### Build output handling

1. **Pre-build:** inspectah notices to stderr (subscription mounting, build
   context path, cert warnings)
2. **During build:** podman's stdout/stderr streams through directly, no
   interleaving from inspectah
3. **Post-build:** `build complete: myimage:v1 (sha256:...)` or
   `error: podman build failed (exit code N)`

### Cleanup

Extracted directory cleaned up on both success and failure, unless
`--keep-context`. Use Rust's `Drop` guard or `tempfile` crate for reliable
cleanup on panic/error paths.

### Error states

| Condition | Message | Exit |
|-----------|---------|------|
| No podman in PATH | `error: podman not found in PATH` | 127 |
| No Containerfile in tarball | `error: no Containerfile found in tarball 'scan.tar.gz'` | 1 |
| Missing `-t` | `error: required flag '--tag' (-t) not provided` | 1 |
| Bare name without tag | `error: tag must include a version: 'myimage:v1', not 'myimage'` | 1 |
| Expired certs | `[warn] Entitlement cert expired (DATE): path — build will proceed` | 0 (unless build fails) |
| Build fails | `error: podman build failed (exit code N)` + hint: `re-run with --no-cache to retry without layer cache` | N |
| Path traversal in tarball | `error: tarball contains unsafe path: <path>` | 1 |
| Absolute path in tarball | `error: tarball contains absolute path: <path>` | 1 |
| Symlink escape in tarball | `error: tarball contains symlink escaping extraction root: <path>` | 1 |
| Hard link escape in tarball | `error: tarball contains hard link escaping extraction root: <path>` | 1 |
| Duplicate path in tarball | `error: tarball contains duplicate path: <path>` | 1 |
| File-type replacement in tarball | `error: tarball contains conflicting entry types for path: <path>` | 1 |
| Special file in tarball | `error: tarball contains unsupported file type: <path>` | 1 |

---

## Shared: `--ack-sensitive` Rename

Rename `--acknowledge-sensitive` to `--ack-sensitive` across scan, fleet,
and web API:

- **CLI:** clap `visible_alias` keeps old name working
- **Web API:** accept both `x-acknowledge-sensitive` and `x-ack-sensitive`
  HTTP headers
- **CORS config** must expose both header names
- **Structural test** asserting CORS config matches handler header names
- **Dynamic error message** when gate triggers — list which sensitive data
  types are actually present

---

## Known Differences with Related Tools

### bootc-image-builder (BIB)

BIB mounts entitlements at `/etc/pki/entitlement` instead of `/run/secrets/`.
Different pattern from `podman build`. inspectah targets `podman build`,
not BIB.

---

## Out of Scope (v1)

- `--base-image` override — base image is a scan-time decision
- Multi-arch builds (`--platform`)
- Registry push
- Build caching intelligence
- Buildah backend
- Containerfile iteration workflow
- Tarball compression format changes (zstd)

---

## Testing Strategy

### Unit tests — `SubscriptionInspector`

- Collects entitlement cert pairs
- Collects CA certs
- Collects `redhat.repo` when present at `/etc/yum.repos.d/redhat.repo`
- Does NOT collect consumer certs or product-default certs as files
- Permission-denied on cert files → warning (not silent skip)
- Invalid/corrupted PEM (truncated, DER, empty) → warning with filename,
  not panic
- Symlinks within subscription paths: valid → follow, dangling → warn,
  loop → handle safely
- Symlinks resolving outside subscription paths → rejected with warning
- Parses cert expiry from X.509 PEM
- Expiry warning at 14 days
- Expiry warning on already-expired cert (includes path and date)
- No flag → section is None
- Large entitlement directory (100+ certs) → handles without OOM
- Incomplete bundle: entitlement cert without matching key → `incomplete: true`
  with warning naming missing key
- Incomplete bundle: missing `rhsm.conf` → `incomplete: true` with warning
- Incomplete bundle: no CA certs in `/etc/rhsm/ca/` → `incomplete: true`
  with warning
- Incomplete bundle: all four components present → `incomplete: false`
- Incomplete bundle: missing `redhat.repo` → `incomplete: true` with warning
- Org metadata: parses org ID, system UUID, RHSM server from consumer cert
- Org metadata: missing consumer cert → fields are `None`, no warning
- Org metadata: unreadable consumer cert → fields are `None`, no warning

### Unit tests — fleet merge

- Picks latest expiry from multiple hosts
- Hostname tiebreak on same expiry
- Mixed presence (some hosts have certs, some don't)
- Fleet `sensitive_snapshot` / `preserved_subscription` boolean OR semantics
- Fleet export requires `--ack-sensitive` when any host contributed
  subscription data

### Unit tests — `inspectah build`

- Extracts and builds (correct podman command constructed) — use `insta`
  snapshot testing
- Subscription mounts added when `subscription/` present
- Only entitlement + rhsm + redhat.repo mounted (not consumer, not
  product-default)
- `redhat.repo` mounted at `/run/secrets/redhat.repo` when present
- No subscription → no mounts, notice emitted
- Dry-run: command to stdout, notices to stderr
- Dry-run with subscription: three `-v` flags in output (entitlement, rhsm,
  redhat.repo)
- Dry-run without subscription: no `-v` flags
- Passthrough args appended correctly
- Passthrough `-t` emits notice about second tag
- Tag required (missing `-t` → error)
- Bare name rejected (`myimage` without `:tag` → error)
- No podman → exit 127
- No Containerfile → exit 1
- Expired cert warning (proceeds with warning including path and date)
- Keep-context preserves extracted dir
- Cleanup on build failure (without `--keep-context`)
- Archive safety: `../` path traversal → rejected with path in error
- Archive safety: absolute path entry → rejected
- Archive safety: symlink escaping extraction root → rejected
- Archive safety: device node entry → rejected
- Archive safety: FIFO entry → rejected
- Archive safety: socket entry → rejected
- Archive safety: hard link escaping extraction root → rejected
- Archive safety: duplicate path (same path twice) → rejected
- Archive safety: file-type replacement (regular file then symlink with same
  name) → rejected
- Platform-appropriate extraction path

### Unit tests — `--ack-sensitive` rename

- Both `--ack-sensitive` and `--acknowledge-sensitive` accepted
- Web API: both HTTP header names accepted
- CORS config exposes both header names (structural test)
- Dynamic error message lists actual sensitive data types present

### Unit tests — Containerfile renderer

- Subscription present → mount instruction comment block emitted
- No subscription → no subscription block
- Cert expiry date included in comment
- No `COPY` or `ENV` instructions for subscription material

### Unit tests — tarball staging

- `subscription/` directory created with correct subdirectory structure
- Files base64-decoded and written with correct names
- Missing optional subdirectories omitted
- Symlinks in tarball handled safely
- Archive safety: `../` path traversal entries rejected
- Archive safety: absolute path entries rejected
- Archive safety: symlinks escaping extraction root rejected
- Archive safety: device nodes rejected
- Archive safety: FIFOs rejected
- Archive safety: sockets rejected
- Archive safety: hard links escaping extraction root rejected
- Archive safety: duplicate paths rejected
- Archive safety: file-type replacement rejected

### Integration tests

- Full scan pipeline with `--preserve-subscription` → tarball contains
  `subscription/`
- Scan without flag → no subscription data
- `--ack-sensitive` required when any preserve flag set
- Fleet aggregate with subscription → merged picks latest expiry
- Build round-trip: scan → build `--dry-run` → valid command with three `-v`
  mounts (entitlement, rhsm, redhat.repo)
- Build round-trip without subscription → no cert mounts, notice emitted
- Build round-trip with expired cert → warning appears

### Verified — real-box findings (2026-05-28)

| Test | RHEL 9.7 | RHEL 10.2 |
|------|----------|-----------|
| `/etc/pki/entitlement/` | `<serial>.pem` + `<serial>-key.pem` | Same structure, different serial |
| `/etc/rhsm/ca/` | `redhat-entitlement-authority.pem`, `redhat-uep.pem` | Identical |
| `/etc/rhsm/rhsm.conf` | Present | Identical format |
| `/etc/yum.repos.d/redhat.repo` | Present (`%ghost`, created by register) | Present (`%ghost`, created by register) |
| `/etc/pki/consumer/cert.pem` | Present (parsed for org metadata) | Present (parsed for org metadata) |

### Remaining verification — requires manual testing

- Cross-version cert mount: RHEL 9 certs used to build a RHEL 10 image
- Full end-to-end: scan on RHEL, build on Mac with podman machine

---

## Implementation Notes

### Dependencies

New Rust crates:

- `x509-parser` — parse cert expiry from PEM
- `base64` — encode/decode cert content (may already be in tree)
- `dirs` — platform-native cache directory resolution
- `tempfile` or manual cleanup with `Drop` guard — reliable cleanup
- `std::process::Command` — execute podman (stdlib, no new dep)

### File size limits

Individual subscription files are small (PEM certs are typically < 10 KB).
No special size handling needed, but reject files > 1 MB as a safety valve.

### Interaction with existing config exclusions

The existing `UNOWNED_EXCLUDE_GLOBS` already excludes `/etc/pki/entitlement/*`,
`/etc/pki/consumer/*`, `/etc/pki/product-default/*` from the config inspector.
The new `SubscriptionInspector` operates independently — reads these paths
directly and is unaffected by config exclusion globs.

### Dependencies between the two features

`SubscriptionInspector` is independent — no dependency on other inspectors.
`inspectah build` depends on having a valid extracted tarball but has no
compile-time coupling to the scan pipeline. The two features share the
`SubscriptionSection` schema type and the tarball layout convention, but
can be implemented and tested independently.
