# Ansible Role Design Spec: ansible-role-inspectah

**Author:** Pele (Automation & Fleet Configuration Engineer)
**Date:** 2026-06-27
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
2. **Scan** the target host, producing a migration snapshot tarball
3. **Fetch** the tarball to the control node
4. **Clean up** orphaned baseline containers (always, even on failure)

### What the role does NOT do

- Run `inspectah aggregate` (that is a control-node operation, not a
  per-host operation; it belongs in the consuming playbook)
- Run `inspectah refine` (interactive, not automatable)
- Manage container registries or image mirrors
- Configure podman itself (podman >= 4.4 is a prerequisite)

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

1. **`inspectah_scan_output_dir` default should be
   `/var/lib/inspectah/scans`.** The proposed `/var/lib/inspectah` is
   too generic. inspectah scan writes a tarball, not a directory tree.
   A subdirectory avoids collisions with other inspectah state that may
   appear in future versions.

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
   If string is kept for simplicity, document the injection risk.

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

The flow is correct: install -> scan -> fetch -> cleanup. Modifications:

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

4. **Cleanup scope.** The brainstorm says "remove orphaned
   `inspectah-baseline-*` containers." The cleanup task should list
   containers matching the prefix, then remove them. Use
   `containers.podman.podman_container` or fall back to command if the
   collection is not available. The `always` block placement is correct.

5. **Register scan output.** The scan task should register its result
   so the role can expose scan outcome (exit code, tarball path) to the
   consuming playbook via `inspectah_scan_result`.

### Decision 7: Example playbook (site.yml)

**Agree with modification.**

Two-play structure is correct. Modifications:

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

### Decision 8: Fleet-scale concerns

**Agree.**

Collins's consult identified the real fleet-scale issues. The role
addresses them:

- **Thundering herd:** `serial` in the consuming playbook (not the role)
- **Disk space:** Documented in README as a prerequisite check
- **Scan timeout:** Exposed via `inspectah_scan_timeout` variable
- **Container cleanup:** `always` block in tasks
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
│   ├── preflight.yml            # Prerequisite validation
│   ├── install.yml              # COPR or RPM install (conditional)
│   ├── scan.yml                 # Run inspectah scan
│   ├── fetch.yml                # Pull tarball to control node
│   └── cleanup.yml              # Remove orphaned baseline containers
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
inspectah_install_method: "copr"

# COPR repository identifier. Used when inspectah_install_method is "copr".
inspectah_copr_repo: "mrussell/inspectah"

# Path to a local .rpm file on the control node.
# Used when inspectah_install_method is "rpm".
# The RPM is copied to the target and installed with dnf.
inspectah_rpm_path: ""

# Version to install. Empty string means "latest".
# Example: "0.8.6~beta.5" (use RPM tilde convention)
inspectah_install_version: ""

# --- Scan ---

# Target base image for cross-distro conversion.
# Example: "registry.redhat.io/rhel9/rhel-bootc:9.6"
# Leave empty for same-distro assessment (most common).
inspectah_base_image: ""

# Preserve sensitive data categories in the snapshot.
# Valid items: password-hashes, ssh-keys, subscription, all
# Implies --ack-sensitive automatically.
inspectah_preserve: []

# Skip the redaction phase (secrets remain unmasked in output).
# Implies --ack-sensitive automatically.
inspectah_no_redaction: false

# Directory on the target host for scan output.
inspectah_scan_output_dir: "/var/lib/inspectah/scans"

# Async timeout for the scan task in seconds.
# Cold scans with large base images may need 15+ minutes.
inspectah_scan_timeout: 900

# Poll interval in seconds for async scan task.
inspectah_scan_poll: 30

# Additional scan flags as a list of strings.
# Example: ["--verbose", "--no-baseline"]
inspectah_extra_args: []

# --- Fetch ---

# Directory on the control node to receive per-host tarballs.
# Tarballs are renamed to {{ inventory_hostname }}.tar.gz.
inspectah_fetch_dest: "{{ playbook_dir }}/scans"

# --- Cleanup ---

# Delete per-host tarballs from targets after successful fetch.
inspectah_cleanup_host_tarballs: true
```

### 4.2 Internal Constants (vars/main.yml)

```yaml
---
# Internal constants. Not part of the public API.
# Do not override these in inventory or playbooks.

# Progress mode forced for non-interactive Ansible execution.
_inspectah_progress_mode: "flat"

# Container name prefix for orphan cleanup.
_inspectah_baseline_container_prefix: "inspectah-baseline-"

# Minimum inspectah version with Ansible-compatible flat progress.
_inspectah_min_version: "0.8.0"
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
        control node, and cleans up orphaned baseline containers.
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
          installs via dnf. "rpm" copies a local RPM to the target.

      inspectah_copr_repo:
        type: str
        default: mrussell/inspectah
        description: COPR repository identifier.

      inspectah_rpm_path:
        type: path
        default: ""
        description: >-
          Path to a local .rpm file on the control node. Required
          when inspectah_install_method is "rpm".

      inspectah_install_version:
        type: str
        default: ""
        description: >-
          Specific version to install. Empty means latest.

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

      inspectah_scan_output_dir:
        type: path
        default: /var/lib/inspectah/scans
        description: >-
          Directory on the target host where the scan tarball is
          written.

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
          Additional CLI flags passed to inspectah scan.

      inspectah_fetch_dest:
        type: path
        default: "{{ playbook_dir }}/scans"
        description: >-
          Directory on the control node for fetched tarballs.

      inspectah_cleanup_host_tarballs:
        type: bool
        default: true
        description: >-
          Remove scan tarballs from target hosts after fetch.
```

---

## 5. Task Flow

### 5.1 Entry Point (tasks/main.yml)

```yaml
---
- name: Preflight checks
  ansible.builtin.include_tasks: preflight.yml

- name: Install inspectah
  ansible.builtin.include_tasks: install.yml
  when: inspectah_install | bool

- name: Run inspectah scan
  ansible.builtin.include_tasks: scan.yml

- name: Fetch scan tarball to control node
  ansible.builtin.include_tasks: fetch.yml
```

The cleanup task is NOT included here. It runs in a `block/always`
wrapper inside `scan.yml` to guarantee execution even on scan failure.

### 5.2 Preflight (tasks/preflight.yml)

Validates prerequisites before any work begins.

**Checks:**
1. `inspectah` binary exists in PATH (when `inspectah_install` is false)
2. `podman` is installed and >= 4.4
3. `nsenter` (from util-linux) is present
4. Target is a supported platform (RHEL/CentOS/Fedora, x86_64/aarch64)
5. `inspectah_rpm_path` is set when `inspectah_install_method` is "rpm"
6. `inspectah_scan_output_dir` parent directory exists

**Idempotency:** Read-only checks. No state changes. Safe to re-run.

### 5.3 Install (tasks/install.yml)

**Conditional on:** `inspectah_install | bool`

**COPR path (`inspectah_install_method == "copr"`):**

```
1. Enable COPR repo (idempotent, skipped if already enabled)
   - Prefer community.general.copr module
   - Fallback: ansible.builtin.command with creates: guard
2. Install inspectah package via ansible.builtin.dnf
   - With version pin when inspectah_install_version is set
```

**RPM path (`inspectah_install_method == "rpm"`):**

```
1. Copy RPM from control node to target via ansible.builtin.copy
2. Install via ansible.builtin.dnf with local file path
```

**Idempotency:** dnf install is naturally idempotent. COPR enable uses
`creates: /etc/yum.repos.d/_copr:copr.fedorainfracloud.org:{{ repo }}.repo`
or module-level idempotency.

### 5.4 Scan (tasks/scan.yml)

The core task. Wrapped in `block/always` for cleanup guarantee.

```yaml
- name: Scan and cleanup
  block:
    - name: Ensure scan output directory exists
      ansible.builtin.file:
        path: "{{ inspectah_scan_output_dir }}"
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
    - name: Clean up orphaned baseline containers
      ansible.builtin.include_tasks: cleanup.yml
```

**Command construction (`_inspectah_scan_argv`):**

Built as a list in a `set_fact` task to avoid shell injection:

```
["inspectah", "scan",
 "-o", "{{ inspectah_scan_output_dir }}",
 "--progress", "{{ _inspectah_progress_mode }}"]
+ (["--base-image", "{{ inspectah_base_image }}"]
   when inspectah_base_image != "")
+ (["--preserve", item] for item in inspectah_preserve)
+ (["--no-redaction"] when inspectah_no_redaction)
+ (["--ack-sensitive"]
   when inspectah_preserve | length > 0 or inspectah_no_redaction)
+ inspectah_extra_args
```

**Critical correctness notes:**

- `--progress flat` is always forced. Without it, Ansible captures ANSI
  escape sequences that corrupt registered output and break `changed_when`.
- `--ack-sensitive` is added automatically when preserve or no-redaction
  is active. Without it, inspectah exits non-zero and the scan fails.
- `become: true` is required. inspectah scan needs root to read system
  configuration, RPM databases, and manage podman containers.
- The `-o` flag controls output location. inspectah writes the tarball
  into this directory with a hostname-stamped filename
  (`inspectah-<hostname>-<timestamp>.tar.gz`).

**Exit code semantics:**

| Exit Code | Meaning | Role Behavior |
|-----------|---------|---------------|
| 0 | Clean scan | Success |
| 0 (degraded) | Trustworthy but with caveats | Success (logged) |
| 2 | Incomplete (inspector failure) | Fail task |
| 130 | User interrupt (SIGINT) | Fail task |
| Non-zero | Other error | Fail task |

**Idempotency:** Scan is not idempotent by nature (it captures system
state at a point in time). Re-running produces a new tarball with a
different timestamp. This is acceptable: the role captures current state,
not desired state.

### 5.5 Fetch (tasks/fetch.yml)

```yaml
- name: Find scan tarball
  ansible.builtin.find:
    paths: "{{ inspectah_scan_output_dir }}"
    patterns: "inspectah-*.tar.gz"
    file_type: file
  register: _inspectah_tarballs
  become: true

- name: Fail if no tarball found
  ansible.builtin.fail:
    msg: >-
      No inspectah tarball found in {{ inspectah_scan_output_dir }}.
      The scan may have failed silently.
  when: _inspectah_tarballs.files | length == 0

- name: Select most recent tarball
  ansible.builtin.set_fact:
    _inspectah_tarball_path: >-
      {{ (_inspectah_tarballs.files
          | sort(attribute='mtime', reverse=true)
          | first).path }}

- name: Fetch tarball to control node
  ansible.builtin.fetch:
    src: "{{ _inspectah_tarball_path }}"
    dest: "{{ inspectah_fetch_dest }}/{{ inventory_hostname }}.tar.gz"
    flat: true
    fail_on_missing: true
  become: true
```

**Design notes:**

- Uses `ansible.builtin.find` instead of hardcoding the tarball name
  because the filename includes a timestamp that the role does not
  control.
- Sorts by `mtime` descending to pick the most recent tarball when
  multiple exist (e.g., re-runs without cleanup).
- Renames to `{{ inventory_hostname }}.tar.gz` for aggregate
  consumption. inspectah aggregate accepts a directory of tarballs;
  hostname-based naming provides natural identification.
- `flat: true` avoids the default Ansible fetch behavior of creating
  a `hostname/path/to/file` directory tree.

### 5.6 Cleanup (tasks/cleanup.yml)

Runs in the `always` block of scan.yml. Guaranteed execution.

```yaml
- name: List orphaned baseline containers
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

- name: Remove orphaned baseline containers
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

- name: Remove host tarball after fetch
  ansible.builtin.file:
    path: "{{ _inspectah_tarball_path | default('') }}"
    state: absent
  become: true
  when:
    - inspectah_cleanup_host_tarballs | bool
    - _inspectah_tarball_path is defined
    - _inspectah_tarball_path | length > 0
```

---

## 6. Example Playbook Design

### 6.1 Full Pipeline (examples/site.yml)

```yaml
---
# Full inspectah fleet scan pipeline.
#
# Play 1: Scan all targets (role applies per-host)
# Play 2: Aggregate results on localhost
#
# Fleet-scale tuning:
#   serial: 10          — process 10 hosts at a time to avoid
#                          thundering herd on base image pulls
#   max_fail_percentage: 10  — tolerate up to 10% host failures
#                                without aborting the campaign

- name: Scan fleet hosts
  hosts: scan_targets
  # serial: 10
  # max_fail_percentage: 10
  become: false
  roles:
    - role: ansible-role-inspectah
      vars:
        inspectah_install: true
        inspectah_install_method: copr
        # inspectah_base_image: "registry.redhat.io/rhel9/rhel-bootc:9.6"
        # inspectah_preserve:
        #   - subscription

- name: Aggregate scan results
  hosts: localhost
  connection: local
  gather_facts: false
  vars:
    _aggregate_input: "{{ playbook_dir }}/scans"
    _aggregate_output: "{{ playbook_dir }}/aggregate"
  tasks:
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
      changed_when: true
```

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
        - "8"
        - "9"
        - "10"
    - name: Fedora
      versions:
        - "39"
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

| Distribution | Versions | Architectures | Notes |
|-------------|----------|---------------|-------|
| RHEL | 8, 9, 10 | x86_64, aarch64 | Primary target |
| CentOS Stream | 8, 9, 10 | x86_64, aarch64 | Community equivalent |
| Fedora | 39, 40, 41 | x86_64, aarch64 | Latest 3 releases |
| AlmaLinux | 8, 9 | x86_64, aarch64 | RHEL rebuild |
| Rocky Linux | 8, 9 | x86_64, aarch64 | RHEL rebuild |

**Prerequisites on target hosts:**

| Dependency | Minimum Version | Provided By |
|-----------|----------------|-------------|
| podman | 4.4 | System packages |
| nsenter | (any) | util-linux |
| dnf | (any) | System default |

**Control node requirements:**

| Dependency | Version | Purpose |
|-----------|---------|---------|
| Ansible | >= 2.14 | Role execution |
| inspectah | >= 0.8.0 | Aggregate command (localhost only) |
| community.general | >= 5.0 | `community.general.copr` module (optional) |

---

## 9. Failure Handling Strategy

### 9.1 Preflight Failures

Fail fast with a clear message. No partial state to clean up.

| Check | Failure Mode | Action |
|-------|-------------|--------|
| inspectah not found | Binary missing, install not requested | `ansible.builtin.fail` with install instructions |
| podman < 4.4 | Version too old | `ansible.builtin.fail` with upgrade instructions |
| nsenter missing | util-linux not installed | `ansible.builtin.fail` with package name |
| Unsupported OS | Not RHEL/CentOS/Fedora family | `ansible.builtin.fail` with platform list |
| RPM path empty | Method is "rpm" but no path | `ansible.builtin.fail` with variable name |

### 9.2 Scan Failures

| Failure | Detection | Action |
|---------|----------|--------|
| Timeout | async exceeds `inspectah_scan_timeout` | Task fails, cleanup still runs |
| Exit code 2 | Incomplete scan | Task fails, tarball may exist but is partial |
| Exit code 130 | SIGINT | Task fails |
| No tarball produced | `find` returns empty | Explicit `fail` with diagnostic message |
| Disk full | inspectah error | Task fails with inspectah stderr |

### 9.3 Fetch Failures

| Failure | Detection | Action |
|---------|----------|--------|
| Tarball missing | `fail_on_missing: true` | Task fails |
| Disk full on control | fetch module error | Task fails |
| Permission denied | fetch module error | Task fails |

### 9.4 Cleanup Resilience

Cleanup runs in `always` block. Individual container removal failures
are non-fatal (`failed_when: false` on the list step, individual `rm`
failures logged but do not abort).

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

### 12.1 Molecule Scenarios

**Default scenario:** COPR install + scan + fetch

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

**Air-gapped scenario:** RPM push install

Tests the `inspectah_install_method: rpm` path with a pre-built RPM
copied into the test container.

### 12.2 Test Limitations

Full scan testing requires:
- A real RHEL/CentOS/Fedora host (not a container)
- Root access for system inspection
- podman with container storage
- Network access for base image pulls

Molecule can test:
- Preflight checks (pass and fail paths)
- Install tasks (COPR enable, package install)
- Task ordering and variable interpolation
- Cleanup logic (create dummy containers, verify removal)
- Fetch task mechanics (create dummy tarball, verify fetch)

Molecule cannot test:
- Full inspectah scan (requires real host, not container)
- Actual base image pulls
- Real tarball content validation

### 12.3 CI Pipeline (.github/workflows/)

**Lint workflow (every PR):**
- ansible-lint at `production` profile
- yamllint
- ansible-playbook --syntax-check on all example playbooks

**Molecule workflow (every PR):**
- Molecule default scenario on EL9
- Molecule air_gapped scenario on EL9
- Matrix: ansible-core 2.14, 2.15, 2.16

---

## 13. Collection Dependencies

```yaml
# requirements.yml
---
collections:
  - name: community.general
    version: ">=5.0.0"
  - name: containers.podman
    version: ">=1.10.0"
```

`community.general` provides the `community.general.copr` module for
idempotent COPR repo management. `containers.podman` is optional but
preferred for container cleanup if available.

Both are soft dependencies: the role falls back to
`ansible.builtin.command` if neither collection is installed. The
`requirements.yml` documents the recommendation.

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
  (`inspectah_cleanup_host_tarballs: true`).

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

---

## 15. Open Questions

1. **COPR availability for RHEL 10 / Fedora 41.** Verify COPR build
   targets cover the full platform matrix before GA.

2. **AWX / AAP compatibility.** The role uses `{{ playbook_dir }}` for
   fetch destination defaults. AWX execution environments may resolve
   this differently. Needs validation on AAP 2.x.

3. **Pre-pull strategy role.** For large fleets, a separate "pre-pull
   base image" play may be worth packaging. Outside this spec's scope
   but noted as a future enhancement.

4. **Collection vs. role.** If a second role emerges (e.g., inspectah
   refine server hosting), consider migrating to a collection. For now,
   standalone role is correct.
