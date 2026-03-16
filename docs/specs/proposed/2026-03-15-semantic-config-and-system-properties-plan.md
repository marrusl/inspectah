# Semantic Config Labels & System Properties Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add semantic category labels to config files (8 path-based categories) and detect system properties (locale, timezone, alternatives) in the kernel_boot inspector.

**Architecture:** Part A adds a `ConfigCategory` enum + `category` field to `ConfigFileEntry`, a path classifier function in the config inspector, and a Category column in the HTML report. Part B adds `locale`, `timezone`, and `alternatives` fields to `KernelBootSection`, file-based detection in the kernel_boot inspector, and new subsections in the Kernel/Boot tab. Single `SCHEMA_VERSION` bump covers both parts.

**Tech Stack:** Python (Pydantic models), Jinja2 templates, PatternFly 6 CSS, pytest.

**Spec:** `docs/specs/proposed/2026-03-15-semantic-config-and-system-properties-design.md`

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `src/yoinkc/schema.py` | Add `ConfigCategory` enum, `AlternativeEntry` model, new fields |
| Modify | `src/yoinkc/inspectors/config.py` | Add `classify_config_path()`, call at construction sites |
| Modify | `src/yoinkc/inspectors/kernel_boot.py` | Add locale, timezone, alternatives detection |
| Modify | `src/yoinkc/templates/report/_config.html.j2` | Add Category column |
| Modify | `src/yoinkc/templates/report/_kernel_boot.html.j2` | Add System Properties + Alternatives sections |
| Modify | `src/yoinkc/renderers/html_report.py` | Add `category` to config file dicts |
| Modify | `src/yoinkc/fleet/merge.py` | Add alternatives union merge, locale/timezone first-wins |
| Create | `tests/test_config_category.py` | Tests for `classify_config_path()` |
| Modify | `tests/test_inspector_domains.py` | Tests for locale/timezone/alternatives detection |

## Chunk 1: Config File Categories (Part A)

### Task 1: Schema — Add `ConfigCategory` Enum and Field

**Files:**
- Modify: `src/yoinkc/schema.py`

- [ ] **Step 1: Add `ConfigCategory` enum** after `ConfigFileKind` (after line 131):

```python
class ConfigCategory(str, Enum):
    """Semantic category derived from config file path."""
    TMPFILES = "tmpfiles"
    ENVIRONMENT = "environment"
    AUDIT = "audit"
    LIBRARY_PATH = "library_path"
    JOURNAL = "journal"
    LOGROTATE = "logrotate"
    AUTOMOUNT = "automount"
    SYSCTL = "sysctl"
    OTHER = "other"
```

- [ ] **Step 2: Add `category` field to `ConfigFileEntry`** (after the `kind` field, ~line 137):

```python
category: ConfigCategory = ConfigCategory.OTHER
```

- [ ] **Step 3: Do NOT bump `SCHEMA_VERSION` yet** — it will be bumped in Task 5 after Part B fields are also added, so the single bump covers both parts.

- [ ] **Step 4: Commit.**

```bash
git add src/yoinkc/schema.py
git commit -m "feat(schema): add ConfigCategory enum and category field

Defaults to OTHER for backward compatibility.

Assisted-by: Claude Code"
```

### Task 2: Config Path Classifier — TDD

**Files:**
- Create: `tests/test_config_category.py`
- Modify: `src/yoinkc/inspectors/config.py`

- [ ] **Step 1: Write tests** for `classify_config_path()`:

```python
"""Tests for config file path classification."""
import pytest
from yoinkc.inspectors.config import classify_config_path
from yoinkc.schema import ConfigCategory


@pytest.mark.parametrize("path, expected", [
    # tmpfiles
    ("/etc/tmpfiles.d/myapp.conf", ConfigCategory.TMPFILES),
    ("/etc/tmpfiles.d/nested/file.conf", ConfigCategory.TMPFILES),
    # environment
    ("/etc/environment", ConfigCategory.ENVIRONMENT),
    ("/etc/profile.d/custom.sh", ConfigCategory.ENVIRONMENT),
    ("/etc/profile.d/proxy.sh", ConfigCategory.ENVIRONMENT),
    # audit
    ("/etc/audit/rules.d/custom.rules", ConfigCategory.AUDIT),
    # library path
    ("/etc/ld.so.conf.d/custom.conf", ConfigCategory.LIBRARY_PATH),
    # journal
    ("/etc/systemd/journald.conf.d/rate-limit.conf", ConfigCategory.JOURNAL),
    # logrotate
    ("/etc/logrotate.d/myapp", ConfigCategory.LOGROTATE),
    # automount
    ("/etc/auto.master", ConfigCategory.AUTOMOUNT),
    ("/etc/auto.misc", ConfigCategory.AUTOMOUNT),
    ("/etc/auto.nfs", ConfigCategory.AUTOMOUNT),
    # sysctl
    ("/etc/sysctl.d/99-custom.conf", ConfigCategory.SYSCTL),
    ("/etc/sysctl.conf", ConfigCategory.SYSCTL),
    # other — no match
    ("/etc/nginx/nginx.conf", ConfigCategory.OTHER),
    ("/etc/ssh/sshd_config", ConfigCategory.OTHER),
    ("/etc/fstab", ConfigCategory.OTHER),
    # edge cases
    ("/etc/profile.d.bak", ConfigCategory.OTHER),  # not a directory prefix
    ("/etc/sysctl.conf.bak", ConfigCategory.OTHER),  # exact match only
    ("/etc/environment.d/50-custom.conf", ConfigCategory.OTHER),  # not in scope (systemd env generators, different from /etc/environment)
])
def test_classify_config_path(path, expected):
    assert classify_config_path(path) == expected
```

- [ ] **Step 2: Run tests to verify they fail.**

```bash
pytest tests/test_config_category.py -v
```

Expected: ImportError — `classify_config_path` not defined yet.

- [ ] **Step 3: Implement `classify_config_path()`** in `src/yoinkc/inspectors/config.py`. Add after the exclusion list (~line 220), before the `run()` function:

```python
from yoinkc.schema import ConfigCategory

_CATEGORY_RULES: list[tuple[ConfigCategory, list[str]]] = [
    (ConfigCategory.TMPFILES, ["/etc/tmpfiles.d/"]),
    (ConfigCategory.ENVIRONMENT, ["/etc/environment", "/etc/profile.d/"]),
    (ConfigCategory.AUDIT, ["/etc/audit/rules.d/"]),
    (ConfigCategory.LIBRARY_PATH, ["/etc/ld.so.conf.d/"]),
    (ConfigCategory.JOURNAL, ["/etc/systemd/journald.conf.d/"]),
    (ConfigCategory.LOGROTATE, ["/etc/logrotate.d/"]),
    (ConfigCategory.AUTOMOUNT, ["/etc/auto.master", "/etc/auto."]),
    (ConfigCategory.SYSCTL, ["/etc/sysctl.d/", "/etc/sysctl.conf"]),
]


def classify_config_path(path: str) -> ConfigCategory:
    """Classify a config file path into a semantic category."""
    for category, prefixes in _CATEGORY_RULES:
        for prefix in prefixes:
            if path == prefix or path.startswith(prefix):
                return category
    return ConfigCategory.OTHER
```

Note: `ConfigCategory` may already be importable if schema imports are at the top of the file. Check existing imports and add to them.

- [ ] **Step 4: Run tests to verify they pass.**

```bash
pytest tests/test_config_category.py -v
```

Expected: all pass.

- [ ] **Step 5: Commit.**

```bash
git add tests/test_config_category.py src/yoinkc/inspectors/config.py
git commit -m "feat(config): add path-based category classifier

8 semantic categories: tmpfiles, environment, audit, library_path,
journal, logrotate, automount, sysctl. Defaults to other.

Assisted-by: Claude Code"
```

### Task 3: Wire Classifier into Config Inspector

**Files:**
- Modify: `src/yoinkc/inspectors/config.py`

- [ ] **Step 1: Add `category=classify_config_path(path)` to all 3 `ConfigFileEntry` construction sites.**

**Site 1** (~line 403, RPM-owned modified):
```python
ConfigFileEntry(
    path=path,
    kind=ConfigFileKind.RPM_OWNED_MODIFIED,
    category=classify_config_path(path),
    content=content,
    rpm_va_flags=entry.flags,
    package=entry.package,
    diff_against_rpm=diff_against_rpm,
)
```

**Site 2** (~line 441, unowned):
```python
ConfigFileEntry(
    path=path_str,
    kind=ConfigFileKind.UNOWNED,
    category=classify_config_path(path_str),
    content=content,
    rpm_va_flags=None,
    package=None,
    diff_against_rpm=None,
)
```

**Site 3** (~line 474, orphaned):
```python
ConfigFileEntry(
    path=path_str,
    kind=ConfigFileKind.ORPHANED,
    category=classify_config_path(path_str),
    content=content,
    rpm_va_flags=None,
    package=pkg_name,
    diff_against_rpm=None,
)
```

- [ ] **Step 2: Run the full test suite** to confirm nothing breaks.

```bash
pytest tests/ -v
```

Expected: all pass.

- [ ] **Step 3: Commit.**

```bash
git add src/yoinkc/inspectors/config.py
git commit -m "feat(config): wire category classifier into all detection paths

RPM-owned modified, unowned, and orphaned config files all get
semantic category labels at construction time.

Assisted-by: Claude Code"
```

### Task 4: Add Category Column to Config Template

**Files:**
- Modify: `src/yoinkc/templates/report/_config.html.j2`

- [ ] **Step 1: Read `_config.html.j2`** to find the exact table header and row markup.

- [ ] **Step 2: Add "Category" column header** to the `<th>` row (line 5). Insert after "Kind":

```html
<th scope="col">Category</th>
```

- [ ] **Step 3: Add category cell to each table row.** For each `<tr>` that displays a config file, add a `<td>` with the category value. Use a human-readable display name. Add after the Kind cell:

```html
<td>{{ f.category | replace('_', ' ') | title }}</td>
```

This converts `library_path` → `Library Path`, `tmpfiles` → `Tmpfiles`, etc. If the template uses a different variable name for the config entry (e.g., `item` vs `f`), match whatever is used.

- [ ] **Step 4: Update `_prepare_config_files()` in `html_report.py`.** This function (line 371) converts `ConfigFileEntry` models into plain dicts. The `category` field must be added explicitly to the dict, using the enum's string value:

```python
"category": f.category.value,
```

Add this alongside the existing `"kind"`, `"path"`, etc. keys in the dict comprehension/construction. The template then receives `f.category` as a plain string (e.g., `"library_path"`).

- [ ] **Step 5: Run tests.**

```bash
pytest tests/ -v
```

- [ ] **Step 6: Commit.**

```bash
git add src/yoinkc/templates/report/_config.html.j2 src/yoinkc/renderers/html_report.py
git commit -m "feat(report): add Category column to config files table

Displays semantic category (Tmpfiles, Audit, Sysctl, etc.) for each
config file entry.

Assisted-by: Claude Code"
```

## Chunk 2: System Properties (Part B)

### Task 5: Schema — Add System Property Fields

**Files:**
- Modify: `src/yoinkc/schema.py`

- [ ] **Step 1: Add `AlternativeEntry` model** near the other kernel_boot models:

```python
class AlternativeEntry(BaseModel):
    """A system alternative (update-alternatives entry)."""
    name: str
    path: str
    status: str  # "auto" or "manual"
```

- [ ] **Step 2: Add new fields to `KernelBootSection`** (after existing fields, ~line 469):

```python
locale: Optional[str] = None
timezone: Optional[str] = None
alternatives: List[AlternativeEntry] = Field(default_factory=list)
```

Add `AlternativeEntry` import if using `List` typing (follow existing patterns in the file).

- [ ] **Step 3: Bump `SCHEMA_VERSION`** from 7 to 8 (line 511). This single bump covers both Part A (ConfigCategory) and Part B (system properties).

- [ ] **Step 4: Commit.**

```bash
git add src/yoinkc/schema.py
git commit -m "feat(schema): add locale, timezone, alternatives to KernelBootSection

New AlternativeEntry model for update-alternatives state.
All fields optional with defaults for backward compatibility.
SCHEMA_VERSION 7 -> 8 (covers both config categories and system properties).

Assisted-by: Claude Code"
```

### Task 6: Detect System Properties — TDD

**Files:**
- Modify: `tests/test_inspector_domains.py` (or create `tests/test_system_properties.py` if cleaner)
- Modify: `src/yoinkc/inspectors/kernel_boot.py`

- [ ] **Step 1: Write tests for locale detection.** Create a temp directory structure with `/etc/locale.conf` containing `LANG=en_US.UTF-8`:

```python
def test_detect_locale(tmp_path):
    locale_conf = tmp_path / "etc" / "locale.conf"
    locale_conf.parent.mkdir(parents=True)
    locale_conf.write_text("LANG=en_US.UTF-8\n")

    result = run(host_root=tmp_path, executor=None)
    assert result.locale == "en_US.UTF-8"


def test_detect_locale_missing(tmp_path):
    (tmp_path / "etc").mkdir(parents=True)

    result = run(host_root=tmp_path, executor=None)
    assert result.locale is None
```

- [ ] **Step 2: Write tests for timezone detection:**

```python
def test_detect_timezone(tmp_path):
    etc = tmp_path / "etc"
    etc.mkdir(parents=True)
    zoneinfo = tmp_path / "usr" / "share" / "zoneinfo" / "America" / "New_York"
    zoneinfo.parent.mkdir(parents=True)
    zoneinfo.write_text("")  # content doesn't matter
    localtime = etc / "localtime"
    localtime.symlink_to(zoneinfo)

    result = run(host_root=tmp_path, executor=None)
    assert result.timezone == "America/New_York"


def test_detect_timezone_missing(tmp_path):
    (tmp_path / "etc").mkdir(parents=True)

    result = run(host_root=tmp_path, executor=None)
    assert result.timezone is None
```

- [ ] **Step 3: Write tests for alternatives detection:**

```python
def test_detect_alternatives_auto(tmp_path):
    # Set up /etc/alternatives/ symlinks
    alt_dir = tmp_path / "etc" / "alternatives"
    alt_dir.mkdir(parents=True)
    target = tmp_path / "usr" / "bin" / "java-17"
    target.parent.mkdir(parents=True)
    target.write_text("")
    (alt_dir / "java").symlink_to(target)

    # Set up /var/lib/alternatives/ status files
    var_alt = tmp_path / "var" / "lib" / "alternatives"
    var_alt.mkdir(parents=True)
    (var_alt / "java").write_text("auto\n/usr/bin/java\n")

    result = run(host_root=tmp_path, executor=None)
    assert len(result.alternatives) == 1
    assert result.alternatives[0].name == "java"
    assert result.alternatives[0].status == "auto"
    # os.readlink returns the raw symlink target
    assert str(target) in result.alternatives[0].path


def test_detect_alternatives_manual(tmp_path):
    alt_dir = tmp_path / "etc" / "alternatives"
    alt_dir.mkdir(parents=True)
    target = tmp_path / "usr" / "bin" / "python3.11"
    target.parent.mkdir(parents=True)
    target.write_text("")
    (alt_dir / "python3").symlink_to(target)

    var_alt = tmp_path / "var" / "lib" / "alternatives"
    var_alt.mkdir(parents=True)
    (var_alt / "python3").write_text("manual\n/usr/bin/python3\n")

    result = run(host_root=tmp_path, executor=None)
    assert len(result.alternatives) == 1
    assert result.alternatives[0].name == "python3"
    assert result.alternatives[0].status == "manual"


def test_detect_alternatives_empty(tmp_path):
    (tmp_path / "etc").mkdir(parents=True)

    result = run(host_root=tmp_path, executor=None)
    assert result.alternatives == []
```

- [ ] **Step 4: Run tests to verify they fail.**

```bash
pytest tests/test_inspector_domains.py -v -k "locale or timezone or alternatives"
```

Expected: failures — detection not implemented yet.

- [ ] **Step 5: Implement detection in `kernel_boot.py`.** Add detection functions and call them in `run()` before the return statement (~line 290):

```python
def _detect_locale(host_root: Path) -> Optional[str]:
    locale_conf = host_root / "etc" / "locale.conf"
    if not locale_conf.is_file():
        return None
    for line in locale_conf.read_text().splitlines():
        line = line.strip()
        if line.startswith("LANG="):
            return line.split("=", 1)[1].strip('"').strip("'")
    return None


def _detect_timezone(host_root: Path) -> Optional[str]:
    localtime = host_root / "etc" / "localtime"
    if not localtime.is_symlink():
        return None
    # Use os.readlink to get the raw symlink target (not resolved through host_root)
    target = os.readlink(str(localtime))
    marker = "/usr/share/zoneinfo/"
    idx = target.find(marker)
    if idx < 0:
        return None
    return target[idx + len(marker):]


def _detect_alternatives(host_root: Path) -> List[AlternativeEntry]:
    alt_dir = host_root / "etc" / "alternatives"
    var_dir = host_root / "var" / "lib" / "alternatives"
    if not alt_dir.is_dir():
        return []
    entries = []
    for link in sorted(alt_dir.iterdir()):
        if not link.is_symlink():
            continue
        name = link.name
        # Use os.readlink to get the raw target path (not resolved through host_root)
        path = os.readlink(str(link))
        status = "auto"
        status_file = var_dir / name
        if status_file.is_file():
            first_line = status_file.read_text().splitlines()[0].strip()
            if first_line in ("auto", "manual"):
                status = first_line
        entries.append(AlternativeEntry(name=name, path=path, status=status))
    return entries
```

Then in `run()`, before the `return KernelBootSection(...)`:
```python
locale = _detect_locale(host_root)
timezone = _detect_timezone(host_root)
alternatives = _detect_alternatives(host_root)
```

And add to the return:
```python
return KernelBootSection(
    # ... existing fields ...
    locale=locale,
    timezone=timezone,
    alternatives=alternatives,
)
```

- [ ] **Step 6: Run tests to verify they pass.**

```bash
pytest tests/test_inspector_domains.py -v -k "locale or timezone or alternatives"
```

Expected: all pass.

- [ ] **Step 7: Run full test suite.**

```bash
pytest tests/ -v
```

Expected: all pass.

- [ ] **Step 8: Commit.**

```bash
git add src/yoinkc/inspectors/kernel_boot.py tests/test_inspector_domains.py
git commit -m "feat(kernel_boot): detect locale, timezone, and alternatives

File-based detection for container environment:
- locale from /etc/locale.conf
- timezone from /etc/localtime symlink
- alternatives from /etc/alternatives/ + /var/lib/alternatives/

Assisted-by: Claude Code"
```

### Task 7: Render System Properties in Kernel/Boot Tab

**Files:**
- Modify: `src/yoinkc/templates/report/_kernel_boot.html.j2`

- [ ] **Step 1: Read `_kernel_boot.html.j2`** to find the insertion point (after the tuned section, ~line 85).

- [ ] **Step 2: Add System Properties subsection** after the tuned section:

```html
{% if snapshot.kernel_boot.locale or snapshot.kernel_boot.timezone %}
<h3 class="pf-v6-c-title pf-m-lg">System Properties</h3>
<dl class="pf-v6-c-description-list pf-m-horizontal">
  {% if snapshot.kernel_boot.locale %}
  <div class="pf-v6-c-description-list__group">
    <dt class="pf-v6-c-description-list__term"><span class="pf-v6-c-description-list__text">Locale</span></dt>
    <dd class="pf-v6-c-description-list__description"><div class="pf-v6-c-description-list__text">{{ snapshot.kernel_boot.locale }}</div></dd>
  </div>
  {% endif %}
  {% if snapshot.kernel_boot.timezone %}
  <div class="pf-v6-c-description-list__group">
    <dt class="pf-v6-c-description-list__term"><span class="pf-v6-c-description-list__text">Timezone</span></dt>
    <dd class="pf-v6-c-description-list__description"><div class="pf-v6-c-description-list__text">{{ snapshot.kernel_boot.timezone }}</div></dd>
  </div>
  {% endif %}
</dl>
{% endif %}
```

- [ ] **Step 3: Add Alternatives table** after the system properties:

```html
{% if snapshot.kernel_boot.alternatives %}
<h3 class="pf-v6-c-title pf-m-lg">Alternatives</h3>
<table class="pf-v6-c-table pf-m-compact" role="grid">
  <thead><tr>
    <th scope="col">Name</th>
    <th scope="col">Path</th>
    <th scope="col">Status</th>
  </tr></thead>
  <tbody>
    {% for alt in snapshot.kernel_boot.alternatives %}
    <tr>
      <td>{{ alt.name }}</td>
      <td>{{ alt.path }}</td>
      <td><span class="pf-v6-c-label{{ ' pf-m-blue' if alt.status == 'manual' else '' }}">{{ alt.status }}</span></td>
    </tr>
    {% endfor %}
  </tbody>
</table>
{% endif %}
```

Note: "manual" alternatives get a blue label to visually distinguish them — these are the ones an admin explicitly chose.

- [ ] **Step 4: Run tests.**

```bash
pytest tests/ -v
```

- [ ] **Step 5: Commit.**

```bash
git add src/yoinkc/templates/report/_kernel_boot.html.j2
git commit -m "feat(report): render system properties and alternatives in Kernel/Boot tab

Locale/timezone as description list, alternatives as compact table.
Manual alternatives highlighted with blue label.

Assisted-by: Claude Code"
```

### Task 8: Fleet Merge for System Properties

**Files:**
- Modify: `src/yoinkc/fleet/merge.py`

- [ ] **Step 1: Read `merge.py`** to understand how `KernelBootSection` is currently handled. It is likely passed through from the first snapshot only (no merge logic). Check if `ConfigFileEntry.category` needs any fleet merge changes (it shouldn't — `category` is derived from `path`, which is the identity key).

- [ ] **Step 2: Confirm `ConfigFileEntry.category` needs no merge changes.** The existing merge uses `identity_fn=lambda f: f.path` — since `category` is deterministic from `path`, identical paths will always have identical categories. The `category` field serializes to JSON and passes through the content-hash variant logic unchanged. No code change needed.

- [ ] **Step 3: Add locale/timezone handling.** Since `KernelBootSection` currently uses first-snapshot-wins (no explicit merge), `locale` and `timezone` will automatically use first-wins. Confirm this by checking that the merge function does not deep-merge `KernelBootSection` fields. If it does, no change is needed. If `KernelBootSection` is not merged at all (just taken from first snapshot), no change is needed either.

- [ ] **Step 4: Add alternatives union merge** if `KernelBootSection` fields are merged. If alternatives from multiple hosts should all appear in the fleet result, add union logic:

```python
# Union merge: collect unique (name, path, status) tuples
seen = set()
merged_alternatives = []
for snap in snapshots:
    for alt in snap.kernel_boot.alternatives:
        key = (alt.name, alt.path, alt.status)
        if key not in seen:
            seen.add(key)
            merged_alternatives.append(alt)
```

If `KernelBootSection` is first-snapshot-only, this is not needed (alternatives from host 1 pass through). Add a code comment noting the limitation: `# TODO: union merge alternatives when KernelBootSection gets full fleet support`.

- [ ] **Step 5: Run fleet merge tests.**

```bash
pytest tests/test_fleet_merge.py -v
```

- [ ] **Step 6: Commit** (only if code changes were made).

```bash
git add src/yoinkc/fleet/merge.py
git commit -m "feat(fleet): handle system properties in fleet merge

locale/timezone use first-wins. alternatives noted for future union
merge when kernel_boot gets full fleet support.

Assisted-by: Claude Code"
```

## Chunk 3: Verification

### Task 9: Full Verification

- [ ] **Step 1: Run the full test suite.**

```bash
pytest tests/ -v
```

Expected: all tests pass.

- [ ] **Step 2: Verify schema version.**

```bash
grep "SCHEMA_VERSION" src/yoinkc/schema.py
```

Expected: `SCHEMA_VERSION = 8`

- [ ] **Step 3: Verify no import errors** by running the inspector on a test fixture:

```bash
python -c "from yoinkc.inspectors.config import classify_config_path; from yoinkc.schema import ConfigCategory; print(classify_config_path('/etc/tmpfiles.d/foo.conf'))"
```

Expected: `ConfigCategory.TMPFILES`

- [ ] **Step 4: Visual spot-check.** Generate a report from a test snapshot and verify:
  - Config files table has a "Category" column
  - Known paths show correct categories (tmpfiles, audit, etc.)
  - Unknown paths show "Other"
  - Kernel/Boot tab shows locale and timezone if present
  - Alternatives table renders if alternatives exist

- [ ] **Step 5: Implementation complete.** No further commits needed.
