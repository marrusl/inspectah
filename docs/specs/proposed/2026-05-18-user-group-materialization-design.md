# User/Group Materialization

## Summary

Generate actionable provisioning artifacts for migrating local human user
accounts from package-mode RHEL to image-mode. Kickstart and Blueprint TOML
snippets are always produced as the authoritative provisioning artifacts.
Users can optionally add accounts to the Containerfile via useradd as
install-time seeding into the image.

## Scope

- **In scope:** Local human users (UID 1000–60000) and their primary/supplementary
  groups as first-class artifacts. Password and SSH key preservation via opt-in
  scan flags. Refine UI decision section with Containerfile useradd override.
  Output artifact generation. Sensitive-snapshot export contract.
- **Out of scope:** System accounts (UID < 1000) — handled by package installation.
  Network users (SSSD/IPA/AD) — handled by identity provider. Package-ownership
  correlation for accounts — deferred. Fleet-mode group conflict resolution —
  future work. sysusers.d materialization — belongs in a future system-account
  design where it can faithfully represent the target account state.

## Provisioning Model

### Authoritative vs. advisory artifacts

Kickstart (`inspectah-users.ks`) and Blueprint TOML (`inspectah-users.toml`)
are the **authoritative** provisioning artifacts. They faithfully represent
all in-scope account state: identity, credentials, SSH keys, and group
memberships. They are always generated and always included in the tarball.

Containerfile useradd directives are **install-time seeding** — an optional
alternative for operators who want user accounts baked into the image rather
than provisioned at deploy time. The Containerfile path is secondary to KS/TOML
and is explicitly labeled as such in the UI and artifact output.

### Supported combinations

A user's account appears in KS and TOML regardless of the Containerfile
decision. The Containerfile strategy controls only whether additional
`useradd`/`groupadd` directives are emitted in the Containerfile:

| Containerfile strategy | KS/TOML generated? | Containerfile directives? |
|------------------------|---------------------|---------------------------|
| **Skip** (default)     | Yes                 | No                        |
| **useradd**            | Yes                 | Yes                       |

There is no double-provisioning risk because KS/TOML and Containerfile serve
different deployment models. KS/TOML are consumed at install time by Anaconda
or Image Builder. Containerfile directives are consumed at image build time by
`podman build` / `buildah`. An operator uses one or the other, not both
simultaneously.

### Why sysusers is excluded

`systemd-sysusers` cannot represent passwords, SSH keys, or supplementary
group memberships. For the in-scope human-account set, it creates an identity
stub rather than a faithful migration of account state. Upstream bootc guidance
places sysusers in the system-user bucket and recommends install-time or
external mechanisms for human accounts. sysusers materialization will be
addressed in a separate system-account design where it fits naturally.

## Scan-Time Collection Changes

### Existing behavior (unchanged)

The `UsersGroupsInspector` scans `/etc/passwd`, `/etc/shadow`, `/etc/group`,
`/etc/gshadow`, `/etc/subuid`, `/etc/subgid`, sudoers, and SSH authorized_keys
references for non-system users (UID 1000–60000). Shadow hashes are stripped
and replaced with status strings (`locked`, `disabled`, `password_set`,
`no_password`). This behavior is preserved as the default.

### New flag: `--preserve-password-hashes`

When set, the shadow parser retains the actual `crypt(3)`-format hash string
alongside the status field. The hash is stored in a `password_hash` field on
the user JSON object. The hash must be a valid `crypt(3)` value (e.g.,
`$6$rounds=5000$salt$hash` for sha512crypt, `$y$...` for yescrypt). The
redaction engine's `PASSWORD_HASH` pattern must allowlist shadow entries for
users when this flag is active.

This flag activates sensitive-snapshot mode (see Sensitive Snapshot Contract).

### New flag: `--preserve-ssh-keys`

When set, `collect_ssh_keys` reads the content of `~/.ssh/authorized_keys`
(currently it only checks existence and counts lines). Public keys are stored
as an array of key strings on the user JSON object. Fingerprints and key types
are derived for display in the refine UI.

This flag activates sensitive-snapshot mode (see Sensitive Snapshot Contract).

### Per-user data shape

After scanning, each user entry in the snapshot contains:

```json
{
  "name": "alice",
  "uid": 1000,
  "gid": 1000,
  "shell": "/bin/bash",
  "home": "/home/alice",
  "classification": "interactive",
  "classification_rationale": "bash shell, home at /home/alice, password set, member of wheel, has 2 SSH keys",
  "password_status": "password_set",
  "password_hash": "$6$rounds=5000$...",  // only with --preserve-password-hashes
  "ssh_key_count": 2,
  "ssh_keys": ["ssh-ed25519 ...", ...],   // only with --preserve-ssh-keys
  "has_sudo": true,
  "has_subuid": true,
  "supplementary_groups": ["wheel", "docker"]
}
```

### Per-group data shape

Groups in the human GID range (1000–60000) are collected alongside users:

```json
{
  "name": "alice",
  "gid": 1000,
  "members": ["alice"],
  "is_primary": true,
  "source": "custom"
}
```

The `source` field classifies groups:
- `custom` — GID 1000+ and not owned by a known package. Always materialized.
- `system` — GID < 1000 (wheel, docker, etc.). Assumed to exist via packages.
  Referenced in supplementary membership lists but not materialized.

## Classification Model

Two classifications for human-range users, determined at scan time by shell
type:

| Classification   | Signal                                                   | Default Containerfile Strategy |
|------------------|----------------------------------------------------------|-------------------------------|
| **Interactive**      | Shell in the login-shell set (bash, zsh, fish, sh, etc.) | Skip                          |
| **Non-interactive**  | Shell is nologin, false, or other non-login shell        | Skip                          |

Both classifications default to **skip** for the Containerfile strategy because
kickstart and blueprint TOML artifacts are always generated regardless. The
Containerfile strategy is an opt-in override.

Non-interactive users in the human UID range are unusual (likely manually-created
service accounts or deactivated users) and receive a "review recommended" visual
indicator.

### Classification rationale

Each user's classification includes a plain-language explanation assembled from
observed signals: shell type, home directory, password status, sudo access, SSH
key presence, and group memberships. This builds user trust in the defaults
before they consider overriding.

### Login shell set

Canonical list for the `interactive` classification:

```
/bin/bash, /bin/zsh, /bin/sh, /bin/fish, /bin/tcsh, /bin/csh,
/usr/bin/bash, /usr/bin/zsh, /usr/bin/fish
```

Any shell not in this set maps to `non-interactive`. This matches the existing
`VALID_LOGIN_SHELLS` constant in `inspectah-collect/src/inspectors/users.rs`.

## Sensitive Snapshot Contract

When either `--preserve-password-hashes` or `--preserve-ssh-keys` is used,
the snapshot enters **sensitive mode**. This changes the trust model of
inspectah artifacts.

### Snapshot metadata

```json
{
  "sensitive_snapshot": true,
  "preserved_credentials": true,   // when --preserve-password-hashes
  "preserved_ssh_keys": true       // when --preserve-ssh-keys
}
```

### Redaction state

A new `redaction_state` variant `PartiallyRedacted` indicates that the
snapshot has been through the redaction engine but intentionally retains
specific credential material. This is distinct from `FullyRedacted` (normal
safe-by-default) and `Unredacted` (never processed).

### Export gating

Sensitive tarballs require explicit operator acknowledgment before export:

- **Scan export:** The CLI prints a warning and requires `--acknowledge-sensitive`
  or interactive confirmation before writing the tarball.
- **Refine export:** The export API returns a gating response requiring
  acknowledgment. The UI shows a sensitivity banner with the specific
  sensitive content types (passwords, SSH keys) before allowing download.
- **Re-import:** Sensitive tarballs can be imported into refine. The session
  displays a persistent sensitivity banner indicating the snapshot contains
  credential material. `redaction_state: PartiallyRedacted` is preserved
  through import.

### Preview safety

Preserved password hashes and full SSH key content are **redacted by default**
in all preview surfaces (artifact viewer, user cards, Containerfile preview).
A per-value reveal toggle allows the operator to view sensitive values
explicitly. Reveal state is ephemeral — it resets on page navigation or
session reload, never persisted.

Detailed preview/reveal interaction patterns, focus/close behavior, and
accessibility contract are deferred to the companion backlog item
(`workflow/backlog/2026-05-18-inspectah-user-group-sensitive-export-hardening.md`).

## Refine Decision Section

Users/Groups becomes a lightweight decision section in the main area (promoted
from the context sidebar). The sidebar maintains its read-only contract — all
interactive controls live in the decision section.

### Navigation taxonomy

`users_groups` moves from the Context group to the Decisions group in the
sidebar navigation. This change must be reflected in:
- `inspectah-web/ui/src/components/Sidebar.tsx` — section group assignment
- `inspectah-web/ui/src/components/MainContent.tsx` — rendering as decision section
- `useKeyboard()` — shortcut registration
- Search — include user/group items in search results
- Export — include user decisions in the export contract

### Section header

- Title: **"Users & Groups"**
- Banner: *"Kickstart and Blueprint TOML provisioning snippets are generated
  for all users below. Use the controls to optionally add users to the
  Containerfile as well."*
- **"Preview Artifacts"** button opens a read-only viewer showing the generated
  `inspectah-users.ks` and `inspectah-users.toml` content. Tabbed or toggled
  between formats. Updates live as password/key selections change. Sensitive
  values redacted by default with per-value reveal toggles.

### User card anatomy

```
+--------------------------------------------------------------+
|  alice (UID 1000)                       [sudo] [ssh:2] [sub] |
|  /bin/bash . /home/alice . wheel, docker                     |
|  Interactive user -- password set, 2 SSH keys                |
|                                                              |
|  Containerfile:  ( ) Skip  ( ) useradd                       |
|                                                              |
|  > Password options (collapsed)                              |
|  > SSH keys: 2 captured (collapsed)                          |
+--------------------------------------------------------------+
```

- **Name + UID** — primary identity line
- **Badges** — visual indicators with cross-references to the config section:
  - **sudo** (amber) — privilege escalation, highest visual weight
  - **SSH keys** (blue/gray) — shows count and captured vs. detected state
  - **subuid/subgid** (muted teal) — container enablement flag, lowest visual weight
- **Details line** — shell, home directory, supplementary group memberships
- **Classification rationale** — plain-language explanation
- **Containerfile strategy selector** — radio group. Default: Skip.
  Selecting useradd shows an inline warning about secrets in image layers.
- **Password options** — collapsed by default. Expands to show:
  - With `--preserve-password-hashes`: "Keep existing" / "Set new" / "No password"
  - Without: "Set new" / "No password"
  - New password entry field — plaintext input, hashed client-side to
    `crypt(3)` sha512crypt format (`$6$rounds=5000$<salt>$<hash>`) before
    storage in refine state. The browser uses a JS `crypt(3)` implementation
    (e.g., `crypt-js` or equivalent) to produce the hash, not a generic
    SHA-512 digest.
- **SSH key detail** — collapsed by default. Expands to show key type,
  fingerprint (e.g., `ed25519 SHA256:abc...xyz`), and captured/detected state
  per key. Full key text hidden behind per-key reveal toggle. Display-only —
  keys are not editable.

### Non-interactive user cards

Visually distinguished with a muted border or background and a "Review
recommended" tag. These accounts (non-login shell in the human UID range) are
unusual and warrant the user's attention.

### Empty state

*"No local user accounts found (UID 1000–60000). Network-managed users
(SSSD/IPA/AD) are handled by your identity provider and don't require
migration."*

### Keyboard navigation

Tab through cards, arrow keys within strategy selector, Enter to
expand/collapse password/SSH sections, Escape to collapse and return focus
to the card header. Focus trap within expanded sections. Standard form
controls throughout. ARIA: cards are `role="region"` with `aria-label`
including the username; strategy selector is a `radiogroup`.

## Refine State and API Contract

### Persisted state

Per-user decisions are stored in the refine session as `RefinementOp` variants:

```rust
enum RefinementOp {
    // ... existing package/config/repo ops ...
    UserStrategy { username: String, strategy: UserContainerfileStrategy },
    UserPassword { username: String, password: UserPasswordChoice },
}

enum UserContainerfileStrategy {
    Skip,     // default — no Containerfile directives
    Useradd,  // RUN useradd + optional credentials
}

enum UserPasswordChoice {
    NoPassword,
    PreserveExisting,   // only valid when snapshot has preserved hashes
    NewPassword(String), // crypt(3)-format hash
}
```

User decisions participate in the existing op pipeline: undo/redo,
generation tracking, and stale-generation protection apply the same way
as package/config ops.

### API operations

| Endpoint                       | Method | Body                                       |
|--------------------------------|--------|---------------------------------------------|
| `/api/user-strategy`           | POST   | `{ "username": "alice", "strategy": "useradd" }` |
| `/api/user-password`           | POST   | `{ "username": "alice", "choice": "preserve" }` or `{ "username": "alice", "choice": "new", "hash": "$6$..." }` |
| `/api/user-preview`            | GET    | Returns rendered `inspectah-users.ks` and `inspectah-users.toml` content |

All endpoints return the updated `ViewResponse` with the Users & Groups
section reflecting the new state, consistent with how package ops work today.

### Export contract

The refine export tarball gains two new files:
- `inspectah-users.ks`
- `inspectah-users.toml`

These are added to the approved export file set in
`inspectah-refine/tests/export_contract_test.rs`. Preview/export parity is
tested: the content returned by `/api/user-preview` must match the content
written to the export tarball byte-for-byte.

When the snapshot is in sensitive mode and the user has selected "Keep existing"
or "Set new" password, the exported KS/TOML contain credential material. The
export gating contract (see Sensitive Snapshot Contract) applies.

User decisions survive export/re-import through the refine tarball. The op
log is serialized alongside the snapshot in the export, and
`from_tarball()` reconstructs user decisions the same way it reconstructs
package/config decisions today.

## Output Artifact Generation

### Group materialization

Groups are first-class artifacts. Custom groups (GID 1000+) are explicitly
materialized before user creation in every output format:

**Kickstart:**
```kickstart
# Groups — generated by inspectah
group --name=alice --gid=1000
group --name=developers --gid=1005

# Users — generated by inspectah
user --name=alice --uid=1000 --gid=1000 --groups=wheel,docker,developers ...
```

**Blueprint TOML:**
```toml
[[customizations.group]]
name = "alice"
gid = 1000

[[customizations.group]]
name = "developers"
gid = 1005

[[customizations.user]]
name = "alice"
uid = 1000
gid = 1000
groups = ["wheel", "docker", "developers"]
...
```

**Containerfile (useradd strategy):**
```dockerfile
# Groups (custom, GID 1000+)
RUN groupadd -g 1000 alice
RUN groupadd -g 1005 developers

# Users
RUN useradd -u 1000 -g 1000 -G wheel,docker,developers ...
```

### Group rules

- **Custom groups** (GID 1000+, `source: "custom"`): always materialized
  in KS, TOML, and Containerfile (when useradd is selected for any member).
- **System groups** (GID < 1000): never materialized. Referenced in
  supplementary membership lists (`-G wheel,docker`). Assumed to exist via
  their owning packages.
- **Collision policy:** If a custom group's GID conflicts with an existing
  group in the base image (detectable when base image data is available),
  warn at refine time. Same treatment as UID collisions.
- **Shared groups:** When multiple users share a custom group, the group is
  materialized once. All users reference it. No per-user duplication.
- **Primary group creation:** `groupadd` for the primary group is always
  emitted before the `useradd` that references it. For KS, the `group`
  line precedes the `user` line. For TOML, `[[customizations.group]]`
  precedes `[[customizations.user]]`.

### Always generated (in tarball)

**`inspectah-users.ks` — Kickstart snippet:**

```kickstart
# Groups — generated by inspectah
group --name=alice --gid=1000

# Users — generated by inspectah
user --name=alice --uid=1000 --gid=1000 --groups=wheel,docker --homedir=/home/alice --shell=/bin/bash --iscrypted --password=$6$rounds=5000$salt$hash...

# SSH keys
sshkey --username=alice "ssh-ed25519 AAAAC3Nza... alice@work"
sshkey --username=alice "ssh-rsa AAAAB3Nza... alice@laptop"
```

- `--iscrypted --password=` only when hash is available (preserved or newly set)
- Password hash must be a valid `crypt(3)` value accepted by Anaconda
- `sshkey` lines only when keys are captured via `--preserve-ssh-keys`
- All users appear here regardless of Containerfile strategy — KS/TOML is
  independent of the Containerfile decision

**`inspectah-users.toml` — Blueprint TOML:**

```toml
[[customizations.group]]
name = "alice"
gid = 1000

[[customizations.user]]
name = "alice"
uid = 1000
gid = 1000
groups = ["wheel", "docker"]
home = "/home/alice"
shell = "/bin/bash"
password = "$6$rounds=5000$salt$hash..."
key = "ssh-ed25519 AAAAC3Nza... alice@work"
```

- `password` field only when hash is available
- `key` field only when keys are captured
- Blueprint TOML's `key` field takes a single string. For users with multiple
  SSH keys, a TOML comment notes this limitation. The complete key set is
  available in the kickstart artifact and as a generated `authorized_keys`
  file (see SSH key staging below).

### Containerfile directives (useradd only, when strategy != skip)

```dockerfile
# Groups (custom, GID 1000+)
RUN groupadd -g 1000 alice

# User: alice (useradd — install-time seeding)
RUN useradd -u 1000 -g 1000 -G wheel,docker -d /home/alice -s /bin/bash -m alice
```

**Password block** (conditional — only when hash available):
```dockerfile
# WARNING: Password hash in image layer — inspectable by anyone with image access
RUN echo 'alice:$6$rounds=5000$salt$hash...' | chpasswd -e
```

**SSH key block** (conditional — only when keys captured):
```dockerfile
COPY config/home/alice/.ssh/authorized_keys /home/alice/.ssh/authorized_keys
RUN chown alice:alice /home/alice/.ssh/authorized_keys && \
    chmod 600 /home/alice/.ssh/authorized_keys
```

SSH keys use `COPY` with a generated `authorized_keys` file staged in the
tarball's `config/` tree, not shell `echo` chains. This is lossless and
shell-safe regardless of key content. The generated file is one key per line,
no trailing whitespace, newline-terminated. See SSH key staging below.

### SSH key staging

When `--preserve-ssh-keys` is active, a generated `authorized_keys` file is
created per user in the tarball at `config/home/<username>/.ssh/authorized_keys`.
This file:
- Contains one key per line in standard OpenSSH format
- Is newline-terminated (no trailing blank lines)
- Is referenced by `COPY` in the Containerfile (useradd strategy)
- Is referenced by `sshkey` directives in kickstart (one per key)
- Is referenced by the `key` field in blueprint TOML (first key only;
  comment notes the limitation)

This staging approach is lossless: keys are written once to a file and
consumed by reference, avoiding shell escaping issues entirely.

### Containerfile ordering

1. `groupadd` commands for custom groups (GID 1000+)
2. `useradd` commands referencing those groups via `-G`
3. `chpasswd -e` for password setup (if applicable)
4. `COPY` for SSH `authorized_keys` files (if applicable)
5. Ownership/permission fixup for `.ssh` directories

System groups referenced in `-G` (wheel, docker, etc.) are assumed to exist
from their respective packages.

### UID/GID pinning

All output artifacts pin the exact UID and GID from the scanned source host.
This is non-negotiable for data continuity — persistent volumes, NFS mounts,
and shared storage carry ownership as numeric IDs. Drift causes silent
permission breakage.

### Format capability matrix

| Attribute             | Kickstart            | Blueprint TOML     | useradd (Containerfile)  |
|-----------------------|----------------------|--------------------|--------------------------|
| Name, UID, GID        | Yes                  | Yes                | Yes                      |
| Shell, home           | Yes                  | Yes                | Yes                      |
| Supplementary groups  | Yes (`--groups`)     | Yes (`groups`)     | Yes (`-G`)               |
| Password hash         | Yes (`--iscrypted`)  | Yes (`password`)   | Yes (`chpasswd -e`)      |
| SSH public keys       | Yes (`sshkey`, multi)| Yes (`key`, single)| Yes (`COPY authorized_keys`) |
| Custom groups         | Yes (`group --name`) | Yes (`[[customizations.group]]`) | Yes (`groupadd`) |

### Credential format contract

**Password hashes** must be `crypt(3)`-compatible values accepted by all three
sinks:
- Kickstart `user --iscrypted --password=<hash>`
- Blueprint TOML `password = "<hash>"`
- `echo '<user>:<hash>' | chpasswd -e`

Accepted formats: `$6$...` (sha512crypt), `$y$...` (yescrypt), `$5$...`
(sha256crypt). The browser-side hashing implementation generates sha512crypt
(`$6$rounds=5000$<16-char-random-salt>$<hash>`). Preserved hashes from the
source system pass through unchanged regardless of algorithm.

**SSH keys** are stored and emitted as complete OpenSSH `authorized_keys`
lines (key type + base64 blob + optional comment). No transformation,
truncation, or re-encoding.

## Edge Cases

### Fleet UID conflicts

When the same username maps to different UIDs across scanned hosts in fleet
mode, flag as a warning: *"User 'jsmith' has different UIDs across scanned
hosts (1001, 1042). Resolve before migrating — consider centralized identity
(FreeIPA/SSSD) for fleet-wide consistency."* No auto-resolution. The user
must choose which UID wins.

### UID/GID collision with base image

If a source host user's UID or a custom group's GID collides with an
account in the selected base image (detectable when base image data is
available), warn at refine time. Unlikely in the 1000–60000 range but
possible.

### Group name/GID conflicts

If a custom group's name matches an existing system group but with a
different GID (or vice versa), warn and block materialization for that
group until the operator resolves. This prevents silent GID remapping.

### Locked/disabled accounts

Users with `locked` or `disabled` password status still receive KS/TOML
artifacts. The card displays the status prominently. Most likely candidates
for the `skip` Containerfile strategy, but the user may choose to re-enable
with a new password.

### No-password accounts

Accounts with `no_password` status get a card note: *"No password set — user
cannot log in without SSH keys or other authentication."*

### Orphaned shadow entries

Shadow entries with no matching passwd entry are silently skipped.

### Malformed shadow lines

Skipped with a warning in the inspector output.

### Supplementary groups from packages

System groups (wheel, docker, libvirt) are assumed to exist via their owning
packages. If a referenced group's package isn't included in the Containerfile's
`RUN dnf install`, the image build will fail with a group-not-found error.
This is a signal about the package list, not the user list.

### Missing supplementary groups in useradd

When a useradd user references a supplementary group that is not being
materialized (system group) and that group's package is not in the package
section, the UI shows a warning: *"User 'alice' references group 'docker'
which requires the docker package. Ensure it is included in the package
section."*

### Sudoers cross-reference

Sudoers files are handled by the config section as normal config files (COPY
directives in the Containerfile). The user card shows a **sudo badge** (amber)
as a cross-reference indicator. No duplication of config-section handling.

### Subuid/subgid cross-reference

`/etc/subuid` and `/etc/subgid` are handled by the config section. The user
card shows a **subuid badge** (muted teal). RHEL 9+ auto-allocates subuid/subgid
for new users; original ranges are preserved in the config section for manual
alignment if needed.

## Future Work

- **Package-ownership correlation:** Trace accounts to the RPM that created
  them. Deferred — not needed for human-range users.
- **System account materialization with sysusers.d:** Separate design for
  UID < 1000 accounts where sysusers faithfully represents the target state.
- **Fleet subuid range preservation:** Explicit range pinning in
  materialization artifacts for fleet-wide UID consistency with rootless
  Podman.
- **TUI support:** Strategy overrides in the terminal UI (after web UI ships).
- **Ignition/cloud-init output:** Additional output formats beyond KS/TOML.
- **Detailed preview/reveal UX:** Interaction patterns for sensitive value
  reveal, focus/close behavior, accessibility contract
  (`workflow/backlog/2026-05-18-inspectah-user-group-sensitive-export-hardening.md`).

## Team Input

### Brainstorm session (2026-05-18)

Consults:

- **Fern (UX):** Recommended enhanced sidebar (overruled — Mark prefers clean
  info/action boundary). Advised on badge hierarchy (sudo amber, SSH blue,
  subuid teal), sudo/subuid cross-reference pattern, SSH key display (fingerprints
  not blobs, display-only not editable).
- **Ember (strategy):** Confirmed SSH key collection as differentiation (cross-paradigm
  migration gap nobody else solves). Confirmed UID pinning as migration fidelity
  requirement. Flagged fleet UID divergence as a detection/warning concern.
- **Seal (container plumbing):** Confirmed UID pinning is non-negotiable for
  persistent volume continuity. Confirmed rootless subuid mapping is orthogonal
  to host UID pinning. Noted sysusers.d limitation (no passwords/keys/groups).

### Round 1 review (2026-05-18)

All five reviewers requested changes. Shared blockers addressed in this
revision:

1. **Provisioning contract ambiguity** — resolved by defining KS/TOML as
   authoritative and Containerfile as install-time seeding. Removed sysusers
   from scope (Collins: wrong fit for human accounts).
2. **Groups not first-class** — resolved by adding explicit group
   materialization in all formats with collision policy (Thorn, Collins).
3. **Sensitive export contract** — resolved by defining sensitive-snapshot
   mode with `PartiallyRedacted` state, export gating, and preview-redaction
   defaults (Collins, Tang, Thorn, Press). Detailed hardening deferred to
   companion backlog item.
4. **Refine/API/export not repo-real** — resolved by defining `RefinementOp`
   variants, API endpoints, export file set additions, and navigation
   taxonomy changes (Tang, Thorn, Fern).
5. **Credential/artifact rendering too loose** — resolved by specifying exact
   `crypt(3)` format, staged `authorized_keys` files via `COPY` instead of
   shell echo chains, and format capability matrix (Tang, Thorn, Press).

Individual review files:
- `marks-inbox/reviews/2026-05-18-inspectah-user-group-materialization-design-collins-review.md`
- `marks-inbox/reviews/2026-05-18-inspectah-user-group-materialization-design-fern-review.md`
- `marks-inbox/reviews/2026-05-18-inspectah-user-group-materialization-design-press-review.md`
- `marks-inbox/reviews/2026-05-18-inspectah-user-group-materialization-design-tang-review.md`
- `marks-inbox/reviews/2026-05-18-inspectah-user-group-materialization-design-thorn-review.md`

Related backlog items:
- `workflow/backlog/2026-05-18-inspectah-user-group-contract-hardening.md`
- `workflow/backlog/2026-05-18-inspectah-user-group-sensitive-export-hardening.md`
