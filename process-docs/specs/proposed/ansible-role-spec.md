# Ansible Role Design Spec: ansible-role-inspectah

**Author:** Pele (Automation & Fleet Configuration Engineer)
**Date:** 2026-06-27
**Revised:** 2026-06-27 (round 3 review — campaign ID scoping fix)
**Status:** Proposed
**Scope:** Galaxy-publishable Ansible role for fleet-wide inspectah scan orchestration

---

## 1. Purpose

Provide an Ansible role that automates the inspectah scan-fetch-aggregate
pipeline across a fleet of RHEL/CentOS/Fedora hosts. The role handles
per-host work (optional install, scan, fetch tarball to control node).
The aggregate step runs on localhost and is demonstrated in an example
playbook but is not part of the role itself.

### What the role does

1. **(Optional) Install** inspectah via COPR or local RPM push
2. **Preflight** — validate prerequisites including inspectah version
3. **Scan** the target host, producing a migration snapshot tarball
4. **Fetch** the tarball to the control node
5. **Clean up** — tarball cleanup (post-fetch) and opt-in orphan
   container sweep (inspectah's internal cleanup is the primary
   container cleanup mechanism)
6. **(Optional) Uninstall** — remove inspectah and COPR repo if the
   role installed them (`inspectah_cleanup_host` flag)

### What the role does NOT do

- Run `inspectah aggregate` (that is a control-node operation, not a
  per-host operation; it belongs in the consuming playbook)
- Run `inspectah refine` (interactive, not automatable)
- Manage container registries or image mirrors
- Configure podman itself (podman >= 4.4 is a prerequisite)
- Verify provenance of local RPMs — that is the operator's
  responsibility (see Section 14.4)

---

## 2. Design Decision Evaluation

The team brainstormed this role before Pele joined. Below is a
decision-by-decision assessment against Ansible community standards.

### Decision 1: Scope (Scan + Fetch + Aggregate)

**Agree with modification.**

Scan and fetch are per-host operations and belong in the role. Aggregate
is a control-node operation that consumes the fetched tarballs. Including
aggregate as a task inside the role would violate the single-responsibility
principle: the role would need `delegate_to: localhost` logic inside a
host-targeted play, which creates confusing execution semantics and breaks
`--limit`.

The brainstorm correctly placed aggregate in the example playbook as a
separate play. The spec formalizes this: aggregate is out of role scope.
The example playbook demonstrates the full pipeline.

### Decision 2: Separate repo (ansible-role-inspectah)

**Agree.**

Galaxy convention requires `ansible-role-<name>` repository naming for
standalone roles. A separate repo gives the role its own release cycle,
issue tracker, and CI pipeline. Galaxy import works directly from GitHub
with this naming convention.

Keeping it outside the inspectah Rust workspace avoids polluting the
Cargo workspace with unrelated Ansible content and lets Ansible-only
contributors work without the Rust toolchain.

**Repository location:**
- **GitHub:** Under Mark's personal namespace initially (same as inspectah)
- **Local development path:** `/Users/mrussell/Work/bootc-migration/ansible-role-inspectah/`
  (alongside inspectah and driftify in the bootc-migration workspace)

### Decision 3: Optional installation via inspectah_install

**Agree with modification.**

Optional install is the right default. Many sites pre-install via their
own package management. Two modifications:

1. **Add `inspectah_install_version` variable.** Without version pinning,
   `dnf install inspectah` pulls whatever is latest. Fleet scans should
   use a consistent version across all hosts. Default: `""` (latest).
   When set, the task uses `ansible.builtin.dnf` with
   `name: "inspectah-{{ inspectah_install_version }}"`.

2. **Separate COPR enablement from package install.** The COPR repo
   enable step is a distinct operation from the package install. If the
   repo is already enabled (common in fleet management), re-running the
   enable command is harmless but the role should be explicit about this
   two-step flow in its task structure. Use
   `community.general.copr` module when available, falling back to
   `ansible.builtin.command` with `creates:` guard for idempotency.

### Decision 4: Repository structure

**Agree with modification.**

The proposed structure is sound Galaxy layout. Three modifications:

1. **Add `meta/argument_specs.yml`.** The brainstorm mentioned this as
   a "consider" item from Birch's consult. This is not optional for
   publication-quality roles. Argument specs provide type validation,
   default documentation, and `ansible-doc` integration. Every variable
   in `defaults/main.yml` must have a corresponding entry.

2. **Rename `tests/` to a Molecule scaffold.** The proposed `tests/`
   directory with `inventory` and `test.yml` is the pre-Molecule testing
   pattern. Modern Galaxy roles use Molecule for testing. The `tests/`
   directory stays (Galaxy expects it) but Molecule scenarios go in
   `molecule/`. See Section 12 for the full Molecule layout.

3. **Add `vars/main.yml` for internal constants.** Role-internal values
   that consumers should not override (the `--progress flat` flag,
   container name prefix for cleanup, etc.) belong in `vars/main.yml`,
   not hardcoded in tasks or exposed in `defaults/`. This is standard
   Galaxy practice: `defaults/` is the public API, `vars/` is internal.

### Decision 5: Variables (defaults/main.yml)

**Agree with modification.**

The variable interface is well-designed. The `inspectah_` prefix is
correct. Modifications:

1. **Replace `inspectah_scan_output_dir` with `inspectah_scan_output`.**
   inspectah's `-o` flag takes a **file path**, not a directory. The
   role constructs an explicit tarball path:
   `/var/lib/inspectah/scans/{{ inventory_hostname }}.tar.gz` and passes
   it directly to `-o`. This eliminates tarball discovery ambiguity:
   the role knows the exact path at scan time and threads it through
   fetch and cleanup without glob searches.

   The variable `inspectah_scan_output` defaults to
   `/var/lib/inspectah/scans/{{ inventory_hostname }}.tar.gz`. Operators
   who need a custom path can override it, but the default is correct
   for most fleets.

2. **`inspectah_fetch_dest` default `"./scans"` is fragile.** Relative
   paths resolve against the playbook directory, which varies by
   invocation method (ansible-playbook, AWX, AAP). Change the default
   to `"{{ playbook_dir }}/scans"` to make the resolution explicit. The
   README should warn that AWX/AAP users need to set this to an
   absolute path.

3. **Add `inspectah_scan_timeout` variable.** The brainstorm hardcoded
   `async: 600`. Cold scans with large base images can exceed 10
   minutes. Expose this as a variable with a default of `900` (15
   minutes). The poll interval should be a separate variable
   `inspectah_scan_poll` defaulting to `30`.

4. **`inspectah_extra_args` type should be `list`, not `string`.** A
   string invites shell-injection risks and quoting errors. A list is
   joined with spaces at invocation time and is safer:
   `inspectah_extra_args: ["--verbose", "--no-baseline"]`.

   **Safety constraint:** `inspectah_extra_args` MUST NOT override the
   role's safety invariants. The role's scan command construction
   applies `-o`, `--progress flat`, and `--ack-sensitive` unconditionally
   when required. If `extra_args` contains any of these flags, the
   preflight task emits a warning and strips them. Documented in the
   README variable table and enforced by a validation task.

5. **Add `inspectah_base_image` validation.** When set, this should be
   a non-empty string matching a container image reference pattern. The
   argument spec can validate format; the task should fail-fast with a
   clear message if the format is invalid rather than letting inspectah
   error deep in the pipeline.

6. **Remove `inspectah_aggregate_output_dir` and
   `inspectah_aggregate_extra_args`.** These are not role variables.
   Aggregate runs on localhost in the consuming playbook. Putting them
   in the role's defaults implies the role handles aggregate, which it
   does not. Move them to the example playbook as play-level vars.

### Decision 6: Task flow

**Agree with modification.**

The flow is correct: install -> preflight -> scan -> fetch -> cleanup.
Modifications:

1. **`--progress flat` must be forced unconditionally.** Ansible
   captures stdout; rich/plain progress modes emit ANSI escape sequences
   that corrupt the registered output. The brainstorm correctly
   identified this. This goes in `vars/main.yml` as an internal
   constant: `_inspectah_progress_mode: "flat"`.

2. **`--ack-sensitive` auto-add logic needs care.** The brainstorm says
   "auto-add when preserve options set." This is correct but incomplete.
   `--ack-sensitive` is also required when `inspectah_no_redaction` is
   true. The task should add `--ack-sensitive` when EITHER
   `inspectah_preserve` is non-empty OR `inspectah_no_redaction` is
   true. And it should NEVER add `--ack-sensitive` otherwise. This is
   a correctness requirement: inspectah will reject the scan without it.

3. **Fetch task should use `ansible.builtin.fetch` with `fail_on_missing: true`.**
   If the scan failed silently (exit 0 but no tarball), the fetch should
   fail explicitly rather than silently skipping.

4. **Two-layer cleanup model.** Container cleanup and tarball cleanup
   are separate concerns with different mechanisms:
   - **Container cleanup (primary):** inspectah's internal `CleanupGuard`
     runs `podman rm -f` on the baseline container when the scan function
     returns or panics. This is the primary contract — the role does not
     attempt invocation-scoped container cleanup because it cannot predict
     the container name (see Section 5.6).
   - **Container cleanup (orphan janitor):** An opt-in role-level sweep
     of stale `inspectah-baseline-*` containers left by crash-killed
     processes. Runs in scan.yml's `always` block but only when
     `inspectah_cleanup_orphan_containers` is true. Not safe for
     concurrent scans.
   - **Tarball cleanup** runs in fetch.yml's success path, only after
     the tarball has been successfully transferred. Never in an
     `always` block — a failed fetch must not delete the only copy
     of the tarball.

5. **Register scan output.** The scan task should register its result
   so the role can expose scan outcome (exit code, tarball path) to the
   consuming playbook via `inspectah_scan_result`.

### Decision 7: Example playbook (site.yml)

**Agree with modification.**

Three-play structure. Play 0 generates the campaign ID on localhost
(truly once, before serial batching), Play 1 scans the fleet with the
campaign ID passed via `hostvars['localhost']`, and Play 2 aggregates.
This replaces the earlier two-play structure to fix a `run_once` scoping
bug (see Section 5.5). Modifications:

1. **The `serial: 10` recommendation belongs in the example playbook
   itself, not just the README.** Show it as a commented-out line in the
   example with a comment explaining the thundering-herd concern. Users
   copy examples; they don't always read READMEs.

2. **Add a `max_fail_percentage` recommendation.** At fleet scale, a
   few host failures should not abort the entire scan campaign. The
   example should show `max_fail_percentage: 10` or similar.

3. **The aggregate play should pass `--output-dir` explicitly.** The
   brainstorm uses `inspectah_aggregate_output_dir` but aggregate
   accepts `--output-dir` as a flag. Map this clearly.

4. **Propagate `--ack-sensitive` to aggregate when scan used preserve
   options.** If any scan in the campaign used `--preserve` or
   `--no-redaction`, the aggregate also needs `--ack-sensitive`. The
   example playbook should show this with a comment explaining the
   linkage.

### Decision 8: Fleet-scale concerns

**Agree.**

Collins's consult identified the real fleet-scale issues. The role
addresses them:

- **Thundering herd:** `serial` in the consuming playbook (not the role)
- **Disk space:** Documented in README as a prerequisite check
- **Scan timeout:** Exposed via `inspectah_scan_timeout` variable
- **Container cleanup:** inspectah's internal `CleanupGuard` is primary;
  role provides opt-in orphan janitor for crash leftovers
- **nsenter/util-linux:** Added to prerequisite checks (fail_fast
  preflight task)

One addition: **network bandwidth for fetch.** Tarballs are typically
5-50MB per host. At 500 hosts, that is 2.5-25GB flowing to the control
node. The README should mention this and recommend running from a host
with sufficient bandwidth to the fleet.

### Decision 9: Ansible conventions

**Agree.**

FQCN everywhere, `inspectah_` prefix, `examples/` directory, `.ansible-lint`
config, `meta/argument_specs.yml` -- all correct. The Birch consult
aligned with Galaxy standards.

One upgrade: add `meta/argument_specs.yml` as a MUST, not a SHOULD.
Without it, the role cannot pass `ansible-lint` at `production` profile
and Galaxy quality scoring penalizes its absence.

---

## 3. Repository Structure

```
ansible-role-inspectah/
├── defaults/
│   └── main.yml                 # Public variable API (inspectah_ prefix)
├── vars/
│   └── main.yml                 # Internal constants (not user-configurable)
├── handlers/
│   └── main.yml                 # (reserved for future use)
├── tasks/
│   ├── main.yml                 # Entry point: include_tasks dispatcher
│   ├── preflight.yml            # Prerequisite and version validation
│   ├── install.yml              # COPR or RPM install (conditional)
│   ├── scan.yml                 # Run inspectah scan + container cleanup
│   ├── fetch.yml                # Pull tarball to control node + tarball cleanup
│   └── host_cleanup.yml         # Uninstall inspectah + remove COPR repo (conditional)
├── meta/
│   ├── main.yml                 # Galaxy metadata, dependencies, platforms
│   └── argument_specs.yml       # Variable type validation
├── molecule/
│   ├── default/
│   │   ├── molecule.yml         # Default scenario config
│   │   ├── converge.yml         # Convergence playbook
│   │   ├── verify.yml           # Verification playbook
│   │   └── prepare.yml          # Pre-test setup
│   └── air_gapped/
│       ├── molecule.yml         # Air-gapped (RPM push) scenario
│       ├── converge.yml
│       └── verify.yml
├── tests/
│   ├── inventory                # Galaxy legacy test inventory
│   └── test.yml                 # Galaxy legacy test playbook
├── examples/
│   ├── site.yml                 # Full pipeline: scan fleet + aggregate
│   ├── scan_only.yml            # Scan without aggregate
│   └── inventory.ini            # Example inventory with scan_targets group
├── .ansible-lint                # Linter config (production profile)
├── .yamllint                    # YAML linter config
├── .github/
│   └── workflows/
│       ├── lint.yml             # ansible-lint + yamllint
│       └── molecule.yml         # Molecule test matrix
├── README.md
├── CHANGELOG.md
├── LICENSE                      # MIT (matching inspectah)
└── requirements.yml             # Collection dependencies
```

---

## 4. Variable Interface

### 4.1 Public Variables (defaults/main.yml)

```yaml
---
# ============================================================
# ansible-role-inspectah default variables
# All variables are prefixed with inspectah_ to avoid collisions.
# ============================================================

# --- Installation ---

# Whether to install inspectah as part of the role.
# Set to true if inspectah is not pre-installed on targets.
inspectah_install: false

# Installation method: "copr" (COPR repo) or "rpm" (push local RPM).
#
# Trust model:
#   copr — installs from the mrussell/inspectah COPR repository.
#          dnf handles GPG signature verification via the COPR-managed
#          repo signing key. Same trust model as any COPR package.
#   rpm  — copies a local RPM from the control node to the target
#          and installs it with dnf. The role does NOT verify the
#          RPM's provenance; the operator is responsible for ensuring
#          the RPM is authentic. Use this path only for pre-validated
#          artifacts (air-gapped environments, internal build systems).
inspectah_install_method: "copr"

# COPR repository identifier. Used when inspectah_install_method is "copr".
inspectah_copr_repo: "mrussell/inspectah"

# Path to a local .rpm file on the control node.
# Used when inspectah_install_method is "rpm".
# The RPM is copied to the target and installed with dnf.
# NOTE: The role trusts this RPM unconditionally. Ensure provenance
# before pointing this variable at an artifact.
inspectah_rpm_path: ""

# Version to install. Empty string means "latest".
# Example: "0.8.6~beta.5" (use RPM tilde convention)
# Use this to pin a consistent version across the fleet.
inspectah_install_version: ""

# --- Scan ---

# Target base image for cross-distro conversion.
# Example: "registry.redhat.io/rhel9/rhel-bootc:9.6"
# For fleet consistency, prefer digest-pinned references:
#   "registry.redhat.io/rhel9/rhel-bootc@sha256:abc123..."
# Tags can resolve differently across hosts if updated mid-campaign.
# Leave empty for same-distro assessment (most common).
inspectah_base_image: ""

# Preserve sensitive data categories in the snapshot.
# Valid items: password-hashes, ssh-keys, subscription, all
# Implies --ack-sensitive automatically.
inspectah_preserve: []

# Skip the redaction phase (secrets remain unmasked in output).
# Implies --ack-sensitive automatically.
inspectah_no_redaction: false

# Output file path for the scan tarball on the target host.
# inspectah's -o flag takes a FILE PATH, not a directory.
# The role passes this path directly to `inspectah scan -o <path>`.
# Parent directories are created automatically.
inspectah_scan_output: "/var/lib/inspectah/scans/{{ inventory_hostname }}.tar.gz"

# Async timeout for the scan task in seconds.
# Cold scans with large base images may need 15+ minutes.
inspectah_scan_timeout: 900

# Poll interval in seconds for async scan task.
inspectah_scan_poll: 30

# Additional scan flags as a list of strings.
# Example: ["--verbose", "--no-baseline"]
#
# SAFETY: The role strips any flags that conflict with its own
# invariants (-o, --progress, --ack-sensitive, --base-image,
# --preserve, --no-redaction). These are managed by the role's
# own variables and must not be overridden. A preflight warning
# is emitted if conflicting flags are detected.
inspectah_extra_args: []

# --- Fetch ---

# Campaign identifier for grouping fetched tarballs into a single
# directory. When set, tarballs are fetched into
# {{ inspectah_fetch_dest }}/{{ inspectah_campaign_id }}/.
# When empty (default), tarballs are fetched directly into
# {{ inspectah_fetch_dest }}/{{ inventory_hostname }}.tar.gz with
# no campaign subdirectory — suitable for simple single-run scans.
#
# The example playbook (examples/site.yml) demonstrates generating
# a campaign ID in a dedicated Play 0 on localhost and passing it
# to the role. This is the recommended pattern for fleet scans
# with serial batching. See Section 5.5 for details.
#
# NOTE: Do not use run_once to generate this value inside the
# fleet play. In Ansible, run_once fires once per serial batch,
# not once per play — a fleet run with serial: 10 would split
# tarballs across multiple campaign directories.
inspectah_campaign_id: ""

# Base directory on the control node to receive per-host tarballs.
# When inspectah_campaign_id is set, a subdirectory is created
# under this path. When empty, tarballs land directly here.
inspectah_fetch_dest: "{{ playbook_dir }}/scans"

# --- Cleanup ---

# Delete per-host tarball from target after successful fetch.
inspectah_cleanup_host_tarball: true

# Best-effort sweep of orphan inspectah-baseline-* containers.
# Cleans up containers left by crash-killed inspectah processes
# (SIGKILL, OOM-kill, host reboot during scan).
#
# WARNING: This sweeps ALL containers matching the inspectah-baseline-
# prefix. It is NOT safe for hosts running concurrent inspectah scans.
# Enable only when you are certain no other scan is in flight.
# inspectah's internal CleanupGuard handles normal cleanup.
inspectah_cleanup_orphan_containers: false

# Remove inspectah and COPR repo from the target after the campaign.
# Only acts if inspectah_install is true — the role cleans up what
# it brought, but never removes pre-existing installs.
# When enabled:
#   - Uninstalls the inspectah RPM (if inspectah_install was true)
#   - Disables/removes the COPR repo (if the role enabled it)
# When disabled (default): leaves inspectah installed for future scans.
inspectah_cleanup_host: false
```

### 4.2 Internal Constants (vars/main.yml)

```yaml
---
# Internal constants. Not part of the public API.
# Do not override these in inventory or playbooks.

# Progress mode forced for non-interactive Ansible execution.
_inspectah_progress_mode: "flat"

# Container name prefix used by inspectah for baseline containers.
# Used by the orphan janitor (inspectah_cleanup_orphan_containers).
_inspectah_baseline_container_prefix: "inspectah-baseline-"

# Minimum inspectah version this role release supports.
# Preflight compares `inspectah --version` output against this.
_inspectah_min_version: "0.8.0"

# Flags that inspectah_extra_args must not override.
# Preflight strips these with a warning if present.
_inspectah_reserved_flags:
  - "-o"
  - "--output"
  - "--progress"
  - "--ack-sensitive"
  - "--acknowledge-sensitive"
  - "--base-image"
  - "--preserve"
  - "--no-redaction"

# Whether this campaign handles sensitive data (auto-derived).
# True when inspectah_preserve is non-empty or inspectah_no_redaction
# is true. Used to tighten fetch directory permissions.
_inspectah_sensitive_campaign: >-
  {{ (inspectah_preserve | length > 0) or (inspectah_no_redaction | bool) }}
```

### 4.3 Argument Specification (meta/argument_specs.yml)

```yaml
---
argument_specs:
  main:
    short_description: >-
      Run inspectah scan on target hosts and fetch result tarballs
      to the control node.
    description:
      - >-
        Optionally installs inspectah via COPR or local RPM, runs
        a migration scan, fetches the resulting tarball to the
        control node, and cleans up containers and host artifacts.
      - >-
        The role does NOT run inspectah aggregate. See the example
        playbook for the full scan-fetch-aggregate pipeline.
    options:
      inspectah_install:
        type: bool
        default: false
        description: Whether to install inspectah on the target host.

      inspectah_install_method:
        type: str
        default: copr
        choices:
          - copr
          - rpm
        description: >-
          Installation method. "copr" enables the COPR repo and
          installs via dnf (GPG-verified). "rpm" copies a local RPM
          to the target (operator-trusted, no provenance check).

      inspectah_copr_repo:
        type: str
        default: mrussell/inspectah
        description: COPR repository identifier.

      inspectah_rpm_path:
        type: path
        default: ""
        description: >-
          Path to a local .rpm file on the control node. Required
          when inspectah_install_method is "rpm". The role does not
          verify this RPM's provenance.

      inspectah_install_version:
        type: str
        default: ""
        description: >-
          Specific version to install. Empty means latest. Use to
          pin a consistent version across the fleet.

      inspectah_base_image:
        type: str
        default: ""
        description: >-
          Target base image for cross-distro conversion. Leave
          empty for same-distro assessment.

      inspectah_preserve:
        type: list
        elements: str
        default: []
        description: >-
          Sensitive data categories to preserve in the snapshot.
        choices:
          - password-hashes
          - ssh-keys
          - subscription
          - all

      inspectah_no_redaction:
        type: bool
        default: false
        description: >-
          Skip the redaction phase. Secrets remain unmasked in
          output. Automatically adds --ack-sensitive.

      inspectah_scan_output:
        type: str
        default: "/var/lib/inspectah/scans/{{ inventory_hostname }}.tar.gz"
        description: >-
          Output file path for the scan tarball on the target host.
          inspectah's -o flag takes a file path, not a directory.
          Parent directories are auto-created.

      inspectah_scan_timeout:
        type: int
        default: 900
        description: >-
          Async timeout for the scan task in seconds.

      inspectah_scan_poll:
        type: int
        default: 30
        description: >-
          Poll interval in seconds for the async scan task.

      inspectah_extra_args:
        type: list
        elements: str
        default: []
        description: >-
          Additional CLI flags passed to inspectah scan. Must not
          include -o, --progress, --ack-sensitive, --base-image,
          --preserve, or --no-redaction (these are managed by the
          role's own variables and stripped with a warning).

      inspectah_campaign_id:
        type: str
        default: ""
        description: >-
          Campaign identifier for grouping fetched tarballs. When
          set, tarballs are fetched into a subdirectory named by
          this value. When empty (default), tarballs land directly
          in inspectah_fetch_dest. The example playbook generates
          this in a separate Play 0 on localhost — do not use
          run_once inside a serial-batched play (it fires per
          batch, not per play).

      inspectah_fetch_dest:
        type: path
        default: "{{ playbook_dir }}/scans"
        description: >-
          Base directory on the control node for fetched tarballs.
          When inspectah_campaign_id is set, a subdirectory is
          created under this path.

      inspectah_cleanup_host_tarball:
        type: bool
        default: true
        description: >-
          Remove the scan tarball from the target host after
          successful fetch.

      inspectah_cleanup_orphan_containers:
        type: bool
        default: false
        description: >-
          Best-effort sweep of orphan inspectah-baseline-* containers.
          NOT safe for concurrent scans. inspectah's internal cleanup
          handles normal cases; enable only for crash leftovers.

      inspectah_cleanup_host:
        type: bool
        default: false
        description: >-
          Remove inspectah and COPR repo from the target after the
          campaign. Only acts when inspectah_install is true.
```

---

## 5. Task Flow

### 5.1 Entry Point (tasks/main.yml)

```yaml
---
- name: Install inspectah
  ansible.builtin.include_tasks: install.yml
  when: inspectah_install | bool

- name: Preflight checks
  ansible.builtin.include_tasks: preflight.yml

- name: Run inspectah scan
  ansible.builtin.include_tasks: scan.yml

- name: Fetch scan tarball to control node
  ansible.builtin.include_tasks: fetch.yml

- name: Clean up host (uninstall inspectah)
  ansible.builtin.include_tasks: host_cleanup.yml
  when:
    - inspectah_cleanup_host | bool
    - inspectah_install | bool
```

**Ordering rationale:** Install runs before preflight so that preflight
can validate the installed version. Preflight checks both binary
presence AND version compatibility. This means preflight always runs
against the binary the scan will actually use, whether pre-installed
or role-installed.

Orphan container cleanup (opt-in) runs in scan.yml's `always` block
(see 5.6). inspectah's internal `CleanupGuard` handles normal container
cleanup. Tarball cleanup runs inside fetch.yml's success path (see 5.5).
Host cleanup (uninstall) runs at the end after all work is complete.

### 5.2 Preflight (tasks/preflight.yml)

Validates prerequisites before any scan work begins.

**Checks:**
1. `inspectah` binary exists in PATH
2. `inspectah --version` >= `_inspectah_min_version` — compare the
   installed version against the minimum this role supports. Fail with
   a clear message stating both the installed and required versions.
3. `podman` is installed and >= 4.4
4. `nsenter` (from util-linux) is present
5. Target is a supported platform (RHEL/CentOS/Fedora family,
   x86_64/aarch64)
6. When `inspectah_install_method` is `"rpm"`:
   `inspectah_rpm_path` is set and the file exists on the control node
7. Parent directory of `inspectah_scan_output` exists or is creatable
8. **extra_args safety check:** scan `inspectah_extra_args` for any
   flags in `_inspectah_reserved_flags`. If found, emit a warning via
   `ansible.builtin.debug` and strip the conflicting flags from the
   list. This prevents operators from accidentally overriding `-o`,
   `--progress`, `--ack-sensitive`, `--base-image`, `--preserve`, or
   `--no-redaction` — all of which are managed by the role's own
   variables and command construction logic.

**Idempotency:** Read-only checks. No state changes. Safe to re-run.

### 5.3 Install (tasks/install.yml)

**Conditional on:** `inspectah_install | bool`

**COPR path (`inspectah_install_method == "copr"`):**

```
1. Enable COPR repo (idempotent, skipped if already enabled)
   - Prefer community.general.copr module
   - Fallback: ansible.builtin.command with creates: guard
   - Register _inspectah_copr_enabled fact for cleanup tracking
2. Install inspectah package via ansible.builtin.dnf
   - With version pin when inspectah_install_version is set
   - Register _inspectah_pkg_installed fact for cleanup tracking
```

**RPM path (`inspectah_install_method == "rpm"`):**

```
1. Copy RPM from control node to target via ansible.builtin.copy
2. Install via ansible.builtin.dnf with local file path
   - Register _inspectah_pkg_installed fact for cleanup tracking
```

**Provenance model:**

- **COPR:** dnf verifies the RPM's GPG signature against the
  COPR-managed signing key. This is standard dnf behavior for any
  COPR-hosted package. The role does not add or remove trust beyond
  what dnf provides.
- **RPM:** The role copies the file from the control node and installs
  it with `dnf install <local-path>`. dnf does NOT verify provenance
  of local RPMs by default. The operator is responsible for ensuring
  the RPM at `inspectah_rpm_path` is authentic. This path is intended
  for air-gapped environments using pre-validated artifacts from
  internal build systems.

**Idempotency:** dnf install is naturally idempotent. COPR enable uses
`creates: /etc/yum.repos.d/_copr:copr.fedorainfracloud.org:{{ repo }}.repo`
or module-level idempotency.

### 5.4 Scan (tasks/scan.yml)

The core task. Wrapped in `block/always` for container cleanup guarantee.

```yaml
- name: Scan and container cleanup
  block:
    - name: Ensure scan output parent directory exists
      ansible.builtin.file:
        path: "{{ inspectah_scan_output | dirname }}"
        state: directory
        mode: "0750"
      become: true

    - name: Run inspectah scan
      ansible.builtin.command:
        argv: "{{ _inspectah_scan_argv }}"
      become: true
      async: "{{ inspectah_scan_timeout }}"
      poll: "{{ inspectah_scan_poll }}"
      register: inspectah_scan_result
      changed_when: inspectah_scan_result.rc == 0

  always:
    - name: Clean up orphan baseline containers (opt-in)
      ansible.builtin.include_tasks: container_cleanup.yml
      when: inspectah_cleanup_orphan_containers | bool
```

**Command construction (`_inspectah_scan_argv`):**

Built as a list in a `set_fact` task to avoid shell injection:

```
["inspectah", "scan",
 "-o", "{{ inspectah_scan_output }}",
 "--progress", "{{ _inspectah_progress_mode }}"]
+ (["--base-image", "{{ inspectah_base_image }}"]
   when inspectah_base_image != "")
+ (["--preserve", item] for item in inspectah_preserve)
+ (["--no-redaction"] when inspectah_no_redaction)
+ (["--ack-sensitive"]
   when inspectah_preserve | length > 0 or inspectah_no_redaction)
+ inspectah_extra_args  (after reserved-flag stripping)
```

**Critical correctness notes:**

- `-o` receives a **file path** (e.g.,
  `/var/lib/inspectah/scans/webserver01.tar.gz`), NOT a directory.
  inspectah writes the tarball at that exact path, creating parent
  directories as needed. The role knows the exact tarball path at scan
  time — no glob discovery needed downstream.
- `--progress flat` is always forced. Without it, Ansible captures ANSI
  escape sequences that corrupt registered output and break `changed_when`.
- `--ack-sensitive` is added automatically when preserve or no-redaction
  is active. Without it, inspectah exits non-zero and the scan fails.
- `become: true` is required. inspectah scan needs root to read system
  configuration, RPM databases, and manage podman containers.

**Exit code semantics:**

| Exit Code | Meaning | Role Behavior |
|-----------|---------|---------------|
| 0 | Clean scan | Success |
| 0 (degraded) | Trustworthy but with caveats | Success (logged) |
| 2 | Incomplete (inspector failure) | Fail task |
| 130 | User interrupt (SIGINT) | Fail task |
| Non-zero | Other error | Fail task |

**Idempotency:** Scan is not idempotent by nature (it captures system
state at a point in time). Re-running overwrites the tarball at the
same path (because `-o` is deterministic per host). This is acceptable:
the role captures current state, not desired state.

### 5.5 Fetch (tasks/fetch.yml)

The role knows the exact tarball path from `inspectah_scan_output` — no
discovery step is needed.

**Campaign directory derivation:** The fetch destination depends on
whether the consuming playbook provides `inspectah_campaign_id`:

- **With campaign ID** (fleet scans): tarballs go into
  `{{ inspectah_fetch_dest }}/{{ inspectah_campaign_id }}/`. The
  campaign ID is an input variable — the role does not generate it.
  The example playbook (Section 6.1) generates it in a dedicated
  Play 0 on localhost and passes it to the role via `vars`.
- **Without campaign ID** (simple scans): tarballs go directly into
  `{{ inspectah_fetch_dest }}/`. No subdirectory is created.

**Why the role does not generate the campaign ID internally:**

In Ansible, `run_once` fires once per `serial` batch, not once per
play. A fleet run with `serial: 10` and 50 hosts would fire `run_once`
five times, creating five different campaign directories. Tarballs would
scatter across directories and `inspectah aggregate` would see only a
partial fleet per directory.

The correct pattern is a separate play on `localhost` that runs before
the fleet play begins serial batching. `set_fact` on localhost in its
own play is truly play-scoped — it executes exactly once, and all
serial batches in the subsequent fleet play read the same value from
`hostvars['localhost']`. The example playbook demonstrates this.

```yaml
- name: Set fetch directory (with or without campaign scoping)
  ansible.builtin.set_fact:
    _inspectah_run_dir: >-
      {{ (inspectah_campaign_id | default('') | length > 0)
         | ternary(inspectah_fetch_dest ~ '/' ~ inspectah_campaign_id,
                   inspectah_fetch_dest) }}

- name: Ensure fetch directory exists
  ansible.builtin.file:
    path: "{{ _inspectah_run_dir }}"
    state: directory
    mode: "{{ (_inspectah_sensitive_campaign | bool) | ternary('0700', '0755') }}"
  delegate_to: localhost
  run_once: true

- name: Fetch tarball to control node
  ansible.builtin.fetch:
    src: "{{ inspectah_scan_output }}"
    dest: "{{ _inspectah_run_dir }}/{{ inventory_hostname }}.tar.gz"
    flat: true
    fail_on_missing: true
  become: true

- name: Remove tarball from target host
  ansible.builtin.file:
    path: "{{ inspectah_scan_output }}"
    state: absent
  become: true
  when:
    - inspectah_cleanup_host_tarball | bool
```

**Design notes:**

- **No tarball discovery needed.** Because `-o` takes a file path and
  the role constructs it deterministically, the exact tarball location
  is known from the `inspectah_scan_output` variable. No
  `ansible.builtin.find`, no mtime sorting, no glob ambiguity.
- **Campaign scoping is opt-in.** Users who just want a basic
  scan-fetch do not need to set `inspectah_campaign_id` at all.
  Tarballs land directly in `inspectah_fetch_dest`. Campaign scoping
  is a feature of the example playbook for fleet-scale operations.
- **The directory-create task still uses `run_once`.** This is safe
  because `run_once` on a `delegate_to: localhost` task only creates
  the directory — the same directory path is idempotent. Even if
  `run_once` fires per serial batch, the `file` module with
  `state: directory` is idempotent. The critical point is that the
  campaign ID VALUE must be the same across batches, which is
  guaranteed by generating it in a separate play (see Section 6.1).
- **Sensitive campaign permissions.** When `inspectah_preserve` is
  non-empty or `inspectah_no_redaction` is true, `_inspectah_sensitive_campaign`
  is true and the fetch directory is created with `0700` permissions.
  This restricts access to fetched tarballs that may contain password
  hashes, SSH keys, or unredacted secrets. Non-sensitive campaigns use
  the default `0755`.
- Renames to `{{ inventory_hostname }}.tar.gz` for aggregate
  consumption. inspectah aggregate accepts a directory of tarballs;
  hostname-based naming provides natural identification.
- `flat: true` avoids the default Ansible fetch behavior of creating
  a `hostname/path/to/file` directory tree.
- Tarball cleanup is in the success path of fetch, NOT in an `always`
  block. A failed fetch must not delete the only copy of the tarball
  on the target.

### 5.6 Container Cleanup Strategy

Container cleanup follows a two-layer model:

1. **Primary:** inspectah's internal `CleanupGuard` (always active)
2. **Role-level:** opt-in orphan janitor (for crash leftovers only)

**Why the role does not do invocation-scoped cleanup:**

inspectah creates baseline containers named `inspectah-baseline-<unix_ts>`
where the timestamp is generated internally at scan time
(`SystemTime::now()` in `crates/collect/src/baseline.rs`). The role
cannot precompute or predict this name from outside. inspectah's own
`CleanupGuard` (a Rust `Drop` impl) runs `podman rm -f <name>` when
the scan function returns or panics, with explicit cleanup on the
success path and best-effort cleanup via the guard's destructor on
failure. This is the primary cleanup contract.

The role's `always` block in scan.yml is therefore **not** for
invocation-scoped cleanup — that is inspectah's responsibility. The
`always` block exists solely for the orphan janitor described below.

**Orphan janitor (opt-in):**

Crash-killed inspectah processes (SIGKILL, OOM-kill, host reboot during
scan) can leave stale `inspectah-baseline-*` containers. The role
provides an opt-in janitor to clean these up:

```yaml
# vars/main.yml addition (see Section 4.2)
# _inspectah_baseline_container_prefix: "inspectah-baseline-"
```

```yaml
# defaults/main.yml addition (see Section 4.1)
# Whether to clean up orphan inspectah-baseline-* containers.
# This sweeps ALL containers matching the prefix — it is NOT safe
# for hosts running concurrent inspectah scans. Enable only when
# you are certain no other scan is in flight on the same host.
inspectah_cleanup_orphan_containers: false
```

```yaml
# In scan.yml's always block:
- name: Clean up orphan baseline containers
  when: inspectah_cleanup_orphan_containers | bool
  block:
    - name: List orphan baseline containers
      ansible.builtin.command:
        argv:
          - podman
          - ps
          - --all
          - --filter
          - "name={{ _inspectah_baseline_container_prefix }}"
          - --format
          - "{{ '{{' }}.Names{{ '}}' }}"
      register: _inspectah_orphan_containers
      changed_when: false
      become: true
      failed_when: false

    - name: Remove orphan baseline containers
      ansible.builtin.command:
        argv:
          - podman
          - rm
          - --force
          - "{{ item }}"
      loop: "{{ _inspectah_orphan_containers.stdout_lines | default([]) }}"
      become: true
      changed_when: true
      when: _inspectah_orphan_containers.stdout_lines | default([]) | length > 0
```

**Concurrency safety:** This janitor removes ALL containers matching
the `inspectah-baseline-` prefix. It is explicitly unsafe for hosts
running concurrent inspectah scans — a parallel scan's container will
be killed. The variable defaults to `false` and the README documents
this constraint. For concurrent-scan environments, rely solely on
inspectah's internal `CleanupGuard`.

**Future improvement:** When inspectah exposes a stable, externally
predictable container identifier (e.g., via a CLI flag or output file),
the role can switch to invocation-scoped cleanup. Until then, the
internal `CleanupGuard` is the correct primary mechanism.
Track this as a coupling point (see Open Question 4).

### 5.7 Host Cleanup (tasks/host_cleanup.yml)

**Conditional on:** `inspectah_cleanup_host | bool` AND
`inspectah_install | bool`

The "leave no trace" option for production environments. Cleans up what
the role brought, but NEVER removes pre-existing installs.

```yaml
- name: Uninstall inspectah
  ansible.builtin.dnf:
    name: inspectah
    state: absent
  become: true
  when: _inspectah_pkg_installed | default(false) | bool

- name: Remove COPR repo
  ansible.builtin.command:
    argv:
      - dnf
      - copr
      - disable
      - "{{ inspectah_copr_repo }}"
      - --yes
  become: true
  when:
    - _inspectah_copr_enabled | default(false) | bool
    - inspectah_install_method == "copr"
  changed_when: true

- name: Remove staged RPM from target
  ansible.builtin.file:
    path: "/tmp/inspectah.rpm"
    state: absent
  become: true
  when:
    - _inspectah_pkg_installed | default(false) | bool
    - inspectah_install_method == "rpm"
```

**Guard logic:** The `_inspectah_pkg_installed` and
`_inspectah_copr_enabled` facts are set by install.yml. If the role
did not install inspectah (because `inspectah_install: false` or
the package was already present), these facts are false/undefined and
the cleanup tasks are skipped. This ensures the role never removes
an inspectah installation it did not create.

**RPM path cleanup:** When the role installed via RPM push, the
staged `/tmp/inspectah.rpm` file is also removed. The RPM install path
in install.yml copies the RPM to a well-known temp path on the target;
this cleanup step removes that artifact.

---

## 6. Example Playbook Design

### 6.1 Full Pipeline (examples/site.yml)

```yaml
---
# Full inspectah fleet scan pipeline.
#
# Three-play structure:
#   Play 0: Generate a campaign ID on localhost (runs exactly once)
#   Play 1: Scan the fleet (serial batching is safe — campaign ID
#           comes from Play 0, not run_once)
#   Play 2: Aggregate results on localhost
#
# Why three plays instead of two?
#   Ansible's run_once fires once per serial BATCH, not once per
#   play. With serial: 10 and 50 hosts, run_once in Play 1 fires
#   5 times — creating 5 campaign directories and splitting tarballs
#   across them. set_fact on localhost in a separate play is truly
#   play-scoped: it runs exactly once before serial batching begins,
#   and all batches read the same value from hostvars['localhost'].
#
# Fleet-scale tuning (Play 1):
#   serial: 10          — process 10 hosts at a time to avoid
#                          thundering herd on base image pulls
#   max_fail_percentage: 10  — tolerate up to 10% host failures
#                                without aborting the campaign

# Play 0: Generate campaign ID (truly once, before any serial batching)
- name: Initialize campaign
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Generate campaign ID
      ansible.builtin.set_fact:
        inspectah_campaign_id: "{{ lookup('pipe', 'date +%Y%m%dT%H%M%S') }}"

# Play 1: Scan the fleet (serial batching is safe — campaign ID comes from Play 0)
- name: Scan fleet hosts
  hosts: scan_targets
  # serial: 10
  # max_fail_percentage: 10
  become: false
  vars:
    inspectah_campaign_id: "{{ hostvars['localhost'].inspectah_campaign_id }}"
  roles:
    - role: ansible-role-inspectah
      vars:
        inspectah_install: true
        inspectah_install_method: copr    # Preferred: GPG-verified via COPR
        # inspectah_install_version: "0.8.6~beta.5"  # Pin version across fleet
        # inspectah_base_image: "registry.redhat.io/rhel9/rhel-bootc:9.6"
        # inspectah_preserve:
        #   - subscription
        # inspectah_cleanup_host: true    # Remove inspectah after scan

# Play 2: Aggregate on control node
- name: Aggregate scan results
  hosts: localhost
  connection: local
  gather_facts: false
  vars:
    # inspectah aggregate needs the same minimum version as the role.
    _inspectah_min_version: "0.8.0"
    # The campaign directory is named by the campaign ID from Play 0.
    _aggregate_input: "{{ playbook_dir }}/scans/{{ inspectah_campaign_id }}"
    _aggregate_output: "{{ playbook_dir }}/aggregate"
  tasks:
    - name: Check localhost inspectah version
      ansible.builtin.command:
        argv:
          - inspectah
          - --version
      register: _localhost_inspectah_version
      changed_when: false

    - name: Assert localhost inspectah meets minimum version
      ansible.builtin.assert:
        that:
          - >-
            _localhost_inspectah_version.stdout
            | regex_search('[0-9]+\.[0-9]+\.[0-9]+')
            is version(_inspectah_min_version, '>=')
        fail_msg: >-
          localhost inspectah version
          {{ _localhost_inspectah_version.stdout }} is below the
          minimum required {{ _inspectah_min_version }}. Upgrade
          inspectah on the control node before running aggregate.

    - name: Verify campaign directory exists
      ansible.builtin.stat:
        path: "{{ _aggregate_input }}"
      register: _campaign_dir

    - name: Fail if campaign directory missing
      ansible.builtin.fail:
        msg: >-
          Campaign directory {{ _aggregate_input }} not found.
          Run the scan play first.
      when: not _campaign_dir.stat.exists

    - name: Ensure aggregate output directory exists
      ansible.builtin.file:
        path: "{{ _aggregate_output }}"
        state: directory
        mode: "0755"

    - name: Run inspectah aggregate
      ansible.builtin.command:
        argv:
          - inspectah
          - aggregate
          - "{{ _aggregate_input }}"
          - --output-dir
          - "{{ _aggregate_output }}"
          # When scan used --preserve or --no-redaction, aggregate
          # also needs --ack-sensitive to process sensitive tarballs.
          # Uncomment the next line if your scan campaign preserved
          # sensitive data:
          # - --ack-sensitive
      changed_when: true
```

**Three-play rationale:** The `set_fact` on localhost in Play 0 is
truly play-scoped — it executes exactly once, before Play 1 begins
serial batching. Every serial batch in Play 1 reads the same
`inspectah_campaign_id` from `hostvars['localhost']`, guaranteeing
all tarballs land in a single campaign directory. This is not possible
with `run_once` inside Play 1 (see the comment in the playbook above).

**Partial-campaign behavior:** When `max_fail_percentage` allows some
hosts to fail, the aggregate play runs on whatever tarballs were
successfully fetched. This is the correct behavior: `inspectah aggregate`
processes whatever tarballs it finds in the campaign directory. The
aggregate output documents which hosts are included. Operators who need
all-or-nothing semantics should set `max_fail_percentage: 0` (the
Ansible default) and not use the aggregate play on partial results.

**Note on `--ack-sensitive` propagation:** If the scan play used
`inspectah_preserve` or `inspectah_no_redaction`, the resulting tarballs
contain sensitive data. `inspectah aggregate` requires `--ack-sensitive`
to process such tarballs. The example shows this as a comment. In
production playbooks, use a variable or conditional to propagate this
flag automatically.

### 6.2 Scan Only (examples/scan_only.yml)

Runs the role without the aggregate play. Useful for collecting
tarballs to process later or on a different machine.

### 6.3 Example Inventory (examples/inventory.ini)

```ini
[scan_targets]
webserver01.example.com
webserver02.example.com
dbserver01.example.com

[scan_targets:vars]
ansible_user=ansible
ansible_become=true
```

### 6.4 RPM Push Example (examples/scan_rpm.yml)

Shows the RPM install path. Deliberately positioned as the exception
path, not the primary example.

```yaml
---
# Air-gapped / RPM push example.
# Use this when targets cannot reach COPR (air-gapped, restricted networks).
#
# Prerequisites:
#   - The RPM at inspectah_rpm_path has been independently verified
#     for provenance (checksum, signature, build source).
#   - The role does NOT verify RPM provenance — that is the operator's
#     responsibility.

- name: Scan fleet hosts (RPM install)
  hosts: scan_targets
  become: false
  roles:
    - role: ansible-role-inspectah
      vars:
        inspectah_install: true
        inspectah_install_method: rpm
        inspectah_rpm_path: "/path/to/inspectah-0.8.6-1.el9.x86_64.rpm"
        inspectah_cleanup_host: true    # Clean up after scan in air-gapped env
```

---

## 7. Galaxy Metadata (meta/main.yml)

```yaml
---
galaxy_info:
  author: Mark Russell
  description: >-
    Run inspectah migration analysis across a fleet of RHEL, CentOS,
    or Fedora hosts. Produces per-host snapshot tarballs ready for
    aggregate analysis.
  license: MIT
  min_ansible_version: "2.14"
  platforms:
    - name: EL
      versions:
        - "9"
        - "10"
    - name: Fedora
      versions:
        - "40"
        - "41"
  galaxy_tags:
    - migration
    - rhel
    - bootc
    - scanning
    - audit

dependencies: []
```

---

## 8. Supported Platform Matrix

### 8.1 Tiered Support

The role supports three tiers. The tier determines CI investment and
the role's response when running on that platform.

| Tier | Meaning | CI Gate | Preflight |
|------|---------|---------|-----------|
| **Release-blocking** | Molecule + real-host CI proves it | PR merge blocked on failure | Pass |
| **Smoke-tested** | Real-host gate proves scan/fetch works | Nightly or manual | Pass |
| **Best-effort** | Should work, not CI-gated | None | Pass (with advisory note) |

### 8.2 Platform Support Table

| Distribution | Versions | Architectures | Tier | Notes |
|-------------|----------|---------------|------|-------|
| CentOS Stream | 9 | x86_64 | Release-blocking | Molecule + real-host smoke in CI |
| RHEL | 9 | x86_64 | Smoke-tested | Manual validation, promote to release-blocking at Galaxy graduation |
| RHEL | 9 | aarch64 | Smoke-tested | When aarch64 CI runner available; promote at Galaxy graduation |
| RHEL | 10 | x86_64, aarch64 | Smoke-tested | COPR builds available, needs real-host validation |
| CentOS Stream | 9 | aarch64 | Smoke-tested | When aarch64 CI runner available |
| CentOS Stream | 10 | x86_64 | Smoke-tested | Tracks RHEL 10 |
| Fedora | 40, 41 | x86_64 | Best-effort | Latest 2 releases, COPR builds available |
| AlmaLinux | 9 | x86_64, aarch64 | Best-effort | RHEL rebuild, should work, not CI-gated |
| Rocky Linux | 9 | x86_64, aarch64 | Best-effort | RHEL rebuild, should work, not CI-gated |

**Release-blocking rationale:** Only CentOS Stream 9 x86_64 is
release-blocking because that is what CI actually proves (Molecule
container tests + real-host smoke). RHEL 9 and aarch64 are promoted
to release-blocking at Galaxy graduation when CI infrastructure for
those platforms is in place. Claiming release-blocking status without
CI enforcement is empty.

**EL8 note:** EL8 is excluded from the support matrix. inspectah requires
podman >= 4.4, which is not available in the default EL8 repos. EL8 is
EOL (RHEL 8 Maintenance Support ends 2029 but active development has
shifted to EL9+). The preflight check rejects EL8 with a clear message.

### 8.3 Prerequisites on target hosts

| Dependency | Minimum Version | Provided By |
|-----------|----------------|-------------|
| podman | 4.4 | System packages |
| nsenter | (any) | util-linux |
| dnf | (any) | System default |

### 8.4 Control node requirements

| Dependency | Version | Purpose | Enforced |
|-----------|---------|---------|----------|
| Ansible | >= 2.14 | Role execution | Galaxy metadata |
| inspectah | >= 0.8.0 | Aggregate command (localhost, example playbook) | Assert task in example aggregate play |
| community.general | >= 5.0 | `community.general.copr` module (optional) | Fallback if absent |

The example aggregate play includes an `ansible.builtin.assert` task
that checks the localhost inspectah version against `_inspectah_min_version`
and fails with a clear message if the version is too old. This is not
"just documented" — it is enforced at runtime.

### 8.5 Role-to-inspectah compatibility

This role release supports inspectah >= 0.8.0. The `_inspectah_min_version`
constant in `vars/main.yml` encodes this. When a new inspectah release
changes CLI flags, output format, or exit codes in a way that affects
the role, bump `_inspectah_min_version` and release a new role version.

**Target-side:** The preflight check enforces this: if the installed
inspectah version is below `_inspectah_min_version`, the role fails
with a message stating the required and installed versions.

**Control-side:** The example aggregate play enforces the same minimum
via an assert task. Consuming playbooks that diverge from the example
should replicate this check.

---

## 9. Failure Handling Strategy

### 9.1 Preflight Failures

Fail fast with a clear message. No partial state to clean up.

| Check | Failure Mode | Action |
|-------|-------------|--------|
| inspectah not found | Binary missing, install not requested | `ansible.builtin.fail` with install instructions |
| inspectah version too old | Below `_inspectah_min_version` | `ansible.builtin.fail` with installed vs. required versions |
| podman < 4.4 | Version too old | `ansible.builtin.fail` with upgrade instructions |
| nsenter missing | util-linux not installed | `ansible.builtin.fail` with package name |
| Unsupported OS | Not RHEL/CentOS/Fedora family | `ansible.builtin.fail` with platform list |
| RPM path empty | Method is "rpm" but no path | `ansible.builtin.fail` with variable name |
| Reserved flags in extra_args | Operator passed -o/--progress/etc. | Warning + strip (non-fatal) |

### 9.2 Scan Failures

| Failure | Detection | Action |
|---------|----------|--------|
| Timeout | async exceeds `inspectah_scan_timeout` | Task fails, container cleanup still runs |
| Exit code 2 | Incomplete scan | Task fails, tarball may exist but is partial |
| Exit code 130 | SIGINT | Task fails |
| No tarball produced | fetch `fail_on_missing: true` | Explicit fail with diagnostic message |
| Disk full | inspectah error | Task fails with inspectah stderr |

### 9.3 Fetch Failures

| Failure | Detection | Action |
|---------|----------|--------|
| Tarball missing | `fail_on_missing: true` | Task fails |
| Disk full on control | fetch module error | Task fails |
| Permission denied | fetch module error | Task fails |

### 9.4 Cleanup Resilience

**Primary container cleanup** is inspectah's internal `CleanupGuard`.
If inspectah exits normally (success or handled error), the guard
removes the baseline container. The role does not duplicate this.

**Orphan janitor** (opt-in via `inspectah_cleanup_orphan_containers`)
runs in scan.yml's `always` block. Individual container removal failures
are non-fatal (`failed_when: false` on the list step, individual `rm`
failures logged but do not abort).

### 9.5 Failure Recovery and Resume

**What is safe to retry after partial failure:**

- **Scan failed on some hosts:** Re-run the playbook with
  `--limit @playbook.retry` to target only failed hosts. Each
  re-invocation of the playbook generates a new campaign ID in
  Play 0, creating a new campaign directory — results from
  different attempts do not mix.
- **Fetch failed (scan succeeded):** The tarball remains on the target
  host (tarball cleanup only runs on successful fetch). Re-running
  the role re-scans the host (scan is not idempotent — it captures
  current state). If the original tarball is needed without re-scanning,
  use `ansible.builtin.fetch` manually.
- **Aggregate failed:** Safe to re-run. Aggregate is a stateless
  operation — same inputs produce same outputs.

**What is NOT safe to blindly retry:**

- **Concurrent re-runs on the same host:** The role writes to a
  deterministic tarball path (`inspectah_scan_output`). Two concurrent
  scans on the same host race on this file. Use `serial: 1` or
  `forks: 1` per host to prevent this.

**Campaign isolation:** When using the example playbook's three-play
pattern, each `ansible-playbook` invocation generates a unique campaign
ID in Play 0 and writes all tarballs to its own timestamped campaign
directory. The aggregate play (Play 2) points directly at that campaign
directory via the same `inspectah_campaign_id` fact, so there is no
ambiguity about which tarballs to aggregate.

---

## 10. Fleet-Scale Considerations

### 10.1 Thundering Herd Mitigation

When inspectah scans without a cached base image, it pulls the image
from a container registry. Scanning 500 hosts simultaneously creates
500 concurrent pulls against the same registry.

**Mitigation:** The consuming playbook (not the role) sets `serial: N`
to batch hosts. Recommended starting point: `serial: 10`. The example
playbook includes this as a commented option.

**Alternative for large fleets:** Pre-pull the base image via a
separate play or role before running inspectah. This converts all scans
to "cached" mode (30-60s instead of 3-5 minutes).

### 10.2 Disk Space

| Location | Size Per Host | Notes |
|----------|-------------|-------|
| Target: base image cache | 500MB - 1GB | Persists across runs |
| Target: scan tarball | 5 - 50MB | Cleaned by role if enabled |
| Control: fetched tarballs | 5 - 50MB | Persists for aggregate |
| Control: aggregate output | 10 - 100MB | Grows with fleet size |

### 10.3 Network Bandwidth

Fetch transfers tarballs from every target to the control node. At
500 hosts with 20MB average tarballs, that is 10GB of transfer. Run
the control node topologically close to the fleet or use a bastion
host.

### 10.4 Scan Duration Estimates

| Scenario | Duration | Notes |
|----------|----------|-------|
| Cold scan (no cached image) | 3 - 5 minutes | Base image pull dominates |
| Warm scan (cached image) | 30 - 60 seconds | Most fleet re-scans |
| Large host (10k+ packages) | 2 - 3 minutes | Even with cache |

Default timeout is 900 seconds (15 minutes) to accommodate worst-case
cold scans over slow networks.

---

## 11. Galaxy Graduation Path

### Phase 1: GitHub-only (initial release)

1. Create `ansible-role-inspectah` repository on GitHub
2. Include README, LICENSE (MIT), CHANGELOG
3. ansible-lint passing at `production` profile
4. Molecule tests passing (default + air_gapped scenarios)
5. CI: lint + molecule on every PR

### Phase 2: Galaxy import

1. Import role to Galaxy via GitHub integration
2. Galaxy metadata in `meta/main.yml` (see Section 7)
3. Tag releases with semver (Galaxy tracks tags)
4. Quality score target: >= 4.0/5.0

### Phase 3: Collection consideration (future)

If the role grows to include multiple roles (e.g., a separate role for
inspectah refine hosting), consider packaging as an Ansible collection
(`mrussell.inspectah`). This is not needed for the initial single-role
release.

### Galaxy quality checklist

- [ ] `meta/main.yml` with all required fields
- [ ] `meta/argument_specs.yml` for all variables
- [ ] README with role variables table, example playbook, requirements
- [ ] CHANGELOG following Keep a Changelog
- [ ] LICENSE file present
- [ ] `.ansible-lint` at `production` profile with zero warnings
- [ ] Molecule tests with at least one passing scenario
- [ ] No `ansible.builtin.shell` tasks (use `command` with `argv`)
- [ ] FQCN for all modules
- [ ] No bare variables in `when:` conditions

---

## 12. Testing Strategy

### 12.1 Molecule Test Honesty Statement

Molecule scenarios run inside containers. They prove role mechanics but
cannot exercise a real inspectah scan.

**What Molecule proves:**

- Role syntax is valid (`ansible-playbook --syntax-check`)
- Task ordering and variable interpolation work correctly
- Preflight checks pass and fail as expected (both paths)
- Install tasks work (COPR enable, package install, version pinning)
- Idempotency — re-running converge produces no unexpected changes
- Cleanup logic works (create dummy containers, verify removal)
- Fetch task mechanics work (create dummy tarball, verify fetch + rename)
- Host cleanup (uninstall) runs only when gated conditions are met
- extra_args safety stripping works

**What Molecule cannot prove:**

- Full inspectah scan (requires real host, not container — needs root,
  podman with container storage, real filesystem)
- Actual base image pulls over the network
- Real tarball content validity
- Scan duration / timeout behavior under real workloads
- Container cleanup scoping with real inspectah-created containers

**Real-host smoke tests** (Section 12.3) cover the gaps Molecule cannot.

### 12.2 Molecule Scenarios

**Default scenario:** COPR install + scan stub + fetch

```yaml
# molecule/default/molecule.yml
---
dependency:
  name: galaxy
driver:
  name: podman  # or docker
platforms:
  - name: inspectah-test-el9
    image: registry.access.redhat.com/ubi9/ubi:latest
    command: /usr/sbin/init
    privileged: true
    pre_build_image: true
provisioner:
  name: ansible
  inventory:
    group_vars:
      all:
        inspectah_install: true
        inspectah_install_method: copr
verifier:
  name: ansible
```

The converge playbook stubs the scan by creating a dummy tarball at
`inspectah_scan_output`, allowing fetch and cleanup tasks to exercise
their real logic against a known artifact.

**Air-gapped scenario:** RPM push install

Tests the `inspectah_install_method: rpm` path with a pre-built RPM
copied into the test container. Validates the RPM trust boundary
documentation is accurate (no GPG check on local RPMs).

**Fallback scenario:** Tests behavior when `community.general` collection
is NOT installed. Validates the COPR enable fallback to
`ansible.builtin.command` with `creates:` guard.

### 12.3 CI Pipeline (.github/workflows/)

**Lint workflow (every PR) — fast:**
- ansible-lint at `production` profile
- yamllint
- ansible-playbook --syntax-check on all example playbooks

**Molecule workflow (every PR, release-blocking) — medium:**
- Molecule default scenario on CentOS Stream 9 x86_64
- Molecule air_gapped scenario on CentOS Stream 9 x86_64
- Molecule fallback scenario (no community.general)
- Matrix: ansible-core 2.14, 2.15, 2.16, 2.17

**Real-host smoke test (nightly / manual trigger, release-blocking) — slow:**
- Provisions an actual VM (CentOS Stream 9 x86_64)
- Runs the full scan-fetch-aggregate pipeline with a real inspectah binary
- Validates tarball content and aggregate output
- Covers gaps Molecule cannot: real scan, base image pull, container
  lifecycle, tarball content validation

**Galaxy graduation gates (not release-blocking for v1):**
- RHEL 9 x86_64 real-host smoke (manual until CI infra exists)
- CentOS Stream 9 / RHEL 9 aarch64 (when runner available)
- These graduate to release-blocking once CI proves them automatically

**Validation cadence split rationale:** Lint and syntax checks run in
seconds. Molecule scenarios run in 2-5 minutes. Real-host smoke tests
run in 10-20 minutes and require VM infrastructure. Separating them
prevents slow tests from blocking fast feedback on PRs while still
catching integration issues nightly.

---

## 13. Collection Dependencies

```yaml
# requirements.yml
---
collections:
  - name: community.general
    version: ">=5.0.0"
```

`community.general` provides the `community.general.copr` module for
idempotent COPR repo management. It is a soft dependency: the role
falls back to `ansible.builtin.command` with a `creates:` guard if the
collection is not installed.

The `containers.podman` collection is NOT listed as a dependency.
Container cleanup uses `ansible.builtin.command` with `podman` CLI
directly. This avoids requiring a collection that many control nodes
don't have installed, and the cleanup logic is simple enough that
the podman CLI is adequate.

---

## 14. Security Considerations

### 14.1 Sensitive Data Flow

inspectah scans can capture sensitive system data. The role's default
configuration redacts secrets, but operators can disable redaction via
`inspectah_preserve` or `inspectah_no_redaction`.

**Control points:**
- `--ack-sensitive` is added automatically (not manually) to prevent
  accidental exposure. Users opt in via preserve/no-redaction variables.
- Tarballs are fetched over the Ansible control channel (SSH by default).
  No additional transport encryption needed.
- Host tarballs are deleted after fetch by default
  (`inspectah_cleanup_host_tarball: true`).
- `--ack-sensitive` must also be passed to `inspectah aggregate` when
  processing tarballs that contain sensitive data. The example playbook
  documents this linkage.

### 14.2 Privilege Model

- Scan runs as root (`become: true`) because inspectah needs to read
  system configuration and manage podman containers.
- Fetch runs as root to read tarballs written by the scan.
- Cleanup runs as root to remove podman containers.
- The role does NOT configure or modify sudo rules.

### 14.3 Shell Injection Prevention

- All command invocations use `ansible.builtin.command` with `argv`
  list format. No `ansible.builtin.shell` usage.
- `inspectah_extra_args` is a list, not a string, preventing injection
  via variable interpolation.
- Reserved flags are stripped from `extra_args` at preflight time.

### 14.4 Install Source Trust Model

The role supports two install paths with different trust properties:

| Path | Trust Mechanism | Who Verifies |
|------|----------------|-------------|
| COPR (`inspectah_install_method: copr`) | dnf GPG signature check via COPR signing key | dnf (automatic) |
| Local RPM (`inspectah_install_method: rpm`) | None — operator-trusted input | Operator (manual) |

**COPR** is the recommended install path. dnf verifies the RPM signature
against the COPR-managed key, providing the same trust level as any
COPR-hosted package.

**RPM** is the exception path for air-gapped environments. The role
copies the file from `inspectah_rpm_path` on the control node to the
target and installs it with `dnf install <local-path>`. dnf does NOT
verify provenance of local RPMs by default. The operator must ensure
the RPM is authentic before pointing this variable at it. The README,
variable comments, and RPM example playbook all document this trust
boundary.

### 14.5 Base Image Provenance

The role does not verify or pin the base image used by inspectah during
scanning. `inspectah scan --base-image <ref>` pulls whatever the
registry returns for that reference. This is a known trust boundary:
the operator is responsible for pointing `inspectah_base_image` at a
trusted registry and reference.

**Digest pinning for fleet consistency:** Tag-based image references
(e.g., `registry.redhat.io/rhel9/rhel-bootc:9.6`) can resolve to
different digests across hosts if the tag is updated mid-campaign.
For reproducible fleet-wide scans, use digest-pinned references:

```yaml
inspectah_base_image: "registry.redhat.io/rhel9/rhel-bootc@sha256:abc123..."
```

This guarantees every host in the campaign scans against the same image
content. The README variable table should note this recommendation.

For environments requiring image provenance guarantees, use a local
registry mirror with signature verification, or pre-pull and pin by
digest in a separate play before running the role.

---

## 15. Open Questions

1. **AWX / AAP compatibility.** The role uses `{{ playbook_dir }}` for
   fetch destination defaults. AWX execution environments may resolve
   this differently. Needs validation on AAP 2.x.

2. **Pre-pull strategy role.** For large fleets, a separate "pre-pull
   base image" play may be worth packaging. Outside this spec's scope
   but noted as a future enhancement.

3. **Collection vs. role.** If a second role emerges (e.g., inspectah
   refine server hosting), consider migrating to a collection. For now,
   standalone role is correct.

4. **Container name externalization.** inspectah creates baseline
   containers as `inspectah-baseline-<unix_ts>`, with the timestamp
   generated internally at scan time. The role cannot predict this name
   from outside, so invocation-scoped container cleanup is not possible.
   If inspectah adds a flag to accept an externally specified container
   name (e.g., `--container-name <name>`) or emits the name to a
   discoverable location, the role can switch from the prefix-sweep
   orphan janitor to precise invocation-scoped cleanup. Track this as
   a future enhancement.
