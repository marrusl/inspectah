# Test File Splits — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development
> (if subagents available) or superpowers:executing-plans to implement this
> plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split three large test files into smaller modules grouped by
component, with shared fixtures in `conftest.py`.

**Architecture:** Four commits: (1) create `conftest.py` with shared fixtures,
(2-4) split each original file into domain-grouped modules. Pure structural
refactor — same tests, same results, different file locations.

**Tech Stack:** Python 3, pytest

**Spec:** `docs/specs/2026-03-15-test-file-splits-design.md`

---

## Extraction Pattern

For each split:
1. Read the original file fully
2. Create new files with the assigned classes/functions
3. Add necessary imports to each new file (copy from original, remove unused)
4. Shared fixtures come from `conftest.py` (pytest auto-discovers them)
5. Delete the original file
6. Run `pytest tests/ -v` — same test count, all pass

## Fixture Scope Note

The renderer fixtures (`outputs_with_baseline`, `outputs_no_baseline`) are
`scope="module"`. After the split, they will execute once per new test module
(4x total instead of 1x). This is intentional — correctness over speed.
Each module gets its own fixture instance with no shared mutable state.
Do NOT change to `scope="session"`.

## Helper Import Pattern

Plain helpers (`_make_executor`, `_build_snapshot`, `_fixture_executor`,
`_env`) are not fixtures — pytest doesn't auto-discover them. Each test file
that uses a helper must `from conftest import _helper_name`. This is a
pragmatic choice; a separate `tests/helpers.py` module would also work but
adds another file for marginal benefit.

---

## Chunk 1: conftest.py and file splits

### Task 1: Create `conftest.py`

**Files:**
- Create: `tests/conftest.py`
- Modify: `tests/test_renderer_outputs.py` (remove extracted fixtures)
- Modify: `tests/test_inspectors.py` (remove extracted fixtures)
- Modify: `tests/test_plan_items.py` (remove `_env` helper)

- [ ] **Step 1: Read all three source files**

Read `tests/test_renderer_outputs.py`, `tests/test_plan_items.py`, and
`tests/test_inspectors.py` fully to understand fixture dependencies.

- [ ] **Step 2: Create `tests/conftest.py`**

Extract and combine these fixtures/helpers:

**From `test_renderer_outputs.py`:**
- `_make_executor(pkg_list=None)` helper (L49-88)
- `_build_snapshot(with_baseline)` helper (L89-99)
- `outputs_with_baseline` fixture, `scope="module"` (L100-108)
- `outputs_no_baseline` fixture, `scope="module"` (L109-121)
- Include all imports these depend on

**From `test_inspectors.py`:**
- `_fixture_executor(cmd, cwd=None)` helper (L27-125) — large mock
  command dispatch function
- `fixture_executor` fixture (L126-130)
- `host_root` fixture (L131-135)
- `_mock_user_namespace` fixture (L20-25) — `autouse=True`. This will
  apply to ALL tests. The patch (`preflight_mod.in_user_namespace = False`)
  is benign for non-inspector tests, but verify after setup.
- Include all imports these depend on

**From `test_plan_items.py`:**
- `_env()` helper (L54) — one-liner: `return Environment(autoescape=True)`
- Include its `jinja2` import

- [ ] **Step 3: Update originals to use conftest fixtures**

Remove the extracted fixtures/helpers from each original file. Since pytest
auto-discovers `conftest.py` fixtures, the test files just need the
definitions removed — no explicit imports needed for fixtures. For helpers
(`_make_executor`, `_build_snapshot`, `_fixture_executor`, `_env`), add
`from conftest import ...` in each file that uses them.

- [ ] **Step 4: Run tests**

```bash
python -m pytest tests/ -v
```

Expected: all tests pass, same count as before.

- [ ] **Step 5: Commit**

```bash
git add tests/conftest.py tests/test_renderer_outputs.py tests/test_inspectors.py tests/test_plan_items.py
git commit -m "test: extract shared fixtures into conftest.py

Move shared test fixtures and helpers from test_renderer_outputs.py,
test_inspectors.py, and test_plan_items.py into tests/conftest.py.
Prepares for splitting these files by component.

Assisted-by: Claude <noreply@anthropic.com>"
```

---

### Task 2: Split `test_renderer_outputs.py` → 4 files

**Files:**
- Create: `tests/test_containerfile_output.py`
- Create: `tests/test_html_report_output.py`
- Create: `tests/test_audit_report_output.py`
- Create: `tests/test_fleet_output.py`
- Delete: `tests/test_renderer_outputs.py`

- [ ] **Step 1: Read the (now slimmed) `test_renderer_outputs.py`**

After Task 1, the fixtures are already in conftest. The file contains 17
classes and 7 bare test functions.

- [ ] **Step 2: Create `test_containerfile_output.py`**

Move these classes and functions:
- `TestContainerfile`
- `TestContainerfileQuality`
- `TestKernelKargs`
- `TestBaselineModes`
- `TestEdgeCases`
- `TestServicePackageFiltering`
- `test_gpg_key_copy_precedes_repo_copy`
- `test_systemd_timer_copy_precedes_enable`
- `test_repo_copy_precedes_dnf_install`
- `test_config_tree_timers_excluded_from_services_enable`
- `test_bootc_container_lint_is_last_run`
- `test_nonrpm_emits_nodejs_prereq_when_missing_from_packages`
- `test_nonrpm_no_nodejs_prereq_when_already_in_packages`

Add imports needed by these classes (copy from original, remove unused).
Fixtures `outputs_with_baseline`, `outputs_no_baseline` come from conftest
automatically.

- [ ] **Step 3: Create `test_html_report_output.py`**

Move:
- `TestHtmlReport`
- `TestHtmlStructure`

- [ ] **Step 4: Create `test_audit_report_output.py`**

Move:
- `TestAuditReport`
- `TestKickstart`
- `TestReadme`
- `TestSecretsReview`

- [ ] **Step 5: Create `test_fleet_output.py`**

Move:
- `TestFleetColor`
- `TestFleetBanner`
- `TestFleetPrevalenceBadge`
- `TestFleetConfigPassthrough`
- `TestFleetVariantGrouping`

- [ ] **Step 6: Delete original and verify**

```bash
rm tests/test_renderer_outputs.py
python -m pytest tests/ -v
```

Expected: same test count, all pass.

- [ ] **Step 7: Commit**

```bash
git add tests/test_containerfile_output.py tests/test_html_report_output.py tests/test_audit_report_output.py tests/test_fleet_output.py
git rm tests/test_renderer_outputs.py
git commit -m "test: split test_renderer_outputs.py into 4 domain files

Split by renderer domain: containerfile output (+ quality, ordering, edge
cases), HTML report, audit/kickstart/readme/secrets, and fleet rendering.

Assisted-by: Claude <noreply@anthropic.com>"
```

---

### Task 3: Split `test_plan_items.py` → 5 files

**Files:**
- Create: `tests/test_plan_packages.py`
- Create: `tests/test_plan_services.py`
- Create: `tests/test_plan_containerfile.py`
- Create: `tests/test_plan_users.py`
- Create: `tests/test_plan_include.py`
- Delete: `tests/test_plan_items.py`

- [ ] **Step 1: Read `test_plan_items.py`**

19 classes + 5 bare test functions. `_env()` helper is now in conftest.

- [ ] **Step 2: Create `test_plan_packages.py`**

Move:
- `TestLeafAutoSlimming`
- `TestSourceRepo`
- `TestRepoFileClassification`
- `TestRepoCascadeContainerfile`
- `TestDeepVersionPatterns`
- `TestPythonVersionMap`

- [ ] **Step 3: Create `test_plan_services.py`**

Move:
- `TestServiceBaselinePresets`
- `TestCronToOnCalendar`
- `TestCronCommandExtraction`
- `TestRpmOwnedCronFiltering`

- [ ] **Step 4: Create `test_plan_containerfile.py`**

Move:
- `TestMultiStageContainerfile`
- `TestContainerfileExclusion`
- `TestAuditReportExcluded`
- `TestConfigDiffFallback`
- `TestSanitizeShellValue`
- `TestCrossMajorWarning`
- `test_html_diff_spans`
- `test_storage_recommendation_mapping`

- [ ] **Step 5: Create `test_plan_users.py`**

Move:
- `TestUserStrategies`
- `TestUserGroupIncludeKey`

- [ ] **Step 6: Create `test_plan_include.py`**

Move:
- `TestIncludeFieldDefaults`
- `test_profile_flag_rejected`
- `test_comps_file_flag_rejected`
- `test_all_features_render_together`

- [ ] **Step 7: Delete original and verify**

```bash
rm tests/test_plan_items.py
python -m pytest tests/ -v
```

Expected: same test count, all pass.

- [ ] **Step 8: Commit**

```bash
git add tests/test_plan_packages.py tests/test_plan_services.py tests/test_plan_containerfile.py tests/test_plan_users.py tests/test_plan_include.py
git rm tests/test_plan_items.py
git commit -m "test: split test_plan_items.py into 5 domain files

Split by feature area: packages/repos, services/cron, containerfile
rendering, users/groups, and include-field defaults.

Assisted-by: Claude <noreply@anthropic.com>"
```

---

### Task 4: Split `test_inspectors.py` → 4 files

**Files:**
- Create: `tests/test_inspector_rpm.py`
- Create: `tests/test_inspector_services.py`
- Create: `tests/test_inspector_domains.py`
- Create: `tests/test_inspector_integration.py`
- Delete: `tests/test_inspectors.py`

- [ ] **Step 1: Read `test_inspectors.py`**

1 class + ~45 bare test functions. Shared fixtures are in conftest.

- [ ] **Step 2: Create `test_inspector_rpm.py`**

Move:
- `test_parse_nevr`
- `test_parse_rpm_qa`
- `test_parse_rpm_va`
- `test_rpm_inspector_with_fixtures`
- `test_rpm_inspector_with_baseline_file`
- `test_rpm_inspector_captures_gpg_keys`
- `test_collect_gpg_keys_resolves_dnf_vars`
- `test_source_repo_populated_via_dnf_repoquery`

Add imports: `from yoinkc.inspectors.rpm import _parse_nevr, _parse_rpm_qa, _parse_rpm_va`
(only this file needs the RPM parser imports).

- [ ] **Step 3: Create `test_inspector_services.py`**

Move:
- `test_service_inspector_with_fixtures`
- `test_scan_unit_files_from_fs`
- `test_preset_glob_rules_applied`
- `test_preset_glob_first_match_wins`
- `test_service_inspector_resolves_owning_packages`
- `test_service_inspector_detects_drop_ins`

- [ ] **Step 4: Create `test_inspector_domains.py`**

Move all single-inspector tests for: config, network, storage,
scheduled_tasks, container, non_rpm_software (+ env_files, redaction),
kernel_boot (+ tuned), selinux.

These are the inspectors with only 1-3 test functions each.

- [ ] **Step 5: Create `test_inspector_integration.py`**

Move:
- All `test_users_groups*` and `test_user_classification*` and
  `test_group_strategy*` functions
- `test_run_all_with_fixtures`
- `test_run_all_no_baseline_*` (3-4 functions)
- `test_cross_major_warning_*` (2 functions)
- `test_hostname_*` (3 functions + parametrize)
- `test_snapshot_roundtrip_with_baseline`
- `test_classify_leaf_auto_*` (3 functions)
- `TestInspectorFailures` class

Also keep these helpers LOCAL to this file (not conftest):
- `_no_baseline_executor` — used only by no_baseline tests
- `_failing_executor` — used only by TestInspectorFailures

- [ ] **Step 6: Delete original and verify**

```bash
rm tests/test_inspectors.py
python -m pytest tests/ -v
```

Expected: same test count, all pass.

- [ ] **Step 7: Commit**

```bash
git add tests/test_inspector_rpm.py tests/test_inspector_services.py tests/test_inspector_domains.py tests/test_inspector_integration.py
git rm tests/test_inspectors.py
git commit -m "test: split test_inspectors.py into 4 domain files

Split by inspector domain: RPM (parsers + fixtures), services (presets,
glob rules, drop-ins), grouped domains (config through selinux), and
integration (run_all, baseline, hostname, users/groups).

Assisted-by: Claude <noreply@anthropic.com>"
```

---

### Task 5: Final verification

- [ ] **Step 1: Run full test suite**

```bash
python -m pytest tests/ -v
```

Expected: all tests pass, same total count as before the refactor.

- [ ] **Step 2: Verify no orphaned test file**

```bash
ls tests/test_renderer_outputs.py tests/test_plan_items.py tests/test_inspectors.py 2>&1
```

Expected: "No such file or directory" for all three.

- [ ] **Step 3: Verify new file count**

```bash
ls tests/test_*.py | wc -l
```

Expected: original count minus 3 originals plus 13 new files + conftest.
