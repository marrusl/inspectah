# ansible-role-inspectah Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a Galaxy-publishable Ansible role that automates the inspectah scan-fetch pipeline across RHEL/CentOS/Fedora fleets, with optional install, campaign-scoped fetch, two-layer container cleanup, and leave-no-trace host cleanup.

**Architecture:** The role follows standard Galaxy layout with `defaults/main.yml` as the public API, `vars/main.yml` for internal constants, and task files split by lifecycle phase (install, preflight, scan, fetch, host_cleanup). The entry point (`tasks/main.yml`) dispatches to phase files via `include_tasks` with conditional gates. Example playbooks demonstrate a three-play pattern (campaign ID generation on localhost, serial fleet scan, localhost aggregate) that avoids the `run_once` serial-batch scoping bug.

**Tech Stack:** Ansible >= 2.14, FQCN modules throughout, Molecule with podman driver, ansible-lint production profile, yamllint, GitHub Actions CI

## Global Constraints

- All variables prefixed with `inspectah_`. Internal constants prefixed with `_inspectah_`.
- FQCN for every module (`ansible.builtin.*`, `community.general.*`). No short-form module names.
- No `ansible.builtin.shell` tasks. All command execution via `ansible.builtin.command` with `argv` list.
- No bare variables in `when:` conditions (always use `| bool`, `| default()`, etc.).
- `become: true` only on tasks that require root. Never at play level inside the role.
- Target platforms: RHEL/CentOS Stream 9-10, Fedora 40-41 on x86_64/aarch64.
- Minimum inspectah version: 0.8.0. Minimum podman version: 4.4.
- All file paths are relative to `/Users/mrussell/Work/bootc-migration/ansible-role-inspectah/`.
- License: MIT (matching inspectah).
- Commit attribution: `Assisted-by: Claude Code (Opus 4.6)`

---

### Task 1: Repository Scaffolding and Project Config

**Files:**
- Create: `defaults/main.yml` (empty placeholder)
- Create: `vars/main.yml` (empty placeholder)
- Create: `handlers/main.yml`
- Create: `tasks/main.yml` (empty placeholder)
- Create: `meta/main.yml` (empty placeholder)
- Create: `meta/argument_specs.yml` (empty placeholder)
- Create: `tests/inventory`
- Create: `tests/test.yml`
- Create: `LICENSE`
- Create: `.gitignore`
- Create: `.ansible-lint`
- Create: `.yamllint`
- Create: `requirements.yml`
- Create: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing (first task)
- Produces: directory structure that all subsequent tasks populate; `.ansible-lint` and `.yamllint` configs used by CI (Task 13) and all linting steps

- [ ] **Step 1: Create directory structure**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
mkdir -p defaults vars handlers tasks meta molecule/default molecule/air_gapped molecule/fallback tests examples .github/workflows
```

- [ ] **Step 2: Create LICENSE (MIT)**
```
MIT License

Copyright (c) 2026 Mark Russell

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 3: Create .gitignore**
```
# Byte-compiled / optimized / DLL files
__pycache__/
*.py[cod]

# Molecule
.molecule/
.cache/

# Ansible
*.retry

# IDE
.idea/
.vscode/
*.swp
*.swo

# OS
.DS_Store
Thumbs.db
```

- [ ] **Step 4: Create .ansible-lint**
```yaml
---
profile: production

exclude_paths:
  - .github/
  - molecule/
  - .cache/

skip_list: []

warn_list:
  - experimental

enable_list:
  - fqcn
  - no-changed-when
  - no-jinja-when
```

- [ ] **Step 5: Create .yamllint**
```yaml
---
extends: default

rules:
  line-length:
    max: 120
    level: warning
  truthy:
    allowed-values: ["true", "false", "yes", "no"]
  comments:
    require-starting-space: true
    min-spaces-from-content: 1
  indentation:
    spaces: 2
    indent-sequences: true
```

- [ ] **Step 6: Create handlers/main.yml**
```yaml
---
# Handlers for ansible-role-inspectah.
# Reserved for future use.
```

- [ ] **Step 7: Create tests/inventory**
```ini
localhost ansible_connection=local
```

- [ ] **Step 8: Create tests/test.yml**
```yaml
---
- name: Test ansible-role-inspectah
  hosts: localhost
  remote_user: root
  roles:
    - ansible-role-inspectah
```

- [ ] **Step 9: Create requirements.yml**
```yaml
---
collections:
  - name: community.general
    version: ">=5.0.0"
```

- [ ] **Step 10: Create CHANGELOG.md**
```markdown
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial role implementation: install, preflight, scan, fetch, cleanup
- COPR and RPM install paths with version pinning
- Campaign-scoped fetch with sensitive data permission hardening
- Orphan container janitor (opt-in)
- Leave-no-trace host cleanup
- Molecule default and air-gapped scenarios
- CI: ansible-lint, yamllint, syntax-check, Molecule
- Example playbooks: full pipeline, scan-only, RPM push
```

- [ ] **Step 11: Create placeholder files for tasks and meta**
Create empty YAML documents (`---`) in:
- `defaults/main.yml`
- `vars/main.yml`
- `tasks/main.yml`
- `meta/main.yml`
- `meta/argument_specs.yml`

Each file starts with `---` only. Content is populated by subsequent tasks.

- [ ] **Step 12: Verify structure and lint config**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
find . -not -path './.git/*' -not -path './.git' | sort
yamllint .ansible-lint .yamllint requirements.yml
```
Expected: `find` shows the full tree matching the spec Section 3 layout. `yamllint` returns 0.

- [ ] **Step 13: Commit**
```bash
git add -A
git commit -m "feat(scaffold): create Galaxy role directory structure and project config

Establishes the ansible-role-inspectah repository layout per the approved
spec. Includes ansible-lint production profile, yamllint config, MIT license,
Galaxy legacy test stubs, collection dependency on community.general >= 5.0,
and initial CHANGELOG.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Galaxy Metadata and Argument Specs

**Files:**
- Modify: `meta/main.yml`
- Modify: `meta/argument_specs.yml`

**Interfaces:**
- Consumes: directory structure from Task 1
- Produces: `meta/main.yml` (Galaxy import metadata), `meta/argument_specs.yml` (variable type validation used by ansible-lint and `ansible-doc`)

- [ ] **Step 1: Write meta/main.yml**
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

- [ ] **Step 2: Write meta/argument_specs.yml**
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
          empty for same-distro assessment. When set, must be a
          valid container image reference (registry/image:tag or
          registry/image@sha256:...). Validated at preflight.

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

- [ ] **Step 3: Validate metadata**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint meta/main.yml meta/argument_specs.yml
```
Expected: exit 0, no errors.

- [ ] **Step 4: Commit**
```bash
git add meta/
git commit -m "feat(meta): add Galaxy metadata and argument specs

Galaxy metadata targets EL 9-10 and Fedora 40-41 with MIT license.
Argument specs define type validation for all 16 public variables
including choices for install_method and preserve categories.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Role Variables (defaults and vars)

**Files:**
- Modify: `defaults/main.yml`
- Modify: `vars/main.yml`

**Interfaces:**
- Consumes: nothing directly
- Produces: `inspectah_*` variables consumed by all task files (Tasks 4-8); `_inspectah_*` internal constants consumed by scan.yml (Task 6), preflight.yml (Task 4), fetch.yml (Task 7), scan.yml container cleanup

- [ ] **Step 1: Write defaults/main.yml**
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
# with serial batching.
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

- [ ] **Step 2: Write vars/main.yml**
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

- [ ] **Step 3: Lint both files**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint defaults/main.yml vars/main.yml
```
Expected: exit 0, no errors.

- [ ] **Step 4: Commit**
```bash
git add defaults/main.yml vars/main.yml
git commit -m "feat(vars): define public variable API and internal constants

Public API: 16 inspectah_ prefixed variables covering install, scan,
fetch, and cleanup phases. Internal constants: progress mode lock,
container prefix, min version gate, reserved flag list, and
auto-derived sensitive campaign flag.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Preflight Tasks

**Files:**
- Create: `tasks/preflight.yml`

**Interfaces:**
- Consumes: `_inspectah_min_version`, `_inspectah_reserved_flags` from `vars/main.yml` (Task 3); `inspectah_install_method`, `inspectah_rpm_path`, `inspectah_scan_output`, `inspectah_extra_args` from `defaults/main.yml` (Task 3)
- Produces: `_inspectah_safe_extra_args` fact (list with reserved flags stripped) consumed by scan.yml (Task 6)

- [ ] **Step 1: Write tasks/preflight.yml**

Platform enforcement follows the approved spec Section 8 tiered matrix:
- **Hard reject:** non-RedHat families, unsupported architectures, EL8 and below, Fedora < 40
- **Advisory warn (non-blocking):** best-effort platforms (AlmaLinux, Rocky, Fedora aarch64, EL10)
- **Silent pass:** release-blocking and smoke-tested platforms (CentOS Stream 9 x86_64, RHEL 9 x86_64, CentOS Stream 9 aarch64)

The tiers are about CI coverage claims, not runtime rejection. Best-effort platforms work but are not CI-gated.

```yaml
---
- name: Check inspectah binary exists
  ansible.builtin.command:
    cmd: which inspectah
  register: _inspectah_which
  changed_when: false
  failed_when: false

- name: Fail if inspectah not found
  ansible.builtin.fail:
    msg: >-
      inspectah binary not found in PATH. Either set inspectah_install: true
      to have the role install it, or install inspectah manually before
      running this role. See https://github.com/mrussell/inspectah
  when: _inspectah_which.rc != 0

- name: Get inspectah version
  ansible.builtin.command:
    cmd: inspectah --version
  register: _inspectah_version_output
  changed_when: false

- name: Extract inspectah version number
  ansible.builtin.set_fact:
    _inspectah_installed_version: >-
      {{ _inspectah_version_output.stdout
         | regex_search('[0-9]+\.[0-9]+\.[0-9]+') }}

- name: Validate inspectah version meets minimum
  ansible.builtin.fail:
    msg: >-
      inspectah version {{ _inspectah_installed_version }} is below the
      minimum required {{ _inspectah_min_version }}. Upgrade inspectah
      to >= {{ _inspectah_min_version }} before running this role.
  when: >-
    _inspectah_installed_version is version(_inspectah_min_version, '<')

- name: Check podman is installed
  ansible.builtin.command:
    cmd: podman --version
  register: _podman_version_output
  changed_when: false
  failed_when: false

- name: Fail if podman not found
  ansible.builtin.fail:
    msg: >-
      podman is not installed. inspectah requires podman >= 4.4.
      Install podman before running this role.
  when: _podman_version_output.rc != 0

- name: Extract podman version number
  ansible.builtin.set_fact:
    _podman_version: >-
      {{ _podman_version_output.stdout
         | regex_search('[0-9]+\.[0-9]+\.[0-9]+') }}

- name: Validate podman version meets minimum
  ansible.builtin.fail:
    msg: >-
      podman version {{ _podman_version }} is below the minimum
      required 4.4. Upgrade podman before running this role.
  when: _podman_version is version('4.4', '<')

- name: Check nsenter is installed
  ansible.builtin.command:
    cmd: which nsenter
  register: _nsenter_check
  changed_when: false
  failed_when: false

- name: Fail if nsenter not found
  ansible.builtin.fail:
    msg: >-
      nsenter not found (provided by util-linux). Install util-linux
      before running this role.
  when: _nsenter_check.rc != 0

- name: Fail if not RedHat OS family
  ansible.builtin.fail:
    msg: >-
      Unsupported OS family: {{ ansible_os_family }}.
      This role supports RHEL, CentOS Stream, Fedora, AlmaLinux,
      and Rocky Linux. Detected: {{ ansible_distribution }}
      {{ ansible_distribution_version }} (family: {{ ansible_os_family }}).
  when: ansible_os_family != 'RedHat'

- name: Fail if architecture is unsupported
  ansible.builtin.fail:
    msg: >-
      Unsupported architecture: {{ ansible_architecture }}.
      This role supports x86_64 and aarch64. Detected:
      {{ ansible_distribution }} {{ ansible_distribution_version }}
      on {{ ansible_architecture }}.
  when: ansible_architecture not in ['x86_64', 'aarch64']

- name: Reject EL8 and older (podman >= 4.4 not available)
  ansible.builtin.fail:
    msg: >-
      Unsupported EL version: {{ ansible_distribution }}
      {{ ansible_distribution_major_version }}. This role requires
      EL >= 9 (RHEL 8 does not ship podman >= 4.4 in default repos
      and is past active development). Upgrade to EL9 or newer.
  when:
    - ansible_distribution in ['RedHat', 'CentOS', 'AlmaLinux', 'Rocky', 'OracleLinux']
    - ansible_distribution_major_version | int < 9

- name: Validate Fedora version floor
  ansible.builtin.fail:
    msg: >-
      Unsupported Fedora version: {{ ansible_distribution_major_version }}.
      This role supports Fedora 40 and later. Detected:
      {{ ansible_distribution }} {{ ansible_distribution_version }}
      on {{ ansible_architecture }}.
  when:
    - ansible_distribution == 'Fedora'
    - ansible_distribution_major_version | int < 40

# --- Tiered platform advisory warnings ---
# The hard rejections above enforce the outer boundary (non-RedHat,
# unsupported arch, EL8, old Fedora). The warnings below inform
# operators when they are running on a best-effort platform that is
# NOT CI-gated. The role still runs — tiers are about CI coverage
# claims, not runtime rejection.

- name: Warn on Fedora aarch64 (best-effort, not CI-gated)
  ansible.builtin.debug:
    msg: >-
      NOTE: Fedora on aarch64 is best-effort. The CI matrix only covers
      Fedora on x86_64. The role should work, but this platform is not
      validated in CI. Detected: {{ ansible_distribution }}
      {{ ansible_distribution_version }} on {{ ansible_architecture }}.
  when:
    - ansible_distribution == 'Fedora'
    - ansible_architecture == 'aarch64'

- name: Warn on AlmaLinux / Rocky (best-effort, not CI-gated)
  ansible.builtin.debug:
    msg: >-
      NOTE: {{ ansible_distribution }} is best-effort. The CI matrix
      covers CentOS Stream and RHEL. As a RHEL rebuild,
      {{ ansible_distribution }} should work, but it is not validated
      in CI. Detected: {{ ansible_distribution }}
      {{ ansible_distribution_version }} on {{ ansible_architecture }}.
  when:
    - ansible_distribution in ['AlmaLinux', 'Rocky']

- name: Warn on EL10 (smoke-tested, limited CI coverage)
  ansible.builtin.debug:
    msg: >-
      NOTE: EL10 is smoke-tested tier. COPR builds are available but
      real-host validation is limited. Detected:
      {{ ansible_distribution }} {{ ansible_distribution_version }}
      on {{ ansible_architecture }}.
  when:
    - ansible_distribution in ['RedHat', 'CentOS', 'AlmaLinux', 'Rocky', 'OracleLinux']
    - ansible_distribution_major_version | int == 10

- name: Validate inspectah_base_image format when set
  ansible.builtin.fail:
    msg: >-
      inspectah_base_image is set but does not look like a valid
      container image reference. Expected format:
      registry/namespace/image:tag or registry/namespace/image@sha256:...
      Got: "{{ inspectah_base_image }}"
  when:
    - inspectah_base_image | length > 0
    - inspectah_base_image is not regex('^[a-zA-Z0-9][-a-zA-Z0-9.]*(/[-a-zA-Z0-9._]+)+([:@][-a-zA-Z0-9._:]+)?$')

- name: Validate RPM path is set when install method is rpm
  ansible.builtin.fail:
    msg: >-
      inspectah_install_method is "rpm" but inspectah_rpm_path is empty.
      Set inspectah_rpm_path to the path of the inspectah RPM on the
      control node.
  when:
    - inspectah_install_method == "rpm"
    - inspectah_install | bool
    - inspectah_rpm_path | length == 0

- name: Validate RPM file exists on control node
  ansible.builtin.stat:
    path: "{{ inspectah_rpm_path }}"
  register: _rpm_stat
  delegate_to: localhost
  when:
    - inspectah_install_method == "rpm"
    - inspectah_install | bool
    - inspectah_rpm_path | length > 0

- name: Fail if RPM file does not exist
  ansible.builtin.fail:
    msg: >-
      RPM file not found at {{ inspectah_rpm_path }} on the control
      node. Verify the path is correct.
  when:
    - _rpm_stat is defined
    - _rpm_stat.stat is defined
    - not _rpm_stat.stat.exists

- name: Ensure scan output parent directory is creatable
  ansible.builtin.file:
    path: "{{ inspectah_scan_output | dirname }}"
    state: directory
    mode: "0750"
  become: true

- name: Check for reserved flags in extra_args
  ansible.builtin.set_fact:
    _inspectah_conflicting_flags: >-
      {{ inspectah_extra_args | select('in', _inspectah_reserved_flags) | list }}

- name: Warn about reserved flags in extra_args
  ansible.builtin.debug:
    msg: >-
      WARNING: inspectah_extra_args contains reserved flags that are
      managed by the role: {{ _inspectah_conflicting_flags | join(', ') }}.
      These flags have been stripped. Use the role's own variables
      (inspectah_base_image, inspectah_preserve, inspectah_no_redaction)
      to control these options.
  when: _inspectah_conflicting_flags | length > 0

- name: Build safe extra_args with reserved flags stripped
  ansible.builtin.set_fact:
    _inspectah_safe_extra_args: >-
      {{ inspectah_extra_args | reject('in', _inspectah_reserved_flags) | list }}
```

- [ ] **Step 2: Lint preflight tasks**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint tasks/preflight.yml
```
Expected: exit 0.

- [ ] **Step 3: Commit**
```bash
git add tasks/preflight.yml
git commit -m "feat(preflight): add prerequisite and version validation tasks

Checks: inspectah binary + version >= 0.8.0, podman >= 4.4, nsenter
presence, RedHat OS family, RPM path validation for air-gapped installs,
scan output directory, and reserved flag stripping from extra_args.
Produces _inspectah_safe_extra_args for downstream scan command.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Install Tasks

**Files:**
- Create: `tasks/install.yml`

**Interfaces:**
- Consumes: `inspectah_install_method`, `inspectah_copr_repo`, `inspectah_rpm_path`, `inspectah_install_version` from `defaults/main.yml` (Task 3)
- Produces: `_inspectah_pkg_installed` fact (bool, consumed by host_cleanup.yml Task 8); `_inspectah_copr_enabled` fact (bool, consumed by host_cleanup.yml Task 8)

- [ ] **Step 1: Write tasks/install.yml**
```yaml
---
# COPR install path
- name: Install via COPR
  when: inspectah_install_method == "copr"
  block:
    - name: Enable COPR repository
      community.general.copr:
        name: "{{ inspectah_copr_repo }}"
        state: enabled
      become: true
      register: _inspectah_copr_result

    - name: Record COPR enablement for cleanup tracking
      ansible.builtin.set_fact:
        _inspectah_copr_enabled: true
      when: _inspectah_copr_result is changed

  rescue:
    - name: Enable COPR repository (fallback without community.general)
      ansible.builtin.command:
        argv:
          - dnf
          - copr
          - enable
          - "{{ inspectah_copr_repo }}"
          - --yes
        creates: "/etc/yum.repos.d/_copr:copr.fedorainfracloud.org:{{ inspectah_copr_repo | replace('/', ':') }}.repo"
      become: true
      register: _inspectah_copr_fallback_result

    - name: Record COPR enablement for cleanup tracking (fallback)
      ansible.builtin.set_fact:
        _inspectah_copr_enabled: true
      when: _inspectah_copr_fallback_result is changed

- name: Install inspectah package via dnf (COPR, versioned)
  ansible.builtin.dnf:
    name: "inspectah-{{ inspectah_install_version }}"
    state: present
  become: true
  register: _inspectah_copr_install_versioned
  when:
    - inspectah_install_method == "copr"
    - inspectah_install_version | length > 0

- name: Install inspectah package via dnf (COPR, latest)
  ansible.builtin.dnf:
    name: inspectah
    state: present
  become: true
  register: _inspectah_copr_install_latest
  when:
    - inspectah_install_method == "copr"
    - inspectah_install_version | length == 0

# RPM push install path
- name: Install via local RPM
  when: inspectah_install_method == "rpm"
  block:
    - name: Copy RPM to target host
      ansible.builtin.copy:
        src: "{{ inspectah_rpm_path }}"
        dest: /tmp/inspectah.rpm
        mode: "0644"
      become: true

    - name: Install inspectah from local RPM
      ansible.builtin.dnf:
        name: /tmp/inspectah.rpm
        state: present
        disable_gpg_check: true
      become: true
      register: _inspectah_rpm_install

- name: Record package installation for cleanup tracking
  ansible.builtin.set_fact:
    _inspectah_pkg_installed: true
  when: >-
    (_inspectah_copr_install_versioned is defined and
     _inspectah_copr_install_versioned is changed) or
    (_inspectah_copr_install_latest is defined and
     _inspectah_copr_install_latest is changed) or
    (_inspectah_rpm_install is defined and
     _inspectah_rpm_install is changed)
```

- [ ] **Step 2: Lint install tasks**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint tasks/install.yml
```
Expected: exit 0.

- [ ] **Step 3: Commit**
```bash
git add tasks/install.yml
git commit -m "feat(install): add COPR and RPM install paths

COPR path uses community.general.copr with rescue fallback to
dnf copr enable command. Supports version pinning via
inspectah_install_version. RPM path copies local RPM to target
and installs with dnf (no GPG check — operator-trusted input).
Sets tracking facts for host_cleanup.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Scan Tasks

**Files:**
- Create: `tasks/scan.yml`

**Interfaces:**
- Consumes: `inspectah_scan_output`, `inspectah_base_image`, `inspectah_preserve`, `inspectah_no_redaction`, `inspectah_scan_timeout`, `inspectah_scan_poll` from `defaults/main.yml` (Task 3); `_inspectah_progress_mode`, `_inspectah_baseline_container_prefix` from `vars/main.yml` (Task 3); `_inspectah_safe_extra_args` fact from preflight.yml (Task 4)
- Produces: `inspectah_scan_result` registered variable (available to consuming playbooks); triggers orphan container cleanup when `inspectah_cleanup_orphan_containers` is true

- [ ] **Step 1: Write tasks/scan.yml**
```yaml
---
- name: Build scan command argument list
  ansible.builtin.set_fact:
    _inspectah_scan_argv: >-
      {{
        ['inspectah', 'scan',
         '-o', inspectah_scan_output,
         '--progress', _inspectah_progress_mode]
        + (['--base-image', inspectah_base_image]
           if inspectah_base_image | length > 0 else [])
        + (inspectah_preserve | map('regex_replace', '^(.*)$', '--preserve\n\1')
           | join('\n') | split('\n')
           if inspectah_preserve | length > 0 else [])
        + (['--no-redaction']
           if inspectah_no_redaction | bool else [])
        + (['--ack-sensitive']
           if (inspectah_preserve | length > 0) or
              (inspectah_no_redaction | bool) else [])
        + _inspectah_safe_extra_args
      }}

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

- [ ] **Step 2: Lint scan tasks**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint tasks/scan.yml
```
Expected: exit 0.

- [ ] **Step 3: Commit**
```bash
git add tasks/scan.yml
git commit -m "feat(scan): add scan command construction and orphan janitor

Builds argv list from role variables with --ack-sensitive auto-add
when preserve or no-redaction is active. Uses async/poll for long
scans. Orphan container janitor runs in always block when opt-in
flag is set, sweeping inspectah-baseline-* prefix containers.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Fetch Tasks

**Files:**
- Create: `tasks/fetch.yml`

**Interfaces:**
- Consumes: `inspectah_scan_output`, `inspectah_campaign_id`, `inspectah_fetch_dest`, `inspectah_cleanup_host_tarball` from `defaults/main.yml` (Task 3); `_inspectah_sensitive_campaign` from `vars/main.yml` (Task 3)
- Produces: fetched tarballs at `_inspectah_run_dir/{{ inventory_hostname }}.tar.gz` on the control node (consumed by aggregate in example playbook, Task 10)

- [ ] **Step 1: Write tasks/fetch.yml**
```yaml
---
- name: Set fetch directory (with or without campaign scoping)
  ansible.builtin.set_fact:
    _inspectah_run_dir: >-
      {{ (inspectah_campaign_id | default('') | length > 0)
         | ternary(inspectah_fetch_dest ~ '/' ~ inspectah_campaign_id,
                   inspectah_fetch_dest) }}

- name: Ensure fetch directory exists on control node
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
  when: inspectah_cleanup_host_tarball | bool
```

- [ ] **Step 2: Lint fetch tasks**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint tasks/fetch.yml
```
Expected: exit 0.

- [ ] **Step 3: Commit**
```bash
git add tasks/fetch.yml
git commit -m "feat(fetch): add campaign-scoped fetch with permission hardening

Derives fetch directory from campaign_id presence. Tightens
directory to 0700 for sensitive campaigns. Uses flat fetch with
fail_on_missing. Tarball cleanup runs only on successful fetch
(not in always block).

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 8: Host Cleanup Tasks

**Files:**
- Create: `tasks/host_cleanup.yml`

**Interfaces:**
- Consumes: `inspectah_copr_repo`, `inspectah_install_method` from `defaults/main.yml` (Task 3); `_inspectah_pkg_installed`, `_inspectah_copr_enabled` facts from install.yml (Task 5)
- Produces: clean host state (inspectah uninstalled, COPR repo removed, staged RPM deleted)

- [ ] **Step 1: Write tasks/host_cleanup.yml**
```yaml
---
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
    path: /tmp/inspectah.rpm
    state: absent
  become: true
  when:
    - _inspectah_pkg_installed | default(false) | bool
    - inspectah_install_method == "rpm"
```

- [ ] **Step 2: Lint host cleanup tasks**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint tasks/host_cleanup.yml
```
Expected: exit 0.

- [ ] **Step 3: Commit**
```bash
git add tasks/host_cleanup.yml
git commit -m "feat(cleanup): add leave-no-trace host cleanup tasks

Uninstalls inspectah, removes COPR repo, and deletes staged RPM.
Only acts on artifacts the role itself created via tracking facts
from install.yml. Never removes pre-existing installations.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9: Task Entry Point

**Files:**
- Modify: `tasks/main.yml`

**Interfaces:**
- Consumes: `inspectah_install`, `inspectah_cleanup_host` from `defaults/main.yml` (Task 3)
- Produces: dispatches to install.yml (Task 5), preflight.yml (Task 4), scan.yml (Task 6), fetch.yml (Task 7), host_cleanup.yml (Task 8) in correct order

- [ ] **Step 1: Write tasks/main.yml**
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

- [ ] **Step 2: Validate syntax**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint tasks/main.yml
ansible-playbook tests/test.yml --syntax-check 2>&1 || true
```
Expected: yamllint exit 0. Syntax check may warn about missing localhost connection but no YAML errors.

- [ ] **Step 3: Commit**
```bash
git add tasks/main.yml
git commit -m "feat(tasks): add entry point dispatcher

Dispatches to install, preflight, scan, fetch, and host_cleanup
in spec-defined order. Install and host_cleanup are conditional.
Install runs before preflight so version check validates the
binary the scan will actually use.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 10: Example Playbooks and Inventory

**Files:**
- Create: `examples/site.yml`
- Create: `examples/scan_only.yml`
- Create: `examples/scan_rpm.yml`
- Create: `examples/inventory.ini`

**Interfaces:**
- Consumes: role public API from `defaults/main.yml` (Task 3); role name `ansible-role-inspectah`
- Produces: reference playbooks for users; used by CI syntax-check (Task 13)

- [ ] **Step 1: Write examples/site.yml**
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

# Play 1: Scan the fleet
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
        inspectah_install_method: copr
        # inspectah_install_version: "0.8.6~beta.5"
        # inspectah_base_image: "registry.redhat.io/rhel9/rhel-bootc:9.6"
        # inspectah_preserve:
        #   - subscription
        # inspectah_cleanup_host: true

# Play 2: Aggregate on control node
- name: Aggregate scan results
  hosts: localhost
  connection: local
  gather_facts: false
  vars:
    _inspectah_min_version: "0.8.0"
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

- [ ] **Step 2: Write examples/scan_only.yml**
```yaml
---
# Scan-only playbook — fetch tarballs without aggregating.
# Use this to collect snapshots for later analysis or transfer
# to another machine for aggregate processing.

- name: Initialize campaign
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Generate campaign ID
      ansible.builtin.set_fact:
        inspectah_campaign_id: "{{ lookup('pipe', 'date +%Y%m%dT%H%M%S') }}"

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
        inspectah_install_method: copr
```

- [ ] **Step 3: Write examples/scan_rpm.yml**
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
        inspectah_cleanup_host: true
```

- [ ] **Step 4: Write examples/inventory.ini**
```ini
[scan_targets]
webserver01.example.com
webserver02.example.com
dbserver01.example.com

[scan_targets:vars]
ansible_user=ansible
ansible_become=true
```

- [ ] **Step 5: Lint example playbooks**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint examples/site.yml examples/scan_only.yml examples/scan_rpm.yml
```
Expected: exit 0.

- [ ] **Step 6: Commit**
```bash
git add examples/
git commit -m "feat(examples): add fleet pipeline, scan-only, and RPM push playbooks

Full pipeline demonstrates three-play pattern with campaign ID
generation on localhost, serial fleet scan, and localhost aggregate.
Scan-only strips the aggregate play. RPM push shows air-gapped
install path with trust boundary documentation. Example inventory
defines scan_targets group.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 11: Molecule Default Scenario

**Files:**
- Create: `molecule/default/molecule.yml`
- Create: `molecule/default/converge.yml`
- Create: `molecule/default/prepare.yml`
- Create: `molecule/default/verify.yml`

**Interfaces:**
- Consumes: role public API; role tasks from Tasks 4-9
- Produces: passing Molecule default scenario; used by CI molecule workflow (Task 13)

- [ ] **Step 1: Write molecule/default/molecule.yml**
```yaml
---
dependency:
  name: galaxy
driver:
  name: podman
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

- [ ] **Step 2: Write molecule/default/prepare.yml**

This playbook stubs inspectah and podman binaries so Molecule can
exercise role mechanics without a real scan.

```yaml
---
- name: Prepare test environment
  hosts: all
  become: true
  tasks:
    - name: Install test dependencies
      ansible.builtin.dnf:
        name:
          - util-linux
          - podman
        state: present

    - name: Create stub inspectah binary
      ansible.builtin.copy:
        dest: /usr/local/bin/inspectah
        mode: "0755"
        content: |
          #!/bin/bash
          # Stub inspectah for Molecule testing.
          # Supports --version and scan subcommands.
          case "$1" in
            --version|version)
              echo "inspectah 0.8.6"
              exit 0
              ;;
            scan)
              # Parse -o flag to find output path
              OUTPUT=""
              while [[ $# -gt 0 ]]; do
                case "$1" in
                  -o|--output)
                    OUTPUT="$2"
                    shift 2
                    ;;
                  *)
                    shift
                    ;;
                esac
              done
              if [[ -n "$OUTPUT" ]]; then
                mkdir -p "$(dirname "$OUTPUT")"
                echo "stub-scan-data" | tar czf "$OUTPUT" --files-from=/dev/null 2>/dev/null
                # Create a minimal valid tarball
                touch "$OUTPUT"
                echo '{"host": "test", "timestamp": "2026-01-01T00:00:00Z"}' > /tmp/_inspectah_stub.json
                tar czf "$OUTPUT" -C /tmp _inspectah_stub.json
                rm -f /tmp/_inspectah_stub.json
              fi
              echo "Scan complete (stub)"
              exit 0
              ;;
            *)
              echo "inspectah stub: unknown command $1"
              exit 1
              ;;
          esac
```

- [ ] **Step 3: Write molecule/default/converge.yml**
```yaml
---
- name: Converge
  hosts: all
  become: false
  vars:
    inspectah_install: true
    inspectah_install_method: copr
    inspectah_campaign_id: "molecule-test"
    inspectah_cleanup_orphan_containers: true
  roles:
    - role: ansible-role-inspectah
```

- [ ] **Step 4: Write molecule/default/verify.yml**
```yaml
---
- name: Verify
  hosts: all
  become: true
  tasks:
    - name: Check inspectah stub is present
      ansible.builtin.command:
        cmd: which inspectah
      register: _verify_inspectah
      changed_when: false

    - name: Assert inspectah found
      ansible.builtin.assert:
        that:
          - _verify_inspectah.rc == 0
        fail_msg: "inspectah binary not found after converge"

    - name: Check scan result was registered
      ansible.builtin.assert:
        that:
          - inspectah_scan_result is defined
          - inspectah_scan_result.rc == 0
        fail_msg: "inspectah_scan_result not registered or scan failed"

    - name: Verify tarball was fetched to control node
      ansible.builtin.stat:
        path: "{{ playbook_dir }}/scans/molecule-test/inspectah-test-el9.tar.gz"
      register: _verify_tarball
      delegate_to: localhost

    - name: Assert tarball exists on control node
      ansible.builtin.assert:
        that:
          - _verify_tarball.stat.exists
        fail_msg: >-
          Fetched tarball not found at expected path. Campaign-scoped
          fetch may have failed.
      delegate_to: localhost

    - name: Verify tarball was cleaned from target (default behavior)
      ansible.builtin.stat:
        path: "/var/lib/inspectah/scans/inspectah-test-el9.tar.gz"
      register: _verify_target_tarball

    - name: Assert target tarball was removed
      ansible.builtin.assert:
        that:
          - not _verify_target_tarball.stat.exists
        fail_msg: >-
          Target tarball still present. cleanup_host_tarball should
          have removed it after fetch.
```

- [ ] **Step 5: Lint Molecule files**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint molecule/default/molecule.yml molecule/default/converge.yml molecule/default/prepare.yml molecule/default/verify.yml
```
Expected: exit 0.

- [ ] **Step 5.5: Run Molecule default scenario**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
molecule test --scenario-name default
```
Expected: converge, verify, and destroy all pass. The COPR install path
is exercised (via stub), scan produces a tarball, fetch transfers it to
the control node, and target tarball cleanup removes the original.

- [ ] **Step 6: Commit**
```bash
git add molecule/default/
git commit -m "feat(molecule): add default scenario with scan stub

Default scenario uses UBI9 with podman driver. Prepare creates a
stub inspectah binary that responds to --version and scan commands,
producing a dummy tarball. Verify checks scan result registration,
campaign-scoped fetch, and target tarball cleanup.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 12: Molecule Air-Gapped Scenario

**Files:**
- Create: `molecule/air_gapped/molecule.yml`
- Create: `molecule/air_gapped/prepare.yml`
- Create: `molecule/air_gapped/converge.yml`
- Create: `molecule/air_gapped/verify.yml`

**Interfaces:**
- Consumes: role public API; `inspectah_install_method: rpm` path from install.yml (Task 5)
- Produces: passing Molecule air_gapped scenario with a real RPM built via rpmbuild; used by CI molecule workflow (Task 13)

- [ ] **Step 1: Write molecule/air_gapped/molecule.yml**
```yaml
---
dependency:
  name: galaxy
driver:
  name: podman
platforms:
  - name: inspectah-test-airgap
    image: registry.access.redhat.com/ubi9/ubi:latest
    command: /usr/sbin/init
    privileged: true
    pre_build_image: true
provisioner:
  name: ansible
verifier:
  name: ansible
```

- [ ] **Step 2: Write molecule/air_gapped/prepare.yml**

Build a minimal valid RPM using rpmbuild so that `dnf install` can
process it. The RPM installs a stub inspectah binary at
`/usr/local/bin/inspectah` that responds to `--version` and `scan`.
This proves the RPM install path end-to-end without network access.

```yaml
---
- name: Build stub RPM on localhost
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Create rpmbuild directory structure
      ansible.builtin.file:
        path: "{{ item }}"
        state: directory
        mode: "0755"
      loop:
        - "{{ playbook_dir }}/rpmbuild/SPECS"
        - "{{ playbook_dir }}/rpmbuild/SOURCES"
        - "{{ playbook_dir }}/rpmbuild/BUILD"
        - "{{ playbook_dir }}/rpmbuild/RPMS"
        - "{{ playbook_dir }}/rpmbuild/SRPMS"

    - name: Create stub inspectah script for RPM
      ansible.builtin.copy:
        dest: "{{ playbook_dir }}/rpmbuild/SOURCES/inspectah"
        mode: "0755"
        content: |
          #!/bin/bash
          case "$1" in
            --version|version)
              echo "inspectah 0.8.6"
              exit 0
              ;;
            scan)
              OUTPUT=""
              while [[ $# -gt 0 ]]; do
                case "$1" in
                  -o|--output) OUTPUT="$2"; shift 2 ;;
                  *) shift ;;
                esac
              done
              if [[ -n "$OUTPUT" ]]; then
                mkdir -p "$(dirname "$OUTPUT")"
                echo '{"host":"test"}' > /tmp/_stub.json
                tar czf "$OUTPUT" -C /tmp _stub.json
                rm -f /tmp/_stub.json
              fi
              exit 0
              ;;
            *) exit 1 ;;
          esac

    - name: Create RPM spec file
      ansible.builtin.copy:
        dest: "{{ playbook_dir }}/rpmbuild/SPECS/inspectah.spec"
        mode: "0644"
        content: |
          Name:    inspectah
          Version: 0.8.6
          Release: 1.molecule%{?dist}
          Summary: Stub inspectah for Molecule testing
          License: MIT
          Source0: inspectah

          %description
          Minimal stub RPM for Molecule air-gapped scenario testing.

          %install
          mkdir -p %{buildroot}/usr/local/bin
          cp %{SOURCE0} %{buildroot}/usr/local/bin/inspectah

          %files
          %attr(0755,root,root) /usr/local/bin/inspectah

    - name: Build the stub RPM
      ansible.builtin.command:
        argv:
          - rpmbuild
          - -bb
          - --define
          - "_topdir {{ playbook_dir }}/rpmbuild"
          - "{{ playbook_dir }}/rpmbuild/SPECS/inspectah.spec"
      register: _rpmbuild_result

    - name: Find built RPM
      ansible.builtin.find:
        paths: "{{ playbook_dir }}/rpmbuild/RPMS"
        patterns: "inspectah-*.rpm"
        recurse: true
      register: _built_rpms

    - name: Copy RPM to scenario directory
      ansible.builtin.copy:
        src: "{{ _built_rpms.files[0].path }}"
        dest: "{{ playbook_dir }}/inspectah-stub.rpm"
        mode: "0644"
        remote_src: true

- name: Prepare target with test dependencies
  hosts: all
  become: true
  tasks:
    - name: Install test dependencies
      ansible.builtin.dnf:
        name:
          - util-linux
          - podman
        state: present
```

- [ ] **Step 3: Write molecule/air_gapped/converge.yml**
```yaml
---
- name: Converge (RPM install path)
  hosts: all
  become: false
  roles:
    - role: ansible-role-inspectah
      vars:
        inspectah_install: true
        inspectah_install_method: rpm
        inspectah_rpm_path: "{{ playbook_dir }}/inspectah-stub.rpm"
        inspectah_cleanup_host: true
```

- [ ] **Step 4: Write molecule/air_gapped/verify.yml**
```yaml
---
- name: Verify air-gapped scenario
  hosts: all
  become: true
  tasks:
    - name: Check inspectah is available
      ansible.builtin.command:
        cmd: inspectah --version
      register: _verify_version
      changed_when: false

    - name: Assert inspectah version is reported
      ansible.builtin.assert:
        that:
          - "'0.8' in _verify_version.stdout"
        fail_msg: "inspectah version check failed in air-gapped scenario"

    - name: Check scan result was registered
      ansible.builtin.assert:
        that:
          - inspectah_scan_result is defined
          - inspectah_scan_result.rc == 0
        fail_msg: "Scan result not registered or failed in air-gapped scenario"
```

- [ ] **Step 5: Lint air-gapped scenario files**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint molecule/air_gapped/molecule.yml molecule/air_gapped/prepare.yml molecule/air_gapped/converge.yml molecule/air_gapped/verify.yml
```
Expected: exit 0.

- [ ] **Step 5.5: Run Molecule air-gapped scenario**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
molecule test --scenario-name air_gapped
```
Expected: converge, verify, and destroy all pass. The RPM install path
is exercised end-to-end: prepare builds a real RPM with rpmbuild,
converge installs it via `inspectah_install_method: rpm`, the scan
runs using the stub binary from the RPM, and host cleanup removes the
package and staged RPM.

- [ ] **Step 6: Commit**
```bash
git add molecule/air_gapped/
git commit -m "feat(molecule): add air-gapped scenario for RPM install path

Tests role behavior with pre-staged binary simulating the RPM push
install path. Verifies scan execution and result registration
without COPR repo access.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 12.5: Molecule Fallback Scenario (no community.general)

**Files:**
- Create: `molecule/fallback/molecule.yml`
- Create: `molecule/fallback/converge.yml`
- Create: `molecule/fallback/verify.yml`

**Interfaces:**
- Consumes: role public API; COPR enable fallback path from install.yml (Task 5)
- Produces: passing Molecule fallback scenario validating the `ansible.builtin.command` fallback when `community.general` is not installed; used by CI molecule workflow (Task 13)

- [ ] **Step 1: Write molecule/fallback/molecule.yml**
```yaml
---
dependency:
  name: galaxy
  options:
    # Do NOT install community.general — this scenario tests the fallback path
    requirements-file: ""
driver:
  name: podman
platforms:
  - name: inspectah-test-fallback
    image: registry.access.redhat.com/ubi9/ubi:latest
    command: /usr/sbin/init
    privileged: true
    pre_build_image: true
provisioner:
  name: ansible
verifier:
  name: ansible
```

- [ ] **Step 2: Write molecule/fallback/converge.yml**
```yaml
---
- name: Prepare target with stub binary
  hosts: all
  become: true
  tasks:
    - name: Install test dependencies
      ansible.builtin.dnf:
        name:
          - util-linux
          - podman
        state: present

    - name: Create stub inspectah binary
      ansible.builtin.copy:
        dest: /usr/local/bin/inspectah
        mode: "0755"
        content: |
          #!/bin/bash
          case "$1" in
            --version|version)
              echo "inspectah 0.8.6"
              exit 0
              ;;
            scan)
              OUTPUT=""
              while [[ $# -gt 0 ]]; do
                case "$1" in
                  -o|--output) OUTPUT="$2"; shift 2 ;;
                  *) shift ;;
                esac
              done
              if [[ -n "$OUTPUT" ]]; then
                mkdir -p "$(dirname "$OUTPUT")"
                echo '{"host":"test"}' > /tmp/_stub.json
                tar czf "$OUTPUT" -C /tmp _stub.json
                rm -f /tmp/_stub.json
              fi
              exit 0
              ;;
            *) exit 1 ;;
          esac

- name: Converge (fallback — no community.general)
  hosts: all
  become: false
  roles:
    - role: ansible-role-inspectah
      vars:
        inspectah_install: true
        inspectah_install_method: copr
```

- [ ] **Step 3: Write molecule/fallback/verify.yml**
```yaml
---
- name: Verify fallback scenario
  hosts: all
  become: true
  tasks:
    - name: Check COPR repo was enabled via command fallback
      ansible.builtin.stat:
        path: "/etc/yum.repos.d/_copr:copr.fedorainfracloud.org:mrussell:inspectah.repo"
      register: _verify_copr_repo

    - name: Assert COPR repo file exists (proves fallback path ran)
      ansible.builtin.assert:
        that:
          - _verify_copr_repo.stat.exists
        fail_msg: >-
          COPR repo file not found. The command fallback for COPR
          enablement may not have fired when community.general is
          absent.

    - name: Check scan result was registered
      ansible.builtin.assert:
        that:
          - inspectah_scan_result is defined
          - inspectah_scan_result.rc == 0
        fail_msg: "Scan result not registered or failed in fallback scenario"
```

- [ ] **Step 4: Lint fallback scenario files**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint molecule/fallback/molecule.yml molecule/fallback/converge.yml molecule/fallback/verify.yml
```
Expected: exit 0.

- [ ] **Step 5: Run Molecule fallback scenario**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
molecule test --scenario-name fallback
```
Expected: converge, verify, and destroy all pass. The COPR enable task
fails (community.general not installed), the rescue block fires the
`ansible.builtin.command` fallback, and the scan proceeds normally.

- [ ] **Step 6: Commit**
```bash
git add molecule/fallback/
git commit -m "feat(molecule): add fallback scenario for COPR without community.general

Tests the ansible.builtin.command rescue path for COPR repo enablement
when community.general collection is not installed. Validates that the
creates: guard provides idempotency for the command fallback.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 13: CI Pipeline

**Files:**
- Create: `.github/workflows/lint.yml`
- Create: `.github/workflows/molecule.yml`
- Create: `.github/workflows/smoke.yml`
- Create: `tests/smoke.yml`

**Interfaces:**
- Consumes: `.ansible-lint`, `.yamllint` from Task 1; example playbooks from Task 10; Molecule scenarios from Tasks 11-12.5
- Produces: PR-blocking lint and Molecule gates; weekly/manual real-host smoke gate (requires self-hosted CentOS Stream 9 runner)

- [ ] **Step 1: Write .github/workflows/lint.yml**
```yaml
---
name: Lint

"on":
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Set up Python
        uses: actions/setup-python@v5
        with:
          python-version: "3.12"

      - name: Install dependencies
        run: |
          python -m pip install --upgrade pip
          pip install ansible-core ansible-lint yamllint

      - name: Install collection dependencies
        run: ansible-galaxy collection install -r requirements.yml

      - name: Run yamllint
        run: yamllint .

      - name: Run ansible-lint
        run: ansible-lint

      - name: Syntax check example playbooks
        run: |
          for playbook in examples/*.yml; do
            echo "Checking ${playbook}..."
            ansible-playbook "${playbook}" --syntax-check -i examples/inventory.ini
          done
```

- [ ] **Step 2: Write .github/workflows/molecule.yml**
```yaml
---
name: Molecule

"on":
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  molecule:
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        scenario:
          - default
          - air_gapped
          - fallback
        ansible-version:
          - "2.14"
          - "2.15"
          - "2.16"
          - "2.17"
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Set up Python
        uses: actions/setup-python@v5
        with:
          python-version: "3.12"

      - name: Install dependencies
        run: |
          python -m pip install --upgrade pip
          pip install "ansible-core~=${{ matrix.ansible-version }}.0" \
                      molecule molecule-plugins[podman] \
                      ansible-lint yamllint

      - name: Install collection dependencies
        if: matrix.scenario != 'fallback'
        run: ansible-galaxy collection install -r requirements.yml

      - name: Install rpmbuild (air_gapped scenario)
        if: matrix.scenario == 'air_gapped'
        run: sudo apt-get update && sudo apt-get install -y rpm

      - name: Install podman
        run: |
          sudo apt-get update
          sudo apt-get install -y podman

      - name: Run Molecule (${{ matrix.scenario }})
        run: molecule test --scenario-name ${{ matrix.scenario }}
        env:
          ANSIBLE_FORCE_COLOR: "true"
          MOLECULE_DISTRO: ubi9
```

- [ ] **Step 3: Write .github/workflows/smoke.yml**

This is a real-host smoke test, not a Molecule re-run. It requires a
self-hosted runner with access to a CentOS Stream 9 x86_64 VM (or
bare-metal host). The job installs inspectah from COPR, runs a real
scan, and validates the tarball output. It is triggered via
`workflow_dispatch` and nightly schedule -- not on every PR.

This job will fail if run on GitHub-hosted runners (no CentOS Stream 9
VM access). That is intentional: it is gated on self-hosted runner
availability. The CI README should document the self-hosted runner
requirements.

```yaml
---
name: Real-host smoke test

"on":
  schedule:
    # Weekly on Sundays at 03:00 UTC
    - cron: "0 3 * * 0"
  workflow_dispatch: {}

jobs:
  smoke:
    # Requires a self-hosted runner with:
    #   - CentOS Stream 9 x86_64 (VM or bare-metal, not a container)
    #   - podman >= 4.4 installed
    #   - nsenter (util-linux) installed
    #   - Network access to COPR (copr.fedorainfracloud.org)
    #   - Root access (sudo without password)
    #
    # This job CANNOT run on GitHub-hosted Ubuntu runners.
    # It validates gaps that Molecule container tests cannot cover:
    #   - Real inspectah scan execution (not a stub)
    #   - Real tarball content (not a dummy)
    #   - Real container lifecycle (podman storage, base image pull)
    #   - End-to-end COPR install -> scan -> fetch -> cleanup pipeline
    runs-on: [self-hosted, centos-stream-9, x86_64]
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Set up Python
        uses: actions/setup-python@v5
        with:
          python-version: "3.12"

      - name: Install Ansible dependencies
        run: |
          python -m pip install --upgrade pip
          pip install ansible-core

      - name: Install collection dependencies
        run: ansible-galaxy collection install -r requirements.yml

      - name: Verify prerequisites
        run: |
          echo "--- OS ---"
          cat /etc/os-release
          echo "--- Architecture ---"
          uname -m
          echo "--- podman ---"
          podman --version
          echo "--- nsenter ---"
          which nsenter

      - name: Run smoke test playbook
        run: |
          ansible-playbook tests/smoke.yml \
            -i "localhost," \
            -c local \
            --become \
            -e inspectah_install=true \
            -e inspectah_install_method=copr \
            -e inspectah_cleanup_host=true
        env:
          ANSIBLE_FORCE_COLOR: "true"
```

- [ ] **Step 3.5: Write tests/smoke.yml**

A minimal playbook that exercises the real end-to-end pipeline on
a CentOS Stream 9 host: install from COPR, scan, verify tarball,
clean up. This is NOT a Molecule scenario -- it runs directly via
`ansible-playbook` on the self-hosted runner.

```yaml
---
- name: Real-host smoke test
  hosts: all
  become: false
  vars:
    _smoke_scan_output: "/tmp/inspectah-smoke-{{ ansible_hostname }}.tar.gz"
    _smoke_fetch_dest: "/tmp/inspectah-smoke-results"
  roles:
    - role: ansible-role-inspectah
      vars:
        inspectah_scan_output: "{{ _smoke_scan_output }}"
        inspectah_fetch_dest: "{{ _smoke_fetch_dest }}"
        inspectah_cleanup_host_tarball: false

- name: Verify smoke test results
  hosts: all
  become: true
  tasks:
    - name: Verify tarball exists on target
      ansible.builtin.stat:
        path: "/tmp/inspectah-smoke-{{ ansible_hostname }}.tar.gz"
      register: _smoke_tarball

    - name: Assert tarball was produced
      ansible.builtin.assert:
        that:
          - _smoke_tarball.stat.exists
          - _smoke_tarball.stat.size > 0
        fail_msg: >-
          Smoke test failed: scan tarball not found or empty at
          /tmp/inspectah-smoke-{{ ansible_hostname }}.tar.gz

    - name: Verify tarball is a valid gzip archive
      ansible.builtin.command:
        argv:
          - tar
          - tzf
          - "/tmp/inspectah-smoke-{{ ansible_hostname }}.tar.gz"
      register: _smoke_tarball_contents
      changed_when: false

    - name: Assert tarball contains expected files
      ansible.builtin.assert:
        that:
          - _smoke_tarball_contents.stdout_lines | length > 0
        fail_msg: >-
          Smoke test failed: tarball appears empty or corrupt.

    - name: Verify fetched tarball on control node
      ansible.builtin.stat:
        path: "/tmp/inspectah-smoke-results/{{ ansible_hostname }}.tar.gz"
      delegate_to: localhost
      register: _smoke_fetched

    - name: Assert fetched tarball exists
      ansible.builtin.assert:
        that:
          - _smoke_fetched.stat.exists
        fail_msg: >-
          Smoke test failed: fetched tarball not found on control node.

    - name: Clean up smoke test artifacts
      ansible.builtin.file:
        path: "{{ item }}"
        state: absent
      loop:
        - "/tmp/inspectah-smoke-{{ ansible_hostname }}.tar.gz"
      become: true

- name: Clean up control node artifacts
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Remove fetched smoke test results
      ansible.builtin.file:
        path: "/tmp/inspectah-smoke-results"
        state: absent
```

- [ ] **Step 4: Lint CI files and smoke playbook**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint .github/workflows/lint.yml .github/workflows/molecule.yml .github/workflows/smoke.yml tests/smoke.yml
```
Expected: exit 0.

- [ ] **Step 5: Commit**
```bash
git add .github/ tests/smoke.yml
git commit -m "ci: add lint, Molecule, and real-host smoke test workflows

Lint workflow runs yamllint, ansible-lint (production), and
syntax-check on all example playbooks. Molecule workflow runs
default, air_gapped, and fallback scenarios across ansible-core
2.14-2.17 with podman driver. Fallback job skips community.general
install to prove the no-collection rescue path. Real-host smoke
test runs weekly and on manual dispatch against a self-hosted
CentOS Stream 9 x86_64 runner with real inspectah from COPR.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 14: README

**Files:**
- Create: `README.md`

**Interfaces:**
- Consumes: all role variables from Task 3; example playbooks from Task 10; platform matrix from spec Section 8
- Produces: Galaxy-quality documentation; used by Galaxy quality scoring

- [ ] **Step 1: Write README.md**
```markdown
# ansible-role-inspectah

Ansible role for running [inspectah](https://github.com/mrussell/inspectah) migration analysis across a fleet of RHEL, CentOS, or Fedora hosts.

## What this role does

1. **(Optional) Install** inspectah via COPR or local RPM push
2. **Preflight** — validate prerequisites (inspectah >= 0.8.0, podman >= 4.4, nsenter, supported platform)
3. **Scan** the target host, producing a migration snapshot tarball
4. **Fetch** the tarball to the control node (with optional campaign-scoped directories)
5. **Clean up** — remove target tarball (default), sweep orphan containers (opt-in)
6. **(Optional) Uninstall** — remove inspectah and COPR repo from the target

## What this role does NOT do

- Run `inspectah aggregate` (control-node operation; see [examples/site.yml](examples/site.yml))
- Run `inspectah refine` (interactive, not automatable)
- Manage container registries or image mirrors
- Configure podman (podman >= 4.4 is a prerequisite)

## Requirements

### Target hosts

| Dependency | Minimum Version | Package |
|-----------|----------------|---------|
| podman | 4.4 | podman |
| nsenter | any | util-linux |
| dnf | any | system default |

### Control node

| Dependency | Version | Purpose |
|-----------|---------|---------|
| Ansible | >= 2.14 | Role execution |
| inspectah | >= 0.8.0 | Aggregate (example playbook only) |
| community.general | >= 5.0 | COPR module (optional, fallback exists) |

Install collection dependencies:

```bash
ansible-galaxy collection install -r requirements.yml
```

## Supported Platforms

| Distribution | Versions | Architectures | Tier |
|-------------|----------|---------------|------|
| CentOS Stream | 9 | x86_64 | Release-blocking |
| RHEL | 9, 10 | x86_64, aarch64 | Smoke-tested |
| CentOS Stream | 9, 10 | x86_64, aarch64 | Smoke-tested |
| Fedora | 40, 41 | x86_64 | Best-effort |
| AlmaLinux / Rocky | 9 | x86_64, aarch64 | Best-effort |

## Role Variables

### Installation

| Variable | Default | Description |
|----------|---------|-------------|
| `inspectah_install` | `false` | Install inspectah on target hosts |
| `inspectah_install_method` | `"copr"` | `"copr"` (GPG-verified) or `"rpm"` (operator-trusted) |
| `inspectah_copr_repo` | `"mrussell/inspectah"` | COPR repository identifier |
| `inspectah_rpm_path` | `""` | Path to local RPM on control node (rpm method only) |
| `inspectah_install_version` | `""` | Pin version across fleet (empty = latest) |

### Scan

| Variable | Default | Description |
|----------|---------|-------------|
| `inspectah_base_image` | `""` | Target base image for cross-distro conversion (prefer `@sha256:...` digest-pinned refs for fleet consistency) |
| `inspectah_preserve` | `[]` | Sensitive data to preserve: `password-hashes`, `ssh-keys`, `subscription`, `all` |
| `inspectah_no_redaction` | `false` | Skip redaction (secrets remain unmasked) |
| `inspectah_scan_output` | `"/var/lib/inspectah/scans/{{ inventory_hostname }}.tar.gz"` | Tarball output path on target |
| `inspectah_scan_timeout` | `900` | Async timeout in seconds |
| `inspectah_scan_poll` | `30` | Poll interval in seconds |
| `inspectah_extra_args` | `[]` | Additional CLI flags (list of strings) |

### Fetch

| Variable | Default | Description |
|----------|---------|-------------|
| `inspectah_campaign_id` | `""` | Campaign ID for grouping tarballs (see examples) |
| `inspectah_fetch_dest` | `"{{ playbook_dir }}/scans"` | Base directory on control node for tarballs |

### Cleanup

| Variable | Default | Description |
|----------|---------|-------------|
| `inspectah_cleanup_host_tarball` | `true` | Remove tarball from target after fetch |
| `inspectah_cleanup_orphan_containers` | `false` | Sweep orphan `inspectah-baseline-*` containers (NOT safe for concurrent scans) |
| `inspectah_cleanup_host` | `false` | Uninstall inspectah and COPR repo after campaign |

## Example Playbook

### Full pipeline (scan + aggregate)

```yaml
# Play 0: Generate campaign ID (truly once, before serial batching)
- name: Initialize campaign
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Generate campaign ID
      ansible.builtin.set_fact:
        inspectah_campaign_id: "{{ lookup('pipe', 'date +%Y%m%dT%H%M%S') }}"

# Play 1: Scan the fleet
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

# Play 2: Aggregate
- name: Aggregate scan results
  hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - name: Run inspectah aggregate
      ansible.builtin.command:
        argv:
          - inspectah
          - aggregate
          - "{{ playbook_dir }}/scans/{{ inspectah_campaign_id }}"
          - --output-dir
          - "{{ playbook_dir }}/aggregate"
      changed_when: true
```

See [examples/](examples/) for scan-only and RPM push playbooks.

### Why three plays?

Ansible's `run_once` fires once per `serial` batch, not once per play. With `serial: 10` and 50 hosts, `run_once` would fire 5 times, creating 5 different campaign directories. Play 0 generates the campaign ID on localhost before serial batching begins, guaranteeing all hosts write to the same directory.

## Fleet-Scale Notes

- **Thundering herd:** Use `serial: 10` (or appropriate batch size) in the scan play to avoid concurrent base image pulls overwhelming the registry.
- **Disk space:** Tarballs are typically 5-50MB per host. Plan for target-side (scan output + base image cache) and control-side (fetched tarballs + aggregate output) storage.
- **Network bandwidth:** At 500 hosts with 20MB average tarballs, expect ~10GB of transfer. Run the control node close to the fleet.
- **Scan duration:** Cold scans (no cached image) take 3-5 minutes. Warm scans take 30-60 seconds. Default timeout is 15 minutes.

## Security

- All commands use `ansible.builtin.command` with `argv` (no shell injection).
- `--ack-sensitive` is added automatically when preserve or no-redaction is set.
- Sensitive campaigns tighten fetch directory permissions to `0700`.
- COPR installs are GPG-verified by dnf. RPM push installs are operator-trusted (no provenance check).
- See the [spec](https://github.com/mrussell/inspectah/blob/main/process-docs/specs/proposed/ansible-role-spec.md) for full security analysis.

### Digest-pinned base images

Tag-based image references (e.g., `registry.redhat.io/rhel9/rhel-bootc:9.6`) can resolve to different digests across hosts if the tag is updated mid-campaign. For reproducible fleet-wide scans, use `@sha256:...` digest-pinned references:

```yaml
inspectah_base_image: "registry.redhat.io/rhel9/rhel-bootc@sha256:abc123..."
```

This guarantees every host in the campaign scans against the same image content, regardless of when each host pulls the image.

## Testing

```bash
# Lint
ansible-lint
yamllint .

# Molecule (requires podman)
molecule test                              # default scenario (COPR install)
molecule test --scenario-name air_gapped   # RPM push scenario
molecule test --scenario-name fallback     # COPR without community.general
```

## License

MIT

## Author

Mark Russell
```

- [ ] **Step 2: Lint README**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
# README is markdown, not YAML — no yamllint needed.
# Verify it exists and is non-empty:
wc -l README.md
```
Expected: ~170+ lines.

- [ ] **Step 3: Commit**
```bash
git add README.md
git commit -m "docs: add Galaxy-quality README

Covers role purpose, requirements, platform matrix, all 16 variables
with defaults, three-play example, fleet-scale notes, security model,
and testing instructions.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 15: Final Validation

**Files:**
- No new files
- Test: all files created in Tasks 1-14

**Interfaces:**
- Consumes: entire role from Tasks 1-14
- Produces: validated, lint-clean, syntax-checked role ready for implementation

- [ ] **Step 1: Run yamllint on all YAML files**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
yamllint .
```
Expected: exit 0, no errors.

- [ ] **Step 2: Run ansible-lint**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
ansible-lint
```
Expected: exit 0 at production profile. Fix any findings before proceeding.

- [ ] **Step 3: Syntax-check example playbooks**
```bash
cd /Users/mrussell/Work/bootc-migration/ansible-role-inspectah
for playbook in examples/*.yml; do
  echo "--- Checking ${playbook} ---"
  ansible-playbook "${playbook}" --syntax-check -i examples/inventory.ini
done
```
Expected: all three playbooks pass syntax check.

- [ ] **Step 4: Verify spec coverage**

Cross-reference each spec section against the implementation:

| Spec Section | Plan Task | Status |
|-------------|-----------|--------|
| 1. Purpose | All tasks | Covered |
| 2. Design Decisions | Architecture choices embedded | Covered |
| 3. Repository Structure | Task 1 | Covered |
| 4.1 Public Variables | Task 3 (defaults) | Covered |
| 4.2 Internal Constants | Task 3 (vars) | Covered |
| 4.3 Argument Specs | Task 2 | Covered |
| 5.1 Entry Point | Task 9 | Covered |
| 5.2 Preflight | Task 4 | Covered |
| 5.3 Install | Task 5 | Covered |
| 5.4 Scan | Task 6 | Covered |
| 5.5 Fetch | Task 7 | Covered |
| 5.6 Container Cleanup | Task 6 (always block) | Covered |
| 5.7 Host Cleanup | Task 8 | Covered |
| 6.1 Full Pipeline | Task 10 (site.yml) | Covered |
| 6.2 Scan Only | Task 10 (scan_only.yml) | Covered |
| 6.3 Inventory | Task 10 (inventory.ini) | Covered |
| 6.4 RPM Push | Task 10 (scan_rpm.yml) | Covered |
| 7. Galaxy Metadata | Task 2 | Covered |
| 8. Platform Matrix | Task 4 (preflight) | Covered |
| 9. Failure Handling | Tasks 4-8 | Covered |
| 10. Fleet-Scale | Task 14 (README) | Covered |
| 11. Galaxy Graduation | Task 1 (CHANGELOG), Task 2 (meta) | Covered |
| 12. Testing | Tasks 11-12, 12.5, 13 (smoke) | Covered |
| 13. Collection Deps | Task 1 (requirements.yml) | Covered |
| 14. Security | Tasks 4-7, Task 14 | Covered |

- [ ] **Step 5: Fix any lint or coverage issues found in Steps 1-4, then commit fixes**
```bash
git add -A
git commit -m "fix: address lint findings from final validation

Assisted-by: Claude Code (Opus 4.6)"
```
Only create this commit if fixes were needed. Skip if Steps 1-3 passed clean.
