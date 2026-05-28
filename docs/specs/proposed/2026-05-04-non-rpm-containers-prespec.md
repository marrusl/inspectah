# Non-RPM Software & Containers Triage: Pre-Spec Analysis

**Author:** Collins (Image Mode Technology Specialist)
**Date:** 2026-05-04
**Status:** Pre-spec (input for brainstorm)

---

## 1. Current State

### What the Inspector Detects

The non-RPM software inspector (`cmd/inspectah/internal/inspector/nonrpm.go`) scans `/opt`, `/srv`, and `/usr/local` for software not installed via RPM. It uses five detection methods:

| Method | Detection Technique | Confidence |
|--------|-------------------|------------|
| **ELF binary classification** | `readelf -S` for section headers (`.note.go.buildid`, `.gopclntab` for Go; `.rustc` for Rust); `readelf -d` for dynamic linking analysis | High |
| **pip dist-info** | Scans `{/usr/lib,/usr/lib64,/usr/local/lib}/python3.*/site-packages/*.dist-info` directories; parses RECORD file for `.so` files to detect C extensions | High |
| **Python venv** | Looks for `pyvenv.cfg` under `/opt`, `/srv`; scans dist-info within the venv; reads `system-site-packages` flag | High |
| **npm/yarn lockfiles** | Searches for `package-lock.json`, `yarn.lock`, `Gemfile.lock` under `/opt`, `/srv`, `/usr/local` | High |
| **File/strings scan** | Falls back to `file` command for binary detection; `strings` (first 4KB or full) for version extraction | Low-Medium |

Additional detection: git repositories (`.git` directory with remote/commit/branch extraction), `.env` files routed to secrets review, and `requirements.txt` files.

The container inspector (`cmd/inspectah/internal/inspector/container.go`) scans for:
- **Quadlet units:** `.container`, `.volume`, `.network`, `.kube`, `.pod`, `.image`, `.build` files in `/etc/containers/systemd/`, `/usr/share/containers/systemd/`, and per-user `~/.config/containers/systemd/` directories
- **Compose files:** `docker-compose*.yml`, `compose*.yml` in `/opt`, `/srv`, `/etc`
- **Running containers:** `podman ps -a --format json` + `podman inspect` (optional, requires `--query-podman`)
- **Flatpak apps:** `flatpak list --app`

### Data Model

`NonRpmItem` carries: path, name, method, confidence, lang (go/rust/""), static (bool), version, shared_libs, system_site_packages, packages (pip list), has_c_extensions, git remote/commit/branch, files (lockfile contents), content, fleet prevalence.

`QuadletUnit` carries: path, name, content (full file), image (extracted from `Image=` directive), include, tie/tie_winner, fleet.

`ContainerSection` groups: quadlet_units, compose_files, running_containers, flatpak_apps.

### How Items Surface in Triage

Both non-RPM items and container items are classified by `classifyContainerItems()` in `renderer/triage.go` into the **"containers" section** of the triage UI:

- **Quadlet units:** Tier 2, grouped as `sub:quadlet` in single-machine mode
- **Running containers with quadlet backing:** Tier 2, display-only (not actionable)
- **Running containers without quadlet backing:** Tier 3, display-only, with warning about runtime-only state
- **Non-RPM items:** Tier 3, reason: "Non-RPM binary with unclear provenance"
- **Non-RPM binaries with method "binary":** Notification card type, asking user to provide reproducible build-time source

### How Items Render in the Containerfile

The Containerfile renderer (`renderer/containerfile.go`) handles included items:

- **pip packages with C extensions:** Multi-stage build. A `builder` stage installs gcc/python3-devel, creates a venv, runs `pip install` for each package, then `COPY --from=builder /opt/venv /opt/venv` in the final stage
- **Pure pip packages:** Listed as comments with name==version
- **npm items:** Listed as comments with name and method
- **Go binaries:** Listed as comments with name and path
- **Other standalone items:** Listed as comments with name, path, and method
- **Quadlet units:** `COPY quadlet/ /etc/containers/systemd/`
- **Compose files:** Comment noting inclusion, with guidance to consider converting to Quadlet

**Critical observation:** Most non-RPM items render as **comments only** in the Containerfile. The pip C-extension multi-stage build is the only case where the renderer generates actual executable Containerfile instructions. Everything else is informational -- the user must manually write the COPY/RUN instructions.

---

## 2. False Positive Analysis

### The Core Problem: No RPM Cross-Reference

The `scanPip()` function scans system `site-packages` directories for `.dist-info` directories and reports everything it finds. **It never checks whether the dist-info directory is owned by an RPM package.** The grep for `rpm -qf`, `owning_package`, `OwningPackage`, or `rpmdb` in `nonrpm.go` returns **zero results**.

This is the root cause of the false positives. On a standard Fedora/RHEL system, many Python packages are installed via RPM but ship `.dist-info` metadata as part of the RPM payload. The scanner has no way to distinguish these from pip-installed packages.

### Confirmed False Positive Pattern

The 13 false positives from Mark's Fedora 43 test all follow the same pattern:
- Installed by RPM (e.g., `python3-distro`, `python3-dnf`, `python3-nftables`)
- Have `.dist-info` directories in `/usr/lib/python3.*/site-packages/`
- Detected as method `pip dist-info` with confidence `high`

These are system Python packages that should not appear in the non-RPM section at all.

### Recommended Fix: RPM Ownership Check

**Option A: `rpm -qf` per dist-info directory.**
For each discovered `.dist-info` directory, run `rpm -qf <path>`. If the exit code is 0, the directory is RPM-owned -- skip it. This is simple and accurate but adds one subprocess call per dist-info directory.

**Option B: Batch `rpm -qa --queryformat` comparison.**
Run `rpm -qa --queryformat '%{NAME}\n'` once to get all installed RPM names, then check if a `python3-<name>` or `python3.XX-<name>` RPM exists for each detected pip package. Faster (one subprocess call) but relies on naming conventions.

**Option C: `rpm -ql` batch for site-packages directories.**
Run `rpm -ql $(rpm -qa 'python3-*')` to get all RPM-owned files in site-packages, build a set, then filter against it. Most accurate, handles renamed packages, but generates large output.

**Recommendation:** Option A is the most reliable. The number of dist-info directories on a typical system is 10-50, so 10-50 subprocess calls is acceptable. For ostree/bootc systems, the scanner already limits to `/usr/local/lib/python3` which avoids system packages entirely -- the false positive problem is specific to package-mode systems.

### Schema Impact

Add an `OwningPackage *string` field to `NonRpmItem` (mirroring the pattern already used in `ServiceStateChange`). When non-nil, the item was installed by an RPM and should be excluded from triage by default, or shown with reduced priority as "RPM-installed, no action needed."

---

## 3. Build Strategy Taxonomy

When a non-RPM item is "included" in triage, the Containerfile needs to reproduce it. The right approach depends on the item type and the bootc immutability model.

### Understanding the bootc filesystem model

- `/usr/` is immutable after deployment (the composefs layer). Anything COPYed into `/usr/` during build is permanently baked into the image.
- `/etc/` is mutable but tracked -- changes persist across updates.
- `/var/` is fully mutable -- the persistent data layer. Container storage lives here.
- `/opt/` lives under `/var/opt` or is bind-mounted from `/var/` on many bootc systems. Its mutability depends on the specific deployment.

### Strategy Per Type

| Type | Current Behavior | Recommended Approach | Rationale |
|------|-----------------|---------------------|-----------|
| **Static Go binary** (`/usr/local/bin/foo`) | Comment only | `COPY` from build context or multi-stage `go build` | Static binaries are self-contained. COPY is safe. If git remote is known, a multi-stage build from source is more reproducible. |
| **Dynamic C/C++ binary** | Comment only | `COPY` + validate shared libs exist in base | Must verify shared library dependencies are satisfied by the base image's RPMs. List `SharedLibs` in the triage card so the user can verify. |
| **Shell script** | Comment only | `COPY` | Straightforward. May need `chmod +x`. |
| **Python venv** (`/opt/myapp/venv`) | Comment only | `RUN python3 -m venv /opt/myapp/venv && pip install -r requirements.txt` | Reproducing from requirements is more portable than copying a venv (which may have absolute paths compiled in). If the venv has `system-site-packages: true`, note that RPM Python packages must also be installed. |
| **pip packages (system, RPM-owned)** | Comment as pip package | **Filter out entirely** | These are false positives -- the RPM section already handles them. |
| **pip packages (system, user-installed)** | Multi-stage for C ext, comment for pure | Multi-stage build for C extensions; `RUN pip install` for pure | Current multi-stage approach for C extensions is sound. Pure packages should get `RUN pip install` instructions, not just comments. |
| **npm app** (`/opt/webapp/`) | Comment only | `COPY package*.json` + `RUN npm ci` or `COPY` whole dir | If `package-lock.json` exists, `npm ci` in a multi-stage build is reproducible. Otherwise, COPY the directory. |
| **Git repository** | Comment only | Suggest `RUN git clone` or `COPY` with caveat | If git remote is known, `git clone --branch <branch> <remote>` at a pinned commit is reproducible. But repos in `/opt/` are often deployed apps, not development checkouts. |
| **requirements.txt** | Comment only | `RUN pip install -r` | Direct instruction generation. |
| **Directory with mixed content** | Comment only | `COPY` with warning | Generic fallback. The user needs to assess what's inside. |

### Architectural Decision Needed: COPY vs. Rebuild

The fundamental question is whether inspectah should:

**A) Copy-forward:** Capture the actual files from the source system and COPY them into the image. Advantages: exact reproduction, works for everything. Disadvantages: no provenance, no reproducibility, may carry stale binaries or platform-specific artifacts.

**B) Rebuild from source:** Generate `RUN` instructions that reproduce the software from upstream sources. Advantages: reproducible, auditable, architecture-portable. Disadvantages: requires knowing the source (git remote, pip package name, etc.), may not produce identical results, some software has no public source.

**C) Hybrid (recommended):** Use the detection metadata to choose the best strategy per item:
- If git remote is known: suggest rebuild from source
- If pip/npm with lockfile: suggest `RUN pip install` / `RUN npm ci`
- If static binary with no source info: COPY with provenance warning
- If dynamic binary: COPY with shared lib dependency check

### `/opt/` and the Mutability Question

In the bootc model, content that changes at runtime should not be baked into the image. The inspector should distinguish:

- **Application code in `/opt/`** that is deployed once and runs until the next image update: bake into the image via COPY or RUN.
- **Application data in `/opt/`** that changes at runtime (databases, caches, uploads): must be a volume mount or bind mount, not COPYed.

Currently, the inspector has no way to distinguish these. The triage UI should surface a question: "Does this content change at runtime?" If yes, recommend a volume mount strategy rather than COPY.

---

## 4. Quadlet Portability

### What Works Well

- **Quadlet `.container` files are inherently portable** when they reference images by fully-qualified registry URL (e.g., `Image=registry.example.com/myapp:v1.2`). The file format is a systemd unit generator -- the actual container runtime setup happens at boot via `podman-systemd-generator`.
- **The `COPY quadlet/ /etc/containers/systemd/` pattern is correct.** Quadlet files belong in `/etc/containers/systemd/` (or `/usr/share/containers/systemd/` for image-shipped defaults). The renderer gets this right.
- **`.network` files define Podman networks.** They are portable as long as the network configuration (subnet, gateway) does not conflict with the target environment.

### What's Risky

**1. Registry authentication at build vs. runtime:**
- During `podman build`, the build environment may not have registry credentials. If quadlet files reference private registries, the images cannot be pre-pulled at build time.
- At runtime (first boot), `podman` will pull images as specified in the quadlet files. If the target system needs registry auth, credentials must be configured separately (e.g., via `/etc/containers/auth.json` or `/run/containers/0/auth.json`).
- **Recommendation:** Do not attempt to pre-pull during build. Let quadlet handle pulls at runtime. But warn the user if quadlet files reference non-public registries.

**2. Image tag volatility:**
- `Image=myapp:latest` will pull whatever is current at boot time, not what was running on the source system.
- **Recommendation:** If the source system has the image pulled, capture the image digest and suggest pinning: `Image=myapp@sha256:abcdef...`

**3. Volume and bind mount assumptions:**
- Quadlet `.container` files may reference host paths for volumes (e.g., `Volume=/data/myapp:/app/data:Z`). These paths must exist on the target system.
- In bootc image mode, only `/var/` and `/etc/` are writable post-deployment. Volumes referencing other paths will fail.
- **Recommendation:** Parse Volume= directives and warn if they reference paths outside `/var/` or `/etc/`.

**4. Network mode conflicts:**
- `.network` files create Podman networks. If the target system already has conflicting network definitions, deployment may fail.
- **Recommendation:** Treat `.network` files as includable but flag potential conflicts.

**5. Per-user vs. system quadlet directories:**
- The scanner correctly discovers per-user quadlet dirs (`~/.config/containers/systemd/`). These run under user systemd sessions, not the system-wide systemd.
- **Recommendation:** Preserve the user/system distinction in the Containerfile output. User quadlet files should go to the appropriate user directory, not `/etc/containers/systemd/`.

### Quadlet and the bootc Model

Quadlet is actually the **ideal** container workload pattern for bootc:
- Quadlet files go in `/etc/containers/systemd/` (mutable, tracked by bootc)
- Container images are pulled at runtime to `/var/` (mutable, persistent)
- Container data volumes should point to `/var/` paths
- The image definition (quadlet file) is declarative and version-controlled

The main gap is that the current renderer treats all quadlet files as a single `COPY quadlet/` instruction without differentiating system vs. user quadlets, or warning about volume path compatibility.

---

## 5. What Competitors Do

### leapp (RHEL upgrade tool)

leapp handles in-place RHEL major version upgrades (e.g., RHEL 8 to RHEL 9). It is **not a migration-to-image-mode tool**, but its approach to non-RPM software is instructive:

- leapp **does not migrate non-RPM software.** It focuses exclusively on RPM packages, kernel, and system configuration.
- It runs "actors" that check for known incompatibilities (e.g., removed packages, changed configurations) but does not attempt to carry forward user-installed binaries.
- The `CustomModifications` actor warns about files modified outside RPM but does not remediate them.

**Takeaway:** Even Red Hat's own upgrade tool punts on non-RPM software. This validates that inspectah's approach of detecting and surfacing these items for human triage is the right model -- automated remediation for non-RPM software is not realistic.

### Universal Blue / uBlue

Universal Blue builds custom Fedora Atomic images using Containerfiles. Their patterns:

- **No migration story.** uBlue is a build-from-scratch approach. Users define what they want in their Containerfile; there is no "capture what's on the system and reproduce it."
- **RPM packages via `rpm-ostree install`** in the Containerfile (or `dnf install` on newer Fedora Atomic).
- **Non-RPM binaries:** Manually added via `COPY` or `RUN curl/wget` in the Containerfile. No automated detection.
- **Container workloads:** Some uBlue images ship Podman and quadlet files for container services, following the same `/etc/containers/systemd/` pattern inspectah uses.

**Takeaway:** uBlue validates the quadlet-in-image pattern and confirms that non-RPM software is handled manually in the Containerfile-based immutable OS world.

### bootc Community Patterns

From the bootc documentation and community examples:

- **Application code goes in `/usr/`** at build time. The composefs layer makes it immutable.
- **Configuration goes in `/etc/`** (tracked) or `/usr/etc/` (defaults).
- **Runtime data goes in `/var/`** (mutable, persistent).
- **Container workloads use Quadlet.** The pattern of shipping quadlet files in the image and letting systemd orchestrate containers at boot is the established approach.
- **No tooling exists for automated non-RPM migration.** This is a genuine gap that inspectah fills.

---

## 6. Open Questions for Brainstorm

### Detection Accuracy

1. **Should the RPM ownership check be opt-in or always-on?** Running `rpm -qf` for every dist-info dir adds latency. On ostree/bootc systems it's unnecessary (the scanner already limits to `/usr/local/`). On package-mode systems it's critical.

2. **What about other RPM-installed non-RPM-looking software?** Go binaries shipped by RPMs (e.g., `podman`, `buildah`) in `/usr/bin/` are not scanned (the scanner skips `/usr/bin/` by design). But what about RPM-shipped Go binaries in `/usr/local/bin/`? Unlikely but possible.

3. **Should the scanner detect additional types?** Java JARs/WARs, Ruby gems (beyond Gemfile), Rust binaries, Perl modules, systemd-portable images?

### Build Strategy

4. **How far should the Containerfile renderer go?** Currently most non-RPM items render as comments. Should the renderer generate executable instructions (COPY, RUN) for all types, or continue surfacing items for manual handling?

5. **Should inspectah capture file content for COPY?** If we want to generate `COPY` instructions, inspectah would need to capture the actual files into an output directory. Currently it only captures metadata. This would be a significant architectural change.

6. **Multi-stage build expansion:** The pip C-extension multi-stage build is a good pattern. Should we extend it to npm (`node:XX AS builder` + `npm ci`) and Go (`golang:XX AS builder` + `go build`)?

### Containers

7. **Running containers without quadlet backing: what's the migration path?** The current guidance is "consider converting to a Quadlet unit." Should inspectah offer to generate a quadlet file from `podman inspect` output? (The `podlet` tool does this.)

8. **Compose file conversion:** Should inspectah integrate with `podlet` to convert compose files to quadlet units? Or keep the current "include as-is with conversion suggestion" approach?

9. **Flatpak apps:** Currently detected but not integrated into the Containerfile. On bootc systems, Flatpak runs from `/var/lib/flatpak/`. Should flatpak apps be reproduced in the image via `RUN flatpak install`, or are they inherently user-space and outside the image's scope?

10. **Registry credential handling:** For quadlet files referencing private registries, should inspectah detect and warn? Should it capture auth configuration?

### Triage UX

11. **The "containers" section is overloaded.** It contains quadlet units, compose files, running containers, flatpak apps, AND non-RPM software items. These are conceptually different concerns. Should they be split into separate sections?

12. **All non-RPM items are Tier 3** ("Non-RPM binary with unclear provenance"). Should the tier vary by detection confidence and type? A Go binary with a known git remote is more trustworthy than an unidentified ELF binary.

13. **Provenance information is underused.** The scanner captures git remote, commit, branch, shared libs, version strings -- but the triage card shows only method. Surfacing this data would help users make better include/exclude decisions.

---

## 7. Recommended Approach

### Phase 1: Fix False Positives (Quick Win)

1. Add `rpm -qf <dist-info-dir>` check to `scanPip()` for package-mode systems
2. Add `OwningPackage *string` field to `NonRpmItem`
3. Skip RPM-owned items in triage classification (or show as "RPM-installed, no action needed")
4. Expected result: The 13 false positives on Fedora 43 disappear from triage

### Phase 2: Build Strategy Improvements

1. **Type-aware Containerfile rendering:** Instead of comments for everything, generate actionable instructions:
   - Go binary with git remote: `# Built from <remote>@<commit>; COPY from build context`
   - Python venv: `RUN python3 -m venv ... && pip install -r requirements.txt`
   - npm app with lockfile: multi-stage `npm ci`
   - Static binary without source: `COPY` with provenance warning
2. **Optional file capture:** Add `--capture-files` flag that copies non-RPM binaries into the output directory alongside the snapshot JSON
3. **Shared library dependency check:** For dynamic binaries, verify that required `.so` files are available in the base image's RPM set

### Phase 3: Container Workload Enhancements

1. **Quadlet volume path validation:** Parse `Volume=` directives and warn about paths outside `/var/` and `/etc/`
2. **Private registry detection:** Flag quadlet/compose files referencing non-public registries
3. **Image digest pinning:** Suggest `Image=@sha256:...` when the source system has the image pulled
4. **Running container to quadlet generation:** Integrate with or suggest `podlet generate` for containers without quadlet backing

### Phase 4: Triage UX Refinement

1. **Split the "containers" section** into "Non-RPM Software" and "Container Workloads"
2. **Tiered non-RPM items:** Vary tier by type and provenance (Go binary with git remote = Tier 2; unknown binary = Tier 3)
3. **Rich triage cards:** Surface git remote, shared libs, version, and build strategy recommendation on each card
4. **Mutability question:** For `/opt/` content, ask "Does this change at runtime?" to guide COPY vs. volume strategy

### What NOT to Do

- **Don't try to auto-fix everything.** Non-RPM software migration inherently requires human judgment. inspectah's value is in surfacing the right information and suggesting strategies, not in fully automating the migration.
- **Don't pre-pull container images at build time.** Let quadlet/systemd handle pulls at runtime. Build-time pulls create auth and network complexity.
- **Don't convert compose to quadlet automatically.** Suggest the conversion, point to `podlet`, but let the user decide. Compose files may have semantics that don't map cleanly to quadlet.
