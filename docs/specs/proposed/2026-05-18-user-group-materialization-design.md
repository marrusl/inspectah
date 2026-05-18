# User/Group Materialization

## Summary

Generate actionable provisioning artifacts for migrating local human user
accounts from package-mode RHEL to image-mode. Kickstart and Blueprint TOML
snippets are always produced. Users can optionally add accounts to the
Containerfile via useradd or sysusers.d overrides.

## Scope

- **In scope:** Local human users (UID 1000–60000) and their primary/supplementary
  groups. Password and SSH key preservation via opt-in scan flags. Refine UI
  decision section with strategy overrides. Output artifact generation.
- **Out of scope:** System accounts (UID < 1000) — handled by package installation.
  Network users (SSSD/IPA/AD) — handled by identity provider. Package-ownership
  correlation for accounts — deferred. Fleet-mode group conflict resolution —
  future work.

## Scan-Time Collection Changes

### Existing behavior (unchanged)

The `UsersGroupsInspector` scans `/etc/passwd`, `/etc/shadow`, `/etc/group`,
`/etc/gshadow`, `/etc/subuid`, `/etc/subgid`, sudoers, and SSH authorized_keys
references for non-system users (UID 1000–60000). Shadow hashes are stripped
and replaced with status strings (`locked`, `disabled`, `password_set`,
`no_password`). This behavior is preserved as the default.

### New flag: `--preserve-password-hashes`

When set, the shadow parser retains the actual hash string alongside the
status field. The hash is stored in a `password_hash` field on the user JSON
object. The redaction engine's `PASSWORD_HASH` pattern must allowlist shadow
entries for users when this flag is active.

Snapshot metadata gains `"preserved_credentials": true` so downstream tooling
can detect that the tarball contains sensitive credential material.

### New flag: `--preserve-ssh-keys`

When set, `collect_ssh_keys` reads the content of `~/.ssh/authorized_keys`
(currently it only checks existence and counts lines). Public keys are stored
as an array of key strings on the user JSON object. Fingerprints and key types
are derived for display in the refine UI.

Snapshot metadata gains `"preserved_ssh_keys": true`.

### Per-user data shape

After scanning, each user entry in the snapshot contains:

```
{
  "name": "alice",
  "uid": 1000,
  "gid": 1000,
  "shell": "/bin/bash",
  "home": "/home/alice",
  "classification": "interactive",
  "classification_rationale": "bash shell, home at /home/alice, password set, member of wheel, has 2 SSH keys",
  "password_status": "password_set",
  "password_hash": "$6$...",           // only with --preserve-password-hashes
  "ssh_key_count": 2,
  "ssh_keys": ["ssh-ed25519 ...", ...], // only with --preserve-ssh-keys
  "has_sudo": true,
  "has_subuid": true,
  "supplementary_groups": ["wheel", "docker"]
}
```

## Classification Model

Two classifications for human-range users, determined at scan time by shell
type:

| Classification   | Signal                                         | Default Containerfile Strategy |
|------------------|------------------------------------------------|-------------------------------|
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

## Refine Decision Section

Users/Groups becomes a lightweight decision section in the main area (promoted
from the context sidebar). The sidebar maintains its read-only contract — all
interactive controls live in the decision section.

### Section header

- Title: **"Users & Groups"**
- Banner: *"Kickstart and Blueprint TOML provisioning snippets are generated
  for all users below. Use the controls to optionally add users to the
  Containerfile as well."*
- **"Preview Artifacts"** button opens a read-only viewer showing the generated
  `users.ks` and `users.toml` content. Tabbed or toggled between formats.
  Updates live as password/key selections change.

### User card anatomy

```
┌──────────────────────────────────────────────────────────────┐
│  alice (UID 1000)                       [sudo] [ssh:2] [sub]│
│  /bin/bash · /home/alice · wheel, docker                    │
│  Interactive user — password set, 2 SSH keys                │
│                                                             │
│  Containerfile:  ( ) Skip  ( ) useradd  ( ) sysusers        │
│                                                             │
│  ▸ Password options (collapsed)                             │
└──────────────────────────────────────────────────────────────┘
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
  - New password entry field — plaintext input, hashed client-side (SHA-512
    with random salt) before storage in refine state

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
expand/collapse password options, Escape to collapse. Standard form controls
throughout.

## Refine State

Per-user overrides tracked in the refine session:

```
user_overrides[username] = {
  containerfile_strategy: "skip" | "useradd" | "sysusers",  // default: skip
  new_password_hash: Option<String>,    // if user sets a new password in UI
  use_preserved_password: bool          // only available when snapshot has hashes
}
```

Password precedence:
1. `new_password_hash` — user set a new password in the UI (overrides everything)
2. `use_preserved_password: true` — use the hash from the snapshot
3. Neither — no password in artifacts (account created without credentials)

## Output Artifact Generation

### Always generated (in tarball)

**`users.ks` — Kickstart snippet:**

```kickstart
# Users — generated by inspectah
user --name=alice --uid=1000 --gid=1000 --groups=wheel,docker --homedir=/home/alice --shell=/bin/bash --iscrypted --password=$6$salt$hash...
user --name=bob --uid=1001 --gid=1001 --homedir=/home/bob --shell=/bin/bash

# SSH keys
sshkey --username=alice "ssh-ed25519 AAAAC3Nza... alice@work"
sshkey --username=alice "ssh-rsa AAAAB3Nza... alice@laptop"
```

- `--iscrypted --password=` only when hash is available (preserved or newly set)
- `sshkey` lines only when keys are captured via `--preserve-ssh-keys`
- Users with `skip` Containerfile strategy still appear here — KS/TOML is
  independent of the Containerfile decision

**`users.toml` — Blueprint TOML:**

```toml
[[customizations.user]]
name = "alice"
uid = 1000
gid = 1000
groups = ["wheel", "docker"]
home = "/home/alice"
shell = "/bin/bash"
password = "$6$salt$hash..."
key = "ssh-ed25519 AAAAC3Nza... alice@work"

[[customizations.user]]
name = "bob"
uid = 1001
gid = 1001
home = "/home/bob"
shell = "/bin/bash"
```

- `password` field only when hash is available
- `key` field only when keys are captured
- Blueprint TOML's `key` field takes a single string. For users with multiple
  SSH keys, include a comment noting this limitation and suggesting kickstart
  or post-deploy provisioning for additional keys.

### Containerfile directives (only when strategy != skip)

**useradd strategy:**

```dockerfile
# User: alice (useradd)
RUN groupadd -g 1000 alice && \
    useradd -u 1000 -g 1000 -G wheel,docker -d /home/alice -s /bin/bash -m alice
# WARNING: Password hash in image layer — inspectable by anyone with image access
RUN echo 'alice:$6$salt$hash...' | chpasswd -e
RUN mkdir -p /home/alice/.ssh && \
    echo 'ssh-ed25519 AAAAC3Nza... alice@work' >> /home/alice/.ssh/authorized_keys && \
    echo 'ssh-rsa AAAAB3Nza... alice@laptop' >> /home/alice/.ssh/authorized_keys && \
    chown -R alice:alice /home/alice/.ssh && \
    chmod 700 /home/alice/.ssh && chmod 600 /home/alice/.ssh/authorized_keys
```

Password and SSH key blocks are conditional — only emitted when available.
The `chpasswd` line includes an inline comment warning about hash visibility.

**sysusers strategy:**

```dockerfile
# User: alice (sysusers)
COPY alice.conf /usr/lib/sysusers.d/alice.conf
```

With `alice.conf` generated as a tarball artifact:

```
u alice 1000:1000 "Alice" /home/alice /bin/bash
```

sysusers.d does not support passwords, SSH keys, or supplementary group
memberships. If a user with these attributes selects sysusers, the UI shows
an informational note about what won't be provisioned.

### UID/GID pinning

All output artifacts pin the exact UID and GID from the scanned source host.
This is non-negotiable for data continuity — persistent volumes, NFS mounts,
and shared storage carry ownership as numeric IDs. Drift causes silent
permission breakage.

### Containerfile ordering

1. `groupadd` commands for custom groups (GID 1000+)
2. `useradd` commands referencing those groups
3. `chpasswd` for password setup
4. SSH key provisioning
5. `COPY` for sysusers.d snippets

System groups referenced in `-G` (wheel, docker, etc.) are assumed to exist
from their respective packages.

### Format capability matrix

| Attribute             | Kickstart        | Blueprint TOML | useradd          | sysusers.d |
|-----------------------|------------------|----------------|------------------|------------|
| Name, UID, GID        | Yes              | Yes            | Yes              | Yes        |
| Shell, home           | Yes              | Yes            | Yes              | Yes        |
| Supplementary groups  | Yes              | Yes            | Yes (`-G`)       | No         |
| Password hash         | Yes (`--iscrypted`) | Yes (`password`) | Yes (`chpasswd -e`) | No         |
| SSH public keys       | Yes (`sshkey`)   | Yes (`key`)*   | Yes (`authorized_keys`) | No         |

\* Blueprint TOML supports one key per entry.

## Edge Cases

### Fleet UID conflicts

When the same username maps to different UIDs across scanned hosts in fleet
mode, flag as a warning: *"User 'jsmith' has different UIDs across scanned
hosts (1001, 1042). Resolve before migrating — consider centralized identity
(FreeIPA/SSSD) for fleet-wide consistency."* No auto-resolution. The user
must choose which UID wins.

### UID collision with base image

If a source host user's UID collides with a system account in the selected
base image (detectable when base image data is available), warn at refine
time. Unlikely in the 1000–60000 range but possible.

### Locked/disabled accounts

Users with `locked` or `disabled` password status still receive KS/TOML
artifacts. The card displays the status prominently. Most likely candidates
for `skip`, but the user may choose to re-enable with a new password.

### No-password accounts

Accounts with `no_password` status get a card note: *"No password set — user
cannot log in without SSH keys or other authentication."*

### Orphaned shadow entries

Shadow entries with no matching passwd entry are silently skipped.

### Malformed shadow lines

Skipped with a warning in the inspector output.

### Supplementary groups from packages

System groups (wheel, docker, libvirt) are assumed to exist via their owning
packages. If a referenced group's package isn't in the Containerfile's
`RUN dnf install`, the user will encounter a build error — this is a signal
about the package list, not the user list.

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
- **System account scanning:** Expand to UID < 1000 if customers need
  sysusers classification for non-RPM service accounts.
- **Fleet subuid range preservation:** Explicit range pinning in
  materialization artifacts for fleet-wide UID consistency with rootless
  Podman.
- **TUI support:** Strategy overrides in the terminal UI (after web UI ships).
- **Ignition/cloud-init output:** Additional output formats beyond KS/TOML.

## Team Input

Brainstorm session 2026-05-18. Consults:

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
