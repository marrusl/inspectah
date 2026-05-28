# Test File Splits — Design Spec

**Date:** 2026-03-15
**Status:** Approved
**Scope:** Split three large test files into smaller, navigable modules grouped
by component. Create `conftest.py` for shared fixtures.

## Motivation

Three test files have grown large enough to hinder navigation:
- `test_renderer_outputs.py` (1,764 lines) — 17 classes + 7 bare test functions
- `test_plan_items.py` (1,463 lines) — 19 classes + 6 bare functions/helpers
- `test_inspectors.py` (1,208 lines) — 1 class + ~40 bare test functions

Splitting by component makes it easy to find tests for a given area and keeps
files focused.

## Approach

Split each file by domain affinity — group related tests together, combining
small groups to avoid trivially small files. Move shared fixtures to a new
`tests/conftest.py`. Four commits: conftest creation + one per original file.

## `conftest.py` — Shared Fixtures

Create `tests/conftest.py` with fixtures shared across split files.

### From `test_renderer_outputs.py`

- `_make_executor(pkg_list=None)` helper — builds a mock Executor
- `_build_snapshot(with_baseline)` helper — builds an InspectionSnapshot
- `outputs_with_baseline` fixture (`scope="module"`) — renders all outputs
  with baseline subtraction
- `outputs_no_baseline` fixture (`scope="module"`) — renders all outputs
  without baseline

### From `test_inspectors.py`

- `_fixture_executor(cmd, cwd=None)` helper (~100 lines) — mock command
  responses for all inspectors
- `fixture_executor` fixture — wraps `_fixture_executor` in an Executor
- `host_root` fixture — returns the test fixtures directory path
- `_mock_user_namespace` fixture — patches user namespace check. **Note:**
  this fixture is `autouse=True` in the original file. In `conftest.py`,
  autouse applies to ALL tests in the directory. The patch
  (`preflight_mod.in_user_namespace = False`) is benign for non-inspector
  tests, but the implementer should verify no tests rely on the unpatched
  value. If any do, remove autouse and add the fixture explicitly to
  inspector test files.

### From `test_plan_items.py`

- `_env()` helper — returns `Environment(autoescape=True)`. Used by classes
  in 3 destination files. One-liner, belongs in conftest.

### Scope Note

The renderer fixtures are `scope="module"`. After the split, each new test
module gets its own fixture instance. This is slightly less efficient
(snapshot built per file instead of once total) but more correct — tests in
different files should not share mutable state. Correctness over speed.

## `test_renderer_outputs.py` Split

Split into 4 files. Original deleted.

| New file | Contents | ~Lines |
|----------|----------|--------|
| `test_containerfile_output.py` | TestContainerfile, TestContainerfileQuality, TestKernelKargs, TestBaselineModes, TestEdgeCases, TestServicePackageFiltering, + bare ordering tests (gpg_key_copy, timer_copy, repo_copy, config_tree_timers, bootc_lint, nonrpm_nodejs) | ~750 |
| `test_html_report_output.py` | TestHtmlReport, TestHtmlStructure | ~260 |
| `test_audit_report_output.py` | TestAuditReport, TestKickstart, TestReadme, TestSecretsReview | ~270 |
| `test_fleet_output.py` | TestFleetColor, TestFleetBanner, TestFleetPrevalenceBadge, TestFleetConfigPassthrough, TestFleetVariantGrouping | ~340 |

### Design Decisions

- **Containerfile gets the lion's share** — quality checks, kernel kargs,
  ordering tests, edge cases, and service filtering all test containerfile
  rendering behavior.
- **Audit + kickstart + readme + secrets grouped** — each is 40-70 lines,
  not worth separate files.
- **Fleet is its own file** — coherent domain, added recently, will grow.

## `test_plan_items.py` Split

Split into 5 files. Original deleted.

| New file | Contents | ~Lines |
|----------|----------|--------|
| `test_plan_packages.py` | TestLeafAutoSlimming, TestSourceRepo, TestRepoFileClassification, TestRepoCascadeContainerfile, TestDeepVersionPatterns, TestPythonVersionMap | ~450 |
| `test_plan_services.py` | TestServiceBaselinePresets, TestCronToOnCalendar, TestCronCommandExtraction, TestRpmOwnedCronFiltering | ~250 |
| `test_plan_containerfile.py` | TestMultiStageContainerfile, TestContainerfileExclusion, TestAuditReportExcluded, TestConfigDiffFallback, TestSanitizeShellValue, TestCrossMajorWarning, test_html_diff_spans, test_storage_recommendation_mapping | ~450 |
| `test_plan_users.py` | TestUserStrategies, TestUserGroupIncludeKey | ~290 |
| `test_plan_include.py` | TestIncludeFieldDefaults, test_profile_flag_rejected, test_comps_file_flag_rejected, test_all_features_render_together | ~130 |

### Design Decisions

- **Packages** groups RPM/repo/dependency-related classes.
- **Services** groups service presets and cron/timer conversion.
- **Containerfile** groups broader containerfile rendering tests.
- **Users** is its own file — `TestUserStrategies` alone is ~240 lines.
- **Include** gets the CLI flag tests and the cross-cutting smoke test
  (`test_all_features_render_together`) since these don't fit elsewhere.
- **Bare functions** — `test_html_diff_spans` and
  `test_storage_recommendation_mapping` go to containerfile (closest domain).
- `_env()` helper moves to `conftest.py` (used by 3 destination files).

## `test_inspectors.py` Split

Split into 4 files. Original deleted.

| New file | Contents | ~Lines |
|----------|----------|--------|
| `test_inspector_rpm.py` | test_parse_nevr, test_parse_rpm_qa, test_parse_rpm_va, test_rpm_inspector_with_fixtures, test_rpm_inspector_with_baseline_file, test_rpm_inspector_captures_gpg_keys, test_collect_gpg_keys_resolves_dnf_vars, test_source_repo_populated_via_dnf_repoquery | ~160 |
| `test_inspector_services.py` | test_service_inspector_with_fixtures, test_scan_unit_files_from_fs, test_preset_glob_rules_applied, test_preset_glob_first_match_wins, test_service_inspector_resolves_owning_packages, test_service_inspector_detects_drop_ins | ~120 |
| `test_inspector_domains.py` | test_config_inspector, test_network_inspector, test_storage_inspector, test_scheduled_tasks_inspector, test_container_inspector, test_non_rpm_software_inspector (+ env_files, redaction), test_kernel_boot_inspector (+ tuned), test_selinux_inspector | ~500 |
| `test_inspector_integration.py` | test_users_groups (classification, group_strategy, fixtures), test_run_all_with_fixtures, test_run_all_no_baseline_*, test_cross_major_warning_*, test_hostname_*, test_snapshot_roundtrip_*, test_classify_leaf_auto_*, TestInspectorFailures. Helpers `_no_baseline_executor` and `_failing_executor` stay in this file (not shared). | ~450 |

### Design Decisions

- **RPM** gets its own file — parser tests are distinct from fixture-based
  inspector tests.
- **Services** gets its own file — 6 tests covering presets, glob rules,
  owning packages, drop-ins.
- **Domains** groups inspectors with only 1-3 tests each (config, network,
  storage, scheduled_tasks, container, non_rpm, kernel_boot, selinux).
- **Integration** groups cross-cutting tests — `run_all`, baseline handling,
  hostname detection, snapshot roundtrip, leaf/auto classification, plus
  `TestInspectorFailures`.
- **Users/groups** in integration — classification and group_strategy tests
  are about the framework's user-handling logic.

## Backward Compatibility

- **Four commits** — conftest creation + one per original file split. Each
  split commit deletes the original and creates the new files.
- **No behavioral changes** — tests move between files, nothing else changes.
- **Verification** — run `pytest tests/ -v` after each commit. Same test
  count, same results.

## Migration Strategy

Commit order:

1. **Create `conftest.py`** — extract shared fixtures from both
   `test_renderer_outputs.py` and `test_inspectors.py`. Update the originals
   to import from conftest. All tests pass at this point.
2. **Split `test_renderer_outputs.py`** → 4 files, delete original.
3. **Split `test_plan_items.py`** → 5 files, delete original.
4. **Split `test_inspectors.py`** → 4 files, delete original.
