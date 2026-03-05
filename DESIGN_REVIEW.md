# Design Document Review

Review of `design.md` against the current codebase. Generated 2026-03-05.

---

## Section 1: Design Doc Drift

### 1.1 Containerfile example shows per-file COPYs

**Design doc says:** Separate COPY lines for repos, firewall, timers, NM connections, and user append fragments (lines 282-334).

**Code does:** Consolidated `COPY config/etc/ /etc/` via `_config_copy_roots()` in `containerfile.py`. One COPY per top-level directory, not per file.

**Code is right.** The consolidated approach was an intentional improvement for layer efficiency. The design doc example should be updated to show the consolidated pattern.

### 1.2 User/group section describes only append pattern

**Design doc says:** Users provisioned via `config/tmp/passwd.append` fragments with `cat >> /etc/passwd` (line 194, lines 312-318).

**Code does:** Five-strategy system (sysusers, blueprint, useradd, kickstart, exact-copy) based on user classification. The append pattern is now the `exact-copy` strategy only. Strategy-aware rendering in `containerfile.py` lines 522-608.

**Code is right.** The strategy system was an intentional addition. The design doc should be updated to describe all five strategies, the classification logic, and the `--user-strategy` override.

### 1.3 HTML report category cards lack status indicators

**Design doc says:** Cards show "a status indicator (green checkmark if fully automated, amber if FIXMEs exist, red if manual intervention needed)" (line 404).

**Code does:** Cards show counts and descriptive text only. No green/amber/red status icons. See `report.html.j2` lines 156-168.

**Design is right.** This is an unimplemented feature — the data is available via `compute_triage()` but not surfaced on the cards.

### 1.4 HTML report missing Secrets drill-down tab

**Design doc says:** Drill-down includes "**Secrets**: summary of all redacted items with file paths, pattern types, and remediation suggestions" (line 415).

**Code does:** Redactions appear as warnings in the warning panel and in the audit report, but there is no dedicated Secrets tab or drill-down section. `report.html.j2` has no `section("secrets", ...)` block.

**Design is right.** This is an unimplemented feature.

### 1.5 Config diff comments: block vs per-file placement

**Design doc says:** Each per-file COPY gets a comment summarizing the diff directly above it (lines 68-79).

**Code does:** Diff summaries are in a block comment in `_config_inventory_comment()` before the consolidated COPY, using the `_summarise_diff()` helper. The format of each summary (key: old → new) matches the design, but the placement differs because there are no per-file COPYs.

**Code is right.** Follows from 1.1 — consolidated COPY means diff summaries must be block comments. The design doc example should be updated.

### 1.6 Audit report "Items Requiring Manual Intervention" not risk-prioritized

**Design doc says:** "consolidated list pulled from all inspectors, prioritized by risk" (line 392).

**Code does:** Lists all warnings from `snapshot.warnings` in order, not sorted by risk. See `audit_report.py` lines 492-497.

**Code is partially right.** The list exists but is not risk-prioritized. The triage data (`compute_triage()`) could be used to sort, but this hasn't been implemented.

### 1.7 CLI flags table missing recent additions

**Design doc says:** CLI flags table at lines 454-471 lists 14 flags.

**Code has:** 16 flags in `cli.py`. Missing from design doc: `--user-strategy`.

**Code is right.** Design doc needs updating.

### 1.8 Leaf/auto package dependency analysis not described

**Code does:** RPM inspector computes leaf vs auto packages via dependency graph analysis (`_classify_leaf_auto` in `rpm.py`). Containerfile emits only leaf packages. Audit report shows dependency tree. HTML report shows collapsible tree. Schema has `leaf_packages`, `auto_packages`, `leaf_dep_tree`.

**Not in design doc.** This is a significant feature that should be documented.

### 1.9 Registry auth check not described as distinct step

**Design doc says:** "RHEL images require `podman login registry.redhat.io`" (line 213) and mentions "registry auth failure" as a fallback case (line 227).

**Code does:** `_check_registry_auth()` in `baseline.py` runs `podman login --get-login registry.redhat.io` before attempting image pull, with a specific error message and instructions.

**Code adds detail.** The pre-pull auth check should be explicitly described in the design doc's baseline generation flow.

### 1.10 Progress messages not described

**Code does:** TTY-gated progress messages to stderr ("Inspecting packages...", etc.) in `inspectors/__init__.py`.

**Not in design doc.** Minor — operational UX, not architectural. Could be mentioned in the Runtime Model section.

### 1.11 Kickstart user directives not explicitly described

**Code does:** `kickstart.py` emits `user --name=... --uid=... --shell=...` directives for kickstart-strategy users.

**Design doc says:** Kickstart includes DHCP, hostname, DNS, NFS credentials, and environment variables (lines 425-431). Does not mention user directives.

**Code is right.** Design doc should add user provisioning to the kickstart file description.

---

## Section 2: Unimplemented Design Doc Features

### 2.1 HTML report: category card status indicators (green/amber/red)

The triage data exists (`compute_triage()` in `_triage.py`) but is not used to set card styling. Each card should show a colored indicator based on whether its inspector area has FIXMEs or manual items.

- **Effort:** Small (< 1 hour)
- **Demo value:** Medium — visual at-a-glance health
- **Dependencies:** None

### 2.2 HTML report: Secrets drill-down tab

The redaction data is in `snapshot.redactions` with path, pattern, line, and remediation. A dedicated tab would show this as a table with columns matching the `secrets-review.md` content.

- **Effort:** Small (< 1 hour)
- **Demo value:** Medium — makes the report self-contained for secret review
- **Dependencies:** None

### 2.3 Audit report: risk-prioritized manual intervention list

The current implementation lists warnings in insertion order. Risk prioritization would sort by severity (error > warning > info) and by inspector criticality.

- **Effort:** Small (< 1 hour)
- **Demo value:** Low — the list is already useful unsorted
- **Dependencies:** None

### 2.4 HTML report: Containers drill-down with image references

The design describes "Container workloads (quadlet units with images, compose files with service-to-image mappings)" in the audit report (line 384). The HTML report has a Containers section but it's minimal — just basic quadlet/compose listing without prominent image reference display.

- **Effort:** Small (< 1 hour)
- **Demo value:** Medium — operators need to verify image references
- **Dependencies:** None

---

## Section 3: Recommended Next Steps

### Priority 1: Things that will surprise users

1. **Update design doc: user/group strategies** — The design doc describes only the old append pattern. The implementation has five strategies, classification logic, `--user-strategy` flag, sysusers/blueprint/kickstart rendering. The design doc needs a new "User Provisioning Strategies" subsection. *Medium effort.*

2. **Update design doc: leaf/auto package dependency analysis** — A significant feature (77% package list reduction) that is not described anywhere in the design doc. Needs a section in the Baseline Generation area. *Small effort.*

3. **Update design doc: Containerfile example** — The example shows per-file COPYs and the old user append pattern. Should be updated to show consolidated COPY and strategy-aware user rendering. *Small effort.*

### Priority 2: Close to done

4. **HTML category card status indicators** — The triage data exists. Add CSS classes to cards based on `compute_triage()` output: green border for clean, amber for FIXME, red for manual. *Small effort (~30 min).*

5. **HTML Secrets tab** — Add a `section("secrets", "Secrets")` block to the template with a table of `snapshot.redactions`. Data is already available. *Small effort (~30 min).*

### Priority 3: Significantly improves demo quality

6. **Add interactive re-render to future work section** — The parked feature (browser-based re-rendering from snapshot with strategy picker, package toggles, and Containerfile preview) should be documented in the design doc's Future Work section as the next major evolution of the HTML report. *Small effort (writing only).*

7. **Risk-prioritize manual intervention list** — Sort by severity, group by inspector. Quick improvement to audit report scannability. *Small effort (~30 min).*

### Priority 4: Polish

8. **Update design doc: registry auth check as explicit step** — Add to Baseline Generation flow diagram. *Small effort.*

9. **Update design doc: kickstart user directives** — Add to kickstart file description. *Small effort.*

10. **Update design doc: CLI flags table** — Add `--user-strategy`. *Small effort.*

11. **Update design doc: progress messages** — Mention TTY-gated stderr progress in Runtime Model. *Small effort.*

---

## Items NOT included

The following are intentionally excluded from this review:

- Everything in the "Future Work" section of design.md (in-place migration, fleet analysis, snapshot diffing, enhanced cron conversion, semantic config diffing)
- The interactive re-render feature (parked for future design doc update)
- Per-user strategy overrides (explicitly out of scope per the `--user-strategy` spec)
