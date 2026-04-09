# Tied Variant Visibility

**Status:** Proposed
**Date:** 2026-04-09
**Author:** Team brainstorm
**Revision:** 1 (2026-04-09) — expanded scope, tiebreak precision, template contracts, audit report disambiguation, merge-notes discoverability, normalization accuracy

## Problem

When fleet merge produces ties (multiple variants of the same item with equal `fleet.count`), all tied variants are set to `include=False` and silently excluded from the output tarball. The HTML report flags ties in the triage summary, but the Containerfile and tarball are silently incomplete. A user who skips refine gets migration output quietly missing files.

This affects all item types processed through `_auto_select_variants()` in `fleet/merge.py`:

| Item type | Section | Merge call site |
|-----------|---------|-----------------|
| Config files | `config.files` | `merge.py:353` |
| Systemd drop-ins | `services.drop_ins` | `merge.py:377` |
| Quadlet units | `containers.quadlet_units` | `merge.py:440` |
| Compose files | `containers.compose_files` | `merge.py:441` |
| Non-RPM env files | `non_rpm_software.env_files` | `merge.py:462` |

All five share the same tie-at-the-top behavior: when the top two variants have equal fleet counts, every variant is set to `include=False`. The design in this spec applies uniformly to all five item types. Where an item type requires different treatment, that is called out explicitly.

This is silent data loss in migration output. It violates the user's reasonable expectation: "if yoinkc didn't warn me, my image has everything it needs."

**Discovery:** Identified during review of fleet merge behavior (2026-04-09). Did not surface during Container Guild demo but is a real UX/safety gap.

## Design Decisions

### 1. Level 1 Content Normalization

Normalize trailing whitespace and line endings before content hashing during variant grouping. This collapses semantically identical files that differ only in trivial whitespace into a single variant, eliminating noise ties.

**Where it goes:** In `_merge_content_items()` at the point where `variant_fn` computes the content hash. All five item types that pass through `_auto_select_variants()` use `variant_fn=lambda f: _content_hash(f.content)` (or equivalent). Normalization transforms content before hashing: `_content_hash(normalize(f.content))`. Compose files use a variant function based on sorted `(service, image)` tuples rather than raw content; normalization does not apply to that path since the tuple representation already discards whitespace.

When normalized variants collapse during merge, `_merge_content_items()` retains whichever variant was seen first (first-seen-wins within the `seen` dict). The collapsed variant's content is one representative copy, not all originals. The remaining raw variants are discarded at merge time. Downstream renderers see the surviving representative.

**Scope:** Trailing whitespace stripping and line ending normalization only. No comment stripping (comments are intentional operator state). No format-aware canonicalization (order-sensitive formats like PAM make this unsafe). Higher-level normalization is out of scope for this spec.

**Risk:** Low. Trailing whitespace is rarely semantically meaningful in the file formats yoinkc handles (INI, shell, sysctl, PAM, audit rules, systemd units, quadlet units, dotenv files). Edge cases could exist in formats where trailing whitespace is load-bearing (e.g., Makefile-derived files, heredoc content in shell scripts); these are unlikely in the config/drop-in/env-file paths yoinkc processes but cannot be categorically ruled out.

### 2. Deterministic Tiebreaker

When variants tie on `fleet.count` after normalization, pick a winner deterministically. The tiebreak procedure:

1. Sort tied variants by the full SHA-256 hex digest of raw content (lexicographic, ascending).
2. The first variant in sorted order wins.
3. For 3+ variants tied at the max count (e.g., 3 variants all at count 2), the same rule applies: all tied-at-max variants are sorted by full SHA-256 digest, and the first one wins. The remaining N-1 tied variants stay `include=False`.

**Hash implementation note:** `_content_hash()` currently truncates to 16 hex chars (`merge.py:25-26`). The tiebreaker must use the full 64-character SHA-256 digest for sort comparison to avoid collisions in the tiebreak order. The implementation should either: (a) change `_content_hash()` to return the full digest (used for both variant grouping and tiebreak sort), or (b) compute a separate full digest at tiebreak time. Option (a) is preferred since the truncated hash provides no meaningful performance benefit and the full digest eliminates collision risk for variant grouping as well.

Properties:

- Stable across runs (same input always picks the same winner)
- Host-order-independent (result does not depend on scan order)
- Reproducible (any user can verify the pick by computing SHA-256 of the raw content)

The picked variant gets `include=True`. Remaining tied variants stay `include=False`.

**Precedent:** Package tie-breaking in `packages.py:69-72` uses EVR comparison for deterministic selection.

### 3. Model Changes

Add to the models for all five item types that pass through `_auto_select_variants()` (config files, drop-ins, quadlet units, compose files, non-RPM env files):

- `tie: bool` -- True when this item was part of a tie group. Set on the winner AND the losers.
- `tie_winner: bool` -- True only on the variant that was auto-selected by the tiebreaker.
- Fleet count ratio is already available (`fleet.count` / total hosts).

These flags let every downstream renderer distinguish:
- **Unanimous consensus** (single variant, or all hosts agree): no label, no badge
- **Clear winner** (>50% but not unanimous): labeled with fleet ratio
- **Narrow winner** (exactly at threshold): labeled with fleet ratio
- **Tied, auto-resolved**: labeled with tie badge + fleet ratio

### 4. Consensus Threshold for Visibility

**Any config that is not unanimous gets labeled.** Threshold is 100% -- if even one host diverges, it appears in merge notes and gets a fleet ratio label in the report.

Rationale: A single divergent host is exactly the kind of silent drift that causes production issues. Starting strict is correct; the threshold can be relaxed later if labels prove noisy. You cannot retroactively surface information you suppressed.

**Unanimous configs get no label.** The absence of a label IS the signal that the fleet agreed. This makes non-unanimous labels stand out as the exception, improving the signal-to-noise ratio of the display.

## Output Surface Changes

### Containerfile

Add a "Tied items" category to the existing `config_inventory_comment()` block, structurally consistent with how modified/unowned/orphaned configs are already listed. This covers all item types that pass through `_auto_select_variants()`.

```
# Tied items resolved by content-hash tiebreaker (2):
#   etc/sysconfig/network-scripts/ifcfg-eth0  (config, 3/6 hosts each, 2 variants)
#   etc/systemd/system/app.service.d/override.conf  (drop-in, 2/4 hosts each, 2 variants)
#   See merge-notes.md for tie details
#   Review in report.html or run `yoinkc refine` to change selection
```

The picked winner is written to `config/` (or `quadlet/`, `drop-ins/`) as normal. Existing bulk COPY lines stay untouched. No per-file COPY restructuring.

### HTML Report

- Non-unanimous items display a fleet ratio label with color that varies by homogeneity level. Higher consensus = subtler color; ties = most prominent color. Exact color mapping is an implementation detail.
- Auto-resolved ties shift from `manual` triage severity to a lower "review recommended" severity, since a winner IS selected.
- Auto-picked tied variants carry a visible "tie winner (hash)" badge alongside their fleet count.
- Unanimous items have no additional label or badge.

**Template contract — required changes:**

The current template logic in `_config.html.j2` (line 18) computes `is_tied` as `not group_has_selected`. Once the tiebreaker auto-selects a winner, `group_has_selected` becomes `true` for all tie groups, so the existing `is_tied` check will never fire. The templates must be updated to use the new `tie` and `tie_winner` model flags instead of inferring tie state from `include`.

The same pattern applies to `html_report.py` (lines 688-725) where `unresolved_ties` is computed by checking for variant groups with no selected item. After the tiebreaker, this count will always be zero. The renderer must switch to counting items where `tie=True` and distinguish auto-resolved ties (has a `tie_winner=True` variant) from user-resolved ties (tie resolved via refine override).

Files requiring changes:

| File | Current logic | Required change |
|------|--------------|-----------------|
| `templates/report/_config.html.j2` (line 16-18) | `is_tied = (not group_has_selected) and ...` | Check `tie` flag on items: `is_tied = variants \| selectattr('item.tie', 'equalto', true) \| list \| length > 0`. Show "tie winner" badge when `tie_winner=True`, show "tie loser" styling when `tie=True, tie_winner=False`. |
| `templates/report/_summary.html.j2` (line 47-53) | Displays `unresolved_ties` count from renderer | Show auto-resolved tie count separately from unresolved (user-overridden) ties. After tiebreaker, "unresolved" means the user explicitly deselected the auto-pick without choosing a replacement. |
| `renderers/html_report.py` (lines 688-725) | Counts ties by looking for groups with no `include=True` variant | Count ties by checking for `tie=True` on any variant in the group. Separate auto-resolved ties (has `tie_winner=True`) from unresolved ties (no variant has `include=True`). |
| `templates/report/_services.html.j2` | If it mirrors `_config.html.j2` variant group logic for drop-ins | Apply same `tie`/`tie_winner` flag pattern. |
| `templates/report/_containers.html.j2` | If it mirrors variant group logic for quadlets/compose | Apply same `tie`/`tie_winner` flag pattern. |

### Refine UI

- Auto-picked tied variant shows "(auto-selected: tied)" label so it doesn't look like fleet consensus.
- User clicks a different variant to override the auto-pick.
- No structural change to the refine interaction. The existing variant toggle pattern handles resolution.
- Comparison view defaults to showing the auto-picked variant diffed against the next tied variant (most useful comparison for tie resolution).

### CLI Output

One-line summary appended to merge output when ties exist:

```
  6 hosts merged, threshold 50%
  2 items with tied variants (auto-resolved by content hash)
```

Zero ties = zero additional output.

### Merge Notes (new file)

New file: `merge-notes.md` in the tarball output. Contains:

- **Tied items:** path, item type, variant count, fleet counts per variant, which was auto-selected, content hash of each variant
- **Non-unanimous items:** path, item type, fleet ratio, winning variant's fleet count vs. total

This is the drill-down surface for investigating fleet merge decisions. Scoped to fleet ambiguity -- not secrets (that's `secrets-review.md`), not RPM diffs (that's `audit-report.md`).

**Discoverability requirements:**

- `renderers/readme.py`: Add `merge-notes.md` to the artifacts table (after `secrets-review.md`). Row: `| \`merge-notes.md\` | Fleet merge decisions — ties, non-unanimous items |`. Only include this row when the snapshot has fleet metadata.
- `renderers/containerfile/_config_tree.py`: Add a comment pointer in `config_inventory_comment()` when ties exist, e.g., `# See merge-notes.md for tie details`. This parallels the existing pattern where config inventory comments reference `audit-report.md` and `report.html`.

### Audit Report Disambiguation

`audit_report.py` currently marks any item with `include=False` as `[EXCLUDED]`. After this change, `include=False` can mean three different things:

1. **Below threshold:** Fleet count did not meet `min_prevalence`. The existing behavior.
2. **Tie loser:** Item was part of a tie group and lost the tiebreak (`tie=True, tie_winner=False, include=False`).
3. **Redacted:** Item was excluded by the secrets scanner (`redactions` list).

The audit report must distinguish these. Renderer precedence:

1. Check redactions first: if the item's path appears in `snapshot.redactions` with `kind=excluded`, label as `[REDACTED]` (existing behavior, unchanged).
2. Check tie flags: if `tie=True` and `tie_winner=False`, label as `[TIE LOSER]` with a note indicating which variant was auto-selected.
3. Otherwise, if `include=False`, label as `[EXCLUDED]` (below-threshold, existing behavior).

This applies to all item types in the audit report that can have content variants: config files, drop-ins, quadlet units, compose files, and non-RPM env files.

### secrets-review.md

Unchanged. Ties are not a secrets concern.

## Implementation Notes

### Structural Gap

Multiple renderers currently only see `include=False` -- they cannot distinguish tie-excluded from threshold-excluded from redacted. The `tie` and `tie_winner` flags on all five item type models must be implemented before any renderer changes. The audit report additionally needs redaction-awareness to complete the three-way disambiguation.

### Key Code Locations

| Component | File | Lines | Change |
|-----------|------|-------|--------|
| Content hash | `src/yoinkc/fleet/merge.py` | 25-26 | Switch from truncated (16 hex) to full SHA-256 digest |
| Normalization | `src/yoinkc/fleet/merge.py` | ~347-352, 365-369, 426-435, 456-460 | Inject normalize() into variant_fn lambdas for configs, drop-ins, quadlets, env files |
| Tiebreaker | `src/yoinkc/fleet/merge.py` | 119-155 | Add full-digest sort in `_auto_select_variants`, set `tie`/`tie_winner` flags |
| Model flags | Schema models for ConfigFileEntry, SystemdDropIn, QuadletUnit, ComposeFile, env file model | TBD | Add `tie`, `tie_winner` fields |
| Containerfile comment | `src/yoinkc/renderers/containerfile/_config_tree.py` | `config_inventory_comment()` | Add "Tied items" category with merge-notes.md pointer |
| HTML report — renderer | `src/yoinkc/renderers/html_report.py` | 688-725 | Switch tie detection from include-based to flag-based |
| HTML report — config template | `src/yoinkc/templates/report/_config.html.j2` | 16-18 | Replace `not group_has_selected` tie check with `tie`/`tie_winner` flag checks |
| HTML report — summary template | `src/yoinkc/templates/report/_summary.html.j2` | 47-53 | Distinguish auto-resolved from unresolved ties |
| HTML report — services template | `src/yoinkc/templates/report/_services.html.j2` | variant group logic | Apply `tie`/`tie_winner` pattern for drop-ins |
| HTML report — containers template | `src/yoinkc/templates/report/_containers.html.j2` | variant group logic | Apply `tie`/`tie_winner` pattern for quadlets/compose |
| Audit report | `src/yoinkc/renderers/audit_report.py` | `[EXCLUDED]` labels | Add `[TIE LOSER]` label with precedence rules |
| Readme artifacts | `src/yoinkc/renderers/readme.py` | artifacts table (~133-141) | Add `merge-notes.md` row for fleet merges |
| Refine label | `src/yoinkc/refine.py` | UI rendering | Add "(auto-selected: tied)" label |
| CLI warning | Fleet merge CLI entry point | TBD | One-line tie summary |
| Merge notes | New renderer | New file | `merge-notes.md` generation |

### Testing Strategy

Tests must cover three boundaries (per review analysis):

1. **Containerfile content test:** Given a snapshot with a tied config and a tied drop-in, assert the Containerfile inventory comment contains the "Tied items" category listing path, type, fleet counts, and variant count. Assert the merge-notes.md pointer comment is present.

2. **Tarball/disk test:** Assert the picked winner is present in the appropriate output directory (`config/`, `drop-ins/`, `quadlet/`). Assert `merge-notes.md` is present and contains tie details for all tied item types.

3. **Round-trip test:** Verify that a tie resolved via refine (user selects a different variant) causes the selected variant to appear in the Containerfile COPY normally and removes it from the "Tied items" comment block.

4. **HTML report test:** Tie badge renders on auto-picked variant using `tie`/`tie_winner` flags (not inferred from `include`). Non-unanimous items show fleet ratio labels. Unanimous items have no label. Verify that `is_tied` in `_config.html.j2` detects ties via the `tie` flag, not via `not group_has_selected`.

5. **Normalization test:** Two variants differing only in trailing whitespace collapse into one variant (no tie). Two variants with genuine content differences remain as separate variants. Compose file variant grouping (tuple-based) is unaffected by whitespace normalization.

6. **Deterministic tiebreaker test:** Given the same tied variants, the same winner is always picked regardless of input order. Test with 2-way and 3+-way ties.

7. **Audit report disambiguation test:** Given a snapshot with a tie loser, a below-threshold item, and a redacted item, assert the audit report labels them `[TIE LOSER]`, `[EXCLUDED]`, and `[REDACTED]` respectively.

8. **Readme artifacts test:** Assert `merge-notes.md` appears in the artifacts table for fleet merges and is absent for single-host renders.

## Out of Scope

- **Level 2+ normalization** (comment stripping, format-aware canonicalization): Stripping comments changes semantic content. Format-aware canonicalization is risky for order-sensitive formats (PAM). Both are future work if ever.
- **Non-zero exit codes for ties:** The yoinkc workflow is inspect -> refine -> export. The user can't resolve ties until refine is running, so blocking the initial inspect with a non-zero exit serves no purpose.
- **Pre-resolution of ties via CLI flags:** Future enhancement. For now, resolution happens in refine.
