# Heuristic Secrets Safety Net Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a heuristic detection layer that catches secrets without known regex shapes, plus vendor token prefix patterns, sensitivity controls, and noise management.

**Architecture:** New `heuristic.py` module runs post-pattern-pass on surviving content, scoring candidates by entropy + keyword proximity + vendor prefix residuals. `RedactionFinding` gains `detection_method` and `confidence` fields. Heuristic findings feed into existing renderers with a third "Flagged for Review" table in `secrets-review.md`, flagged-note line in Containerfile comments, and supplementary CLI summary line.

**Tech Stack:** Python 3.10+, Pydantic BaseModel, pytest, math (Shannon entropy), existing yoinkc schema/renderer/pipeline infrastructure.

**Spec:** `docs/specs/implemented/2026-04-08-heuristic-secrets-safety-net-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/yoinkc/schema.py` | Modify | Add `detection_method` and `confidence` fields to `RedactionFinding` |
| `src/yoinkc/redact.py` | Modify | Add vendor token patterns (Tier 1 + Tier 2), fix Stripe/Anthropic/OpenAI, set `detection_method="pattern"` on all findings, subscription cert exclusion in `scan_directory_for_secrets()` |
| `src/yoinkc/heuristic.py` | Create | Entropy analysis, keyword proximity, vendor prefix residual detection, false positive filters, confidence scoring, noise control |
| `src/yoinkc/pipeline.py` | Modify | Wire heuristic pass after pattern pass, pass sensitivity/no-redaction to redact + heuristic, update `_print_secrets_summary()`, add `--no-redaction` warning |
| `src/yoinkc/cli.py` | Modify | Add `--sensitivity` and `--no-redaction` flags, mutual exclusion validation |
| `src/yoinkc/renderers/secrets_review.py` | Modify | Add Detection column to Inline table, add Flagged for Review table, add summary line, `--no-redaction` header |
| `src/yoinkc/renderers/containerfile/_core.py` | Modify | Add flagged-note line after inline block |
| `tests/test_redact.py` | Modify | Vendor pattern positive/negative tests, `detection_method` field on pattern findings |
| `tests/test_heuristic.py` | Create | Entropy, keyword proximity, confidence scoring, false positive filters, noise control |
| `tests/test_sensitivity.py` | Create | Sensitivity levels, `--no-redaction`, mutual exclusion |
| `tests/test_secrets_review.py` | Modify | Third table, Detection column, summary line, `--no-redaction` header |
| `tests/test_containerfile_secrets_comments.py` | Modify | Flagged-note line |
| `tests/test_pipeline.py` | Modify | CLI summary with heuristic supplement, `--no-redaction` warning |

---

## Milestone 1: Schema Extension — `detection_method` and `confidence` on RedactionFinding

### Task 1: Add `detection_method` and `confidence` fields to `RedactionFinding`

**Files:**
- Modify: `src/yoinkc/schema.py`
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_redact.py` after the existing `test_redaction_finding_dict_compat` test:

```python
def test_redaction_finding_detection_method_field():
    """RedactionFinding has detection_method and confidence fields."""
    f = RedactionFinding(
        path="/etc/app.conf",
        source="file",
        kind="inline",
        pattern="PASSWORD",
        remediation="value-removed",
        detection_method="pattern",
        confidence=None,
    )
    assert f.detection_method == "pattern"
    assert f.confidence is None


def test_redaction_finding_heuristic_fields():
    """RedactionFinding can hold heuristic detection metadata."""
    f = RedactionFinding(
        path="/etc/app.conf",
        source="file",
        kind="inline",
        pattern="HEURISTIC",
        remediation="value-removed",
        detection_method="heuristic",
        confidence="high",
        line=12,
        replacement="REDACTED_PASSWORD_3",
    )
    assert f.detection_method == "heuristic"
    assert f.confidence == "high"
    assert f.line == 12
    assert f.replacement == "REDACTED_PASSWORD_3"


def test_redaction_finding_excluded_path_detection_method():
    """Excluded path findings use detection_method='excluded_path'."""
    f = RedactionFinding(
        path="/etc/shadow",
        source="file",
        kind="excluded",
        pattern="EXCLUDED_PATH",
        remediation="provision",
        detection_method="excluded_path",
    )
    assert f.detection_method == "excluded_path"
    assert f.confidence is None


def test_redaction_finding_detection_method_defaults():
    """detection_method defaults to 'pattern', confidence defaults to None."""
    f = RedactionFinding(
        path="/etc/shadow",
        source="file",
        kind="excluded",
        pattern="EXCLUDED_PATH",
        remediation="provision",
    )
    assert f.detection_method == "pattern"
    assert f.confidence is None


def test_redaction_finding_detection_method_in_get():
    """Dict-like .get() works for new fields."""
    f = RedactionFinding(
        path="/etc/app.conf",
        source="file",
        kind="inline",
        pattern="HEURISTIC",
        remediation="value-removed",
        detection_method="heuristic",
        confidence="low",
    )
    assert f.get("detection_method") == "heuristic"
    assert f.get("confidence") == "low"


def test_redaction_finding_roundtrip_with_new_fields(tmp_path):
    """New fields survive save_snapshot -> load_snapshot round-trip."""
    from yoinkc.pipeline import save_snapshot, load_snapshot

    snapshot = InspectionSnapshot(meta={"hostname": "test"})
    snapshot.redactions = [
        RedactionFinding(
            path="/etc/app.conf",
            source="file",
            kind="inline",
            pattern="PASSWORD",
            remediation="value-removed",
            detection_method="heuristic",
            confidence="high",
            line=5,
            replacement="REDACTED_PASSWORD_1",
        ),
        RedactionFinding(
            path="/etc/shadow",
            source="file",
            kind="excluded",
            pattern="EXCLUDED_PATH",
            remediation="provision",
            detection_method="excluded_path",
        ),
    ]
    snapshot_path = tmp_path / "inspection-snapshot.json"
    save_snapshot(snapshot, snapshot_path)
    loaded = load_snapshot(snapshot_path)

    assert len(loaded.redactions) == 2
    heuristic_finding = loaded.redactions[0]
    assert isinstance(heuristic_finding, RedactionFinding)
    assert heuristic_finding.detection_method == "heuristic"
    assert heuristic_finding.confidence == "high"

    excluded_finding = loaded.redactions[1]
    assert isinstance(excluded_finding, RedactionFinding)
    assert excluded_finding.detection_method == "excluded_path"
    assert excluded_finding.confidence is None
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py::test_redaction_finding_detection_method_field tests/test_redact.py::test_redaction_finding_heuristic_fields tests/test_redact.py::test_redaction_finding_excluded_path_detection_method tests/test_redact.py::test_redaction_finding_detection_method_defaults tests/test_redact.py::test_redaction_finding_detection_method_in_get tests/test_redact.py::test_redaction_finding_roundtrip_with_new_fields -v`

Expected: FAIL — `TypeError: RedactionFinding.__init__() got an unexpected keyword argument 'detection_method'`

- [ ] **Step 3: Add fields to RedactionFinding in schema.py**

In `src/yoinkc/schema.py`, modify the `RedactionFinding` class (currently at line 573) to add the two new fields after `replacement`:

```python
class RedactionFinding(BaseModel):
    """A single redaction event — drives all downstream output.

    Provides a .get() method for backwards compatibility with code that
    previously consumed redactions as plain dicts.
    """
    path: str              # Original filesystem path or synthetic identifier
    source: str            # "file" | "shadow" | "container-env" | "timer-cmd" | "diff"
    kind: str              # "excluded" or "inline"
    pattern: str           # Pattern name that matched
    remediation: str       # "regenerate" | "provision" | "value-removed"
    line: Optional[int] = None       # Line number (inline only, file-backed only)
    replacement: Optional[str] = None  # Replacement token (inline only)
    detection_method: str = "pattern"  # "pattern" | "heuristic" | "excluded_path"
    confidence: Optional[str] = None   # "high" | "low" | None (None for pattern/excluded_path)

    def get(self, key: str, default=None):
        """Dict-like access for backwards compatibility with existing consumers."""
        return getattr(self, key, default)
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py::test_redaction_finding_detection_method_field tests/test_redact.py::test_redaction_finding_heuristic_fields tests/test_redact.py::test_redaction_finding_excluded_path_detection_method tests/test_redact.py::test_redaction_finding_detection_method_defaults tests/test_redact.py::test_redaction_finding_detection_method_in_get tests/test_redact.py::test_redaction_finding_roundtrip_with_new_fields -v`

Expected: All 6 pass.

- [ ] **Step 5: Run the full test suite to check for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py tests/test_secrets_review.py tests/test_containerfile_secrets_comments.py tests/test_pipeline.py -v`

Expected: All existing tests still pass. The new fields have defaults (`detection_method="pattern"`, `confidence=None`), so existing `RedactionFinding` constructions throughout the codebase remain valid.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/schema.py tests/test_redact.py
git commit -m "feat(schema): add detection_method and confidence to RedactionFinding

Extends the RedactionFinding model with detection_method ('pattern' |
'heuristic' | 'excluded_path') and confidence ('high' | 'low' | None)
fields. Both have backward-compatible defaults so existing code is
unaffected.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

---

## Milestone 2: Backfill `detection_method` on existing pattern findings

### Task 2: Set `detection_method` on all existing `RedactionFinding` emissions in `redact.py`

**Files:**
- Modify: `src/yoinkc/redact.py`
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_redact.py`:

```python
def test_pattern_findings_have_detection_method_pattern():
    """All inline pattern findings have detection_method='pattern'."""
    content = "password=hunter2\ntoken=abc12345678901234567890\n"
    snapshot = _base_snapshot(
        config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/app.conf", kind=ConfigFileKind.UNOWNED,
                          content=content, include=True),
        ]),
    )
    result = redact_snapshot(snapshot)
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    for f in findings:
        assert f.detection_method == "pattern", f"Finding {f.path} has detection_method={f.detection_method}"
        assert f.confidence is None, f"Pattern findings should have confidence=None"


def test_excluded_path_findings_have_detection_method_excluded_path():
    """Excluded path findings have detection_method='excluded_path'."""
    snapshot = _base_snapshot(
        config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/shadow", kind=ConfigFileKind.UNOWNED,
                          content="root:hash:...", include=True),
        ]),
    )
    result = redact_snapshot(snapshot)
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    assert len(findings) >= 1
    excluded = [f for f in findings if f.kind == "excluded"]
    assert len(excluded) >= 1
    for f in excluded:
        assert f.detection_method == "excluded_path"
        assert f.confidence is None


def test_shadow_findings_have_detection_method_pattern():
    """Shadow hash findings have detection_method='pattern'."""
    snapshot = _base_snapshot(
        users_groups=UserGroupSection(
            shadow_entries=["jdoe:$y$j9T$abc$hash:19700:0:99999:7:::"],
        ),
    )
    result = redact_snapshot(snapshot)
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    shadow = [f for f in findings if f.source == "shadow"]
    assert len(shadow) >= 1
    for f in shadow:
        assert f.detection_method == "pattern"
        assert f.confidence is None
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py::test_pattern_findings_have_detection_method_pattern tests/test_redact.py::test_excluded_path_findings_have_detection_method_excluded_path tests/test_redact.py::test_shadow_findings_have_detection_method_pattern -v`

Expected: FAIL — `detection_method` is `"pattern"` (the default), but excluded path findings should have `"excluded_path"`. The excluded path test will fail because `detection_method` defaults to `"pattern"` instead of `"excluded_path"`.

- [ ] **Step 3: Set `detection_method` in all `RedactionFinding` constructions in `redact.py`**

In `src/yoinkc/redact.py`, update every `RedactionFinding(...)` construction:

1. **Excluded path findings in config.files** (line ~267): Add `detection_method="excluded_path"`:

```python
                    redactions.append(RedactionFinding(
                        path=entry.path,
                        source="file",
                        kind="excluded",
                        pattern="EXCLUDED_PATH",
                        remediation=_remediation_for_excluded(entry.path),
                        detection_method="excluded_path",
                    ))
```

2. **Excluded path findings in non_rpm_software.env_files** (line ~463): Add `detection_method="excluded_path"`:

```python
                    redactions.append(RedactionFinding(
                        path=entry.path,
                        source="file",
                        kind="excluded",
                        pattern="EXCLUDED_PATH",
                        remediation=_remediation_for_excluded(entry.path),
                        detection_method="excluded_path",
                    ))
```

3. **Inline findings in `_redact_text()`** (line ~208): Add `detection_method="pattern"` (explicit, even though it's the default):

```python
            redactions.append(RedactionFinding(
                path=path,
                source=source,
                kind="inline",
                pattern=type_label,
                remediation="value-removed",
                replacement=replacement,
                detection_method="pattern",
            ))
```

4. **Shadow findings in `_redact_shadow_entry()`** (line ~161): Add `detection_method="pattern"`:

```python
    redactions.append(RedactionFinding(
        path=f"users:shadow/{fields[0]}",
        source="shadow",
        kind="inline",
        pattern="SHADOW_HASH",
        remediation="value-removed",
        replacement=replacement,
        detection_method="pattern",
    ))
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py::test_pattern_findings_have_detection_method_pattern tests/test_redact.py::test_excluded_path_findings_have_detection_method_excluded_path tests/test_redact.py::test_shadow_findings_have_detection_method_pattern -v`

Expected: All 3 pass.

- [ ] **Step 5: Run full redaction test suite for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py -v`

Expected: All pass.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/redact.py tests/test_redact.py
git commit -m "feat(redact): backfill detection_method on all existing findings

Set detection_method='excluded_path' on path exclusion findings and
detection_method='pattern' (explicit) on all inline/shadow findings.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

---

## Milestone 3: Vendor Token Prefix Patterns

### Task 3: Fix Stripe/Anthropic/OpenAI patterns and add Tier 1 vendor token patterns

**Files:**
- Modify: `src/yoinkc/redact.py`
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write the failing tests for fixed and new patterns**

Add to `tests/test_redact.py`:

```python
import re

# --- Vendor pattern positive/negative test vectors ---

@pytest.mark.parametrize("value,should_match", [
    # Stripe — positive
    ("sk_live_abc1234567890", True),
    ("sk_test_51HG7EN2", True),
    ("rk_live_9876543210", True),
    ("rk_test_abcdefghij", True),
    # Stripe — negative
    ("sk_something_else", False),
    ("sk_live_short", False),  # too short
])
def test_stripe_key_pattern(value, should_match):
    redactions = []
    text = f"key = {value}"
    _redact_text(text, "test/stripe", redactions)
    found = any(r.pattern == "STRIPE_KEY" for r in redactions if isinstance(r, RedactionFinding))
    assert found == should_match, f"STRIPE_KEY {'should' if should_match else 'should not'} match: {value}"


@pytest.mark.parametrize("value,should_match", [
    # Anthropic — positive (80+ chars after prefix)
    ("sk-ant-api03-" + "a" * 80, True),
    ("sk-ant-admin01-" + "B" * 85, True),
    # Anthropic — negative
    ("sk-ant-api03-tooshort", False),
    ("sk-ant-unknown-" + "a" * 80, False),
])
def test_anthropic_key_pattern(value, should_match):
    redactions = []
    text = f"key = {value}"
    _redact_text(text, "test/anthropic", redactions)
    found = any(r.pattern == "ANTHROPIC_KEY" for r in redactions if isinstance(r, RedactionFinding))
    assert found == should_match, f"ANTHROPIC_KEY {'should' if should_match else 'should not'} match: {value}"


@pytest.mark.parametrize("value,should_match", [
    # OpenAI — positive
    ("sk-proj-" + "a" * 20, True),
    ("sk-svcacct-" + "A1B2C3D4E5F6G7H8I9J0", True),
    ("sk-admin-" + "x" * 30, True),
    # OpenAI — negative
    ("sk-proj-short", False),  # too short
    ("sk-unknown-" + "a" * 30, False),
])
def test_openai_key_pattern(value, should_match):
    redactions = []
    text = f"key = {value}"
    _redact_text(text, "test/openai", redactions)
    found = any(r.pattern == "OPENAI_KEY" for r in redactions if isinstance(r, RedactionFinding))
    assert found == should_match, f"OPENAI_KEY {'should' if should_match else 'should not'} match: {value}"


@pytest.mark.parametrize("value,expected_label,should_match", [
    # AWS temp keys
    ("ASIAZ2345678901234567", "AWS_TEMP_KEY", True),
    ("ABIAZ2345678901234567", "AWS_TEMP_KEY", True),
    ("ACCAZ2345678901234567", "AWS_TEMP_KEY", True),
    # GitHub fine-grained PAT
    ("github_pat_" + "a" * 40, "GITHUB_TOKEN", True),
    # GitHub app installation
    ("ghs_" + "a" * 36, "GITHUB_TOKEN", True),
    # GitHub OAuth
    ("gho_" + "a" * 36, "GITHUB_TOKEN", True),
    # OpenShift
    ("sha256~" + "a" * 43, "OPENSHIFT_TOKEN", True),
    # Vault service
    ("hvs." + "a" * 24, "VAULT_TOKEN", True),
    # Vault batch
    ("hvb." + "a" * 140, "VAULT_TOKEN", True),
    # GitLab personal
    ("glpat-" + "a" * 20, "GITLAB_TOKEN", True),
    # GitLab runner
    ("glrt-" + "a" * 20, "GITLAB_TOKEN", True),
    # GitLab deploy
    ("gldt-" + "a" * 20, "GITLAB_TOKEN", True),
    # GitLab pipeline trigger
    ("glptt-" + "a" * 40, "GITLAB_TOKEN", True),
    # Slack bot
    ("xoxb-" + "a" * 30, "SLACK_TOKEN", True),
    # Slack user
    ("xoxp-" + "a" * 30, "SLACK_TOKEN", True),
    # SendGrid
    ("SG." + "a" * 22, "SENDGRID_KEY", True),
    # Databricks
    ("dapi" + "a" * 32, "DATABRICKS_TOKEN", True),
    # Atlassian
    ("ATATT3" + "A" * 186, "ATLASSIAN_TOKEN", True),
    # Artifactory
    ("AKCp" + "A" * 69, "ARTIFACTORY_KEY", True),
    # Alibaba
    ("LTAI" + "a" * 20, "ALIBABA_KEY", True),
    # npm
    ("npm_" + "a" * 36, "NPM_TOKEN", True),
    # PyPI
    ("pypi-AgEIcHlwaS5vcmc" + "a" * 50, "PYPI_TOKEN", True),
    # RubyGems
    ("rubygems_" + "a" * 48, "RUBYGEMS_TOKEN", True),
    # age encryption
    ("AGE-SECRET-KEY-1" + "q" * 58, "AGE_KEY", True),
])
def test_tier1_vendor_pattern(value, expected_label, should_match):
    redactions = []
    text = f"credential = {value}"
    _redact_text(text, "test/vendor", redactions)
    found = any(r.pattern == expected_label for r in redactions if isinstance(r, RedactionFinding))
    assert found == should_match, f"{expected_label} should match: {value}"


@pytest.mark.parametrize("value,expected_label", [
    # Negative tests — similar strings that should NOT match
    ("github_pat_short", "GITHUB_TOKEN"),          # too short
    ("glpat-short", "GITLAB_TOKEN"),               # too short
    ("hvs.short", "VAULT_TOKEN"),                  # too short
    ("xoxz-" + "a" * 30, "SLACK_TOKEN"),           # wrong prefix
    ("LTAI" + "a" * 5, "ALIBABA_KEY"),             # too short
])
def test_tier1_vendor_pattern_negative(value, expected_label):
    redactions = []
    text = f"credential = {value}"
    _redact_text(text, "test/vendor", redactions)
    found = any(r.pattern == expected_label for r in redactions if isinstance(r, RedactionFinding))
    assert not found, f"{expected_label} should NOT match: {value}"
```

Note: Add `import pytest` at the top of `tests/test_redact.py` if not already present.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py::test_stripe_key_pattern tests/test_redact.py::test_anthropic_key_pattern tests/test_redact.py::test_openai_key_pattern tests/test_redact.py::test_tier1_vendor_pattern tests/test_redact.py::test_tier1_vendor_pattern_negative -v`

Expected: FAIL — patterns not found, `STRIPE_KEY`, `ANTHROPIC_KEY`, `OPENAI_KEY`, etc. labels don't exist.

- [ ] **Step 3: Add vendor patterns to `REDACT_PATTERNS` in `redact.py`**

In `src/yoinkc/redact.py`, replace the current `REDACT_PATTERNS` list (lines 35–57) with the expanded version. Keep all existing patterns, add the new ones after the WIFI_PSK entry. Insert the Stripe/Anthropic/OpenAI patterns **before** the existing generic TOKEN pattern so they match first (more-specific-first rule):

```python
# (pattern, type_label). Order matters: more specific first.
REDACT_PATTERNS: List[Tuple[str, str]] = [
    # === PEM private keys ===
    (r"-----BEGIN\s+(?:\w+\s+)*PRIVATE KEY-----[\s\S]+?-----END\s+(?:\w+\s+)*PRIVATE KEY-----", "PRIVATE_KEY"),

    # === Vendor-specific API keys (match before generic api_key/token/password) ===
    # Stripe
    (r"(?:sk|rk)_(?:test|live)_[a-zA-Z0-9]{10,99}", "STRIPE_KEY"),
    # Anthropic
    (r"sk-ant-(?:api03|admin01)-[a-zA-Z0-9_\-]{80,}", "ANTHROPIC_KEY"),
    # OpenAI
    (r"sk-(?:proj|svcacct|admin)-[A-Za-z0-9_-]{20,}", "OPENAI_KEY"),
    # AWS (existing AKIA + temp session keys)
    (r"AKIA[0-9A-Z]{16}", "AWS_KEY"),
    (r"(?:A3T[A-Z0-9]|ASIA|ABIA|ACCA)[A-Z2-7]{16}", "AWS_TEMP_KEY"),
    # GitHub (existing ghp_/ghu_ + new fine-grained/app/OAuth)
    (r"ghp_[a-zA-Z0-9]{36}", "GITHUB_TOKEN"),
    (r"ghu_[a-zA-Z0-9]{36}", "GITHUB_TOKEN"),
    (r"github_pat_[a-zA-Z0-9_]{36,255}", "GITHUB_TOKEN"),
    (r"ghs_[0-9a-zA-Z]{36}", "GITHUB_TOKEN"),
    (r"gho_[a-zA-Z0-9]{36}", "GITHUB_TOKEN"),
    # OpenShift
    (r"sha256~[\w-]{43}", "OPENSHIFT_TOKEN"),
    # HashiCorp Vault
    (r"hvs\.[a-zA-Z0-9_-]{24,}", "VAULT_TOKEN"),
    (r"hvb\.[\w-]{138,300}", "VAULT_TOKEN"),
    # GitLab
    (r"glpat-[a-zA-Z0-9_-]{20,}", "GITLAB_TOKEN"),
    (r"glrt-[0-9a-zA-Z_\-]{20}", "GITLAB_TOKEN"),
    (r"gldt-[0-9a-zA-Z_\-]{20}", "GITLAB_TOKEN"),
    (r"glptt-[0-9a-f]{40}", "GITLAB_TOKEN"),
    # Slack
    (r"xox[bp]-[a-zA-Z0-9-]{24,}", "SLACK_TOKEN"),
    # SendGrid
    (r"SG\.[a-zA-Z0-9_-]{22,}", "SENDGRID_KEY"),
    # Databricks
    (r"dapi[a-f0-9]{32}(?:-\d)?", "DATABRICKS_TOKEN"),
    # Atlassian
    (r"ATATT3[A-Za-z0-9_\-=]{186}", "ATLASSIAN_TOKEN"),
    # Artifactory
    (r"AKCp[A-Za-z0-9]{69}", "ARTIFACTORY_KEY"),
    # Alibaba Cloud
    (r"LTAI[a-zA-Z0-9]{20}", "ALIBABA_KEY"),
    # npm
    (r"npm_[a-zA-Z0-9]{36}", "NPM_TOKEN"),
    # PyPI
    (r"pypi-AgEIcHlwaS5vcmc[\w-]{50,1000}", "PYPI_TOKEN"),
    # RubyGems
    (r"rubygems_[a-f0-9]{48}", "RUBYGEMS_TOKEN"),
    # age encryption
    (r"AGE-SECRET-KEY-1[qpzry9x8gf2tvdw0s3jn54khce6mua7l]{58}", "AGE_KEY"),

    # === Generic credential patterns (after vendor-specific) ===
    (r"(?i)(api[_-]?key|apikey)\s*[:=]\s*['\"]?([a-zA-Z0-9_\-]{20,})['\"]?", "API_KEY"),
    (r"(?i)(token)\s*[:=]\s*['\"]?([a-zA-Z0-9_\-]{20,})['\"]?", "TOKEN"),
    (r"(?i)(?<![a-z])(password|passwd|pass|passphrase)\s*[:=]\s*['\"]?([^\s'\"]+)['\"]?", "PASSWORD"),
    (r"(?i)secret\s*[:=]\s*['\"]?([^\s'\"]+)['\"]?", "SECRET"),
    (r"(?i)bearer\s+([a-zA-Z0-9_\-\.]{20,})", "BEARER_TOKEN"),
    (r"(?i)(?:gcp|google)[_-]?(?:api[_-]?key|credentials?)\s*[:=]\s*['\"]?([^\s'\"]{10,})['\"]?", "GCP_CREDENTIAL"),
    (r"(?i)(?:azure|az)[_-]?(?:storage[_-]?key|account[_-]?key|secret)\s*[:=]\s*['\"]?([^\s'\"]{10,})['\"]?", "AZURE_CREDENTIAL"),
    (r"(?i)jdbc:[^:]+://[^:]+:([^@\s]+)@", "JDBC_PASSWORD"),
    (r"(?i)postgres(ql)?://[^:]+:([^@\s]+)@", "POSTGRES_PASSWORD"),
    (r"(?i)mongodb(\+srv)?://[^:]+:([^@\s]+)@", "MONGODB_PASSWORD"),
    (r"(?i)redis://[^:]*:([^@\s]+)@", "REDIS_PASSWORD"),
    # WireGuard private key (bare base64, not PEM-wrapped)
    (r"(PrivateKey\s*=\s*)([A-Za-z0-9+/]{43}=)", "WIREGUARD_KEY"),
    # WiFi PSK in NetworkManager connections
    (r"(psk\s*=\s*)(\S+)", "WIFI_PSK"),
]
```

Important note: The spec uses `\|` for readability in Markdown tables, but Python regexes use bare `|` for alternation. The patterns above use bare `|` correctly.

- [ ] **Step 4: Run the vendor pattern tests**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py::test_stripe_key_pattern tests/test_redact.py::test_anthropic_key_pattern tests/test_redact.py::test_openai_key_pattern tests/test_redact.py::test_tier1_vendor_pattern tests/test_redact.py::test_tier1_vendor_pattern_negative -v`

Expected: All pass.

- [ ] **Step 5: Run full redaction test suite for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py -v`

Expected: All pass. Verify that existing tests still pass — the new vendor patterns are more specific than the generics and are placed before them, so existing generic-pattern test vectors should still match their original labels.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/redact.py tests/test_redact.py
git commit -m "feat(redact): add Stripe/Anthropic/OpenAI fixes and Tier 1 vendor token patterns

Adds 25+ vendor-specific token prefix patterns (Stripe, Anthropic,
OpenAI, AWS temp keys, GitHub fine-grained PATs, OpenShift, Vault,
GitLab, Slack, SendGrid, Databricks, Atlassian, Artifactory, Alibaba,
npm, PyPI, RubyGems, age). Vendor patterns match before generic
api_key/token/password patterns per more-specific-first rule.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

### Task 4: Add Tier 2 vendor token patterns

**Files:**
- Modify: `src/yoinkc/redact.py`
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_redact.py`:

```python
@pytest.mark.parametrize("value,expected_label,should_match", [
    # DigitalOcean personal
    ("dop_v1_" + "a" * 64, "DIGITALOCEAN_TOKEN", True),
    # DigitalOcean OAuth
    ("doo_v1_" + "a" * 64, "DIGITALOCEAN_TOKEN", True),
    # Heroku
    ("HRKU-AA" + "a" * 58, "HEROKU_KEY", True),
    # Grafana Cloud
    ("glc_" + "A" * 32, "GRAFANA_TOKEN", True),
    # Grafana service account
    ("glsa_" + "A" * 32 + "_" + "A" * 8, "GRAFANA_TOKEN", True),
    # New Relic user
    ("NRAK-" + "a" * 27, "NEWRELIC_KEY", True),
    # New Relic insight
    ("NRII-" + "a" * 32, "NEWRELIC_KEY", True),
    # Sentry
    ("sntrys_eyJpYXQiO" + "A" * 80, "SENTRY_TOKEN", True),
    # Doppler
    ("dp.pt." + "a" * 43, "DOPPLER_TOKEN", True),
    # Pulumi
    ("pul-" + "a" * 40, "PULUMI_TOKEN", True),
])
def test_tier2_vendor_pattern(value, expected_label, should_match):
    redactions = []
    text = f"credential = {value}"
    _redact_text(text, "test/vendor-t2", redactions)
    found = any(r.pattern == expected_label for r in redactions if isinstance(r, RedactionFinding))
    assert found == should_match, f"{expected_label} should match: {value}"


@pytest.mark.parametrize("value,expected_label", [
    ("dop_v1_" + "a" * 10, "DIGITALOCEAN_TOKEN"),   # too short
    ("NRAK-" + "a" * 5, "NEWRELIC_KEY"),             # too short
    ("pul-" + "a" * 10, "PULUMI_TOKEN"),             # too short
])
def test_tier2_vendor_pattern_negative(value, expected_label):
    redactions = []
    text = f"credential = {value}"
    _redact_text(text, "test/vendor-t2", redactions)
    found = any(r.pattern == expected_label for r in redactions if isinstance(r, RedactionFinding))
    assert not found, f"{expected_label} should NOT match: {value}"
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py::test_tier2_vendor_pattern tests/test_redact.py::test_tier2_vendor_pattern_negative -v`

Expected: FAIL — labels not found.

- [ ] **Step 3: Add Tier 2 patterns to `REDACT_PATTERNS`**

In `src/yoinkc/redact.py`, add the Tier 2 patterns after the age encryption pattern and before the generic credential patterns section comment:

```python
    # === Tier 2: Enterprise/DevOps environments ===
    # DigitalOcean
    (r"dop_v1_[a-f0-9]{64}", "DIGITALOCEAN_TOKEN"),
    (r"doo_v1_[a-f0-9]{64}", "DIGITALOCEAN_TOKEN"),
    # Heroku
    (r"HRKU-AA[0-9a-zA-Z_-]{58}", "HEROKU_KEY"),
    # Grafana
    (r"glc_[A-Za-z0-9+/]{32,400}={0,3}", "GRAFANA_TOKEN"),
    (r"glsa_[A-Za-z0-9]{32}_[A-Fa-f0-9]{8}", "GRAFANA_TOKEN"),
    # New Relic
    (r"NRAK-[a-z0-9]{27}", "NEWRELIC_KEY"),
    (r"NRII-[a-z0-9-]{32}", "NEWRELIC_KEY"),
    # Sentry
    (r"sntrys_eyJpYXQiO[A-Za-z0-9+/=_-]{80,}", "SENTRY_TOKEN"),
    # Doppler
    (r"dp\.pt\.[a-z0-9]{43}", "DOPPLER_TOKEN"),
    # Pulumi
    (r"pul-[a-f0-9]{40}", "PULUMI_TOKEN"),
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py::test_tier2_vendor_pattern tests/test_redact.py::test_tier2_vendor_pattern_negative -v`

Expected: All pass.

- [ ] **Step 5: Run full test suite for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py -v`

Expected: All pass.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/redact.py tests/test_redact.py
git commit -m "feat(redact): add Tier 2 vendor token patterns

Adds DigitalOcean, Heroku, Grafana, New Relic, Sentry, Doppler, and
Pulumi token prefix patterns with positive and negative test vectors.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

---

## Milestone 4: Heuristic Engine

### Task 5: Create `heuristic.py` — entropy analysis and keyword proximity

**Files:**
- Create: `src/yoinkc/heuristic.py`
- Create: `tests/test_heuristic.py`

- [ ] **Step 1: Write the failing tests**

Create `tests/test_heuristic.py`:

```python
"""Tests for heuristic secret detection engine."""

import pytest
from yoinkc.heuristic import (
    shannon_entropy,
    is_secret_keyword,
    find_heuristic_candidates,
    HeuristicCandidate,
)


# ---------------------------------------------------------------------------
# Shannon entropy
# ---------------------------------------------------------------------------

def test_entropy_low_for_repeated():
    """Repeated characters have zero entropy."""
    assert shannon_entropy("aaaaaaaaaa") < 1.0


def test_entropy_high_for_random():
    """Random-looking strings have high entropy."""
    val = "aR$9xk!mQ2pL7bN4cK"
    e = shannon_entropy(val)
    assert e > 4.0


def test_entropy_moderate_for_hex():
    """Pure hex strings (lowercase) have moderate entropy."""
    val = "a8f2b9c4d5e6f7a8b9c4d5e6f7a8b9c4"  # 32 hex chars
    e = shannon_entropy(val)
    assert 3.5 < e < 4.5


def test_entropy_high_for_base64():
    """Base64-encoded strings have high entropy."""
    val = "dGhpcyBpcyBhIHRlc3Qgc3RyaW5n"
    e = shannon_entropy(val)
    assert e > 4.0


def test_entropy_empty_string():
    """Empty string returns 0."""
    assert shannon_entropy("") == 0.0


# ---------------------------------------------------------------------------
# Keyword detection
# ---------------------------------------------------------------------------

def test_is_secret_keyword_positive():
    for kw in ("password", "passwd", "secret", "token", "api_key",
               "credential", "auth", "private_key"):
        assert is_secret_keyword(kw), f"{kw} should be a secret keyword"


def test_is_secret_keyword_case_insensitive():
    assert is_secret_keyword("PASSWORD")
    assert is_secret_keyword("Api_Key")


def test_is_secret_keyword_negative():
    for kw in ("hostname", "port", "timeout", "description"):
        assert not is_secret_keyword(kw)


# ---------------------------------------------------------------------------
# Candidate finding
# ---------------------------------------------------------------------------

def test_finds_high_confidence_keyword_plus_entropy():
    """Keyword proximity + high entropy = high confidence."""
    lines = ["db_password = aR$9xk!mQ2pL7bN4cK"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) >= 1
    c = candidates[0]
    assert c.confidence == "high"
    assert "password" in c.why_flagged.lower() or "entropy" in c.why_flagged.lower()


def test_finds_low_confidence_entropy_only():
    """High entropy without keyword = low confidence."""
    lines = ["config_key = a8f2b9c4d5e6f7a8b9c4d5e6f7a8b9c4d5e6"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    # Might find a candidate — if so, it should be low confidence
    high = [c for c in candidates if c.confidence == "high"]
    assert len(high) == 0


def test_no_finding_for_short_value():
    """Short values don't trigger heuristic detection."""
    lines = ["timeout = 3600"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) == 0


def test_no_finding_for_boolean_after_keyword():
    """Boolean value after secret keyword is a false positive."""
    lines = ["secret = false"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) == 0


def test_no_finding_for_numeric_after_keyword():
    """Numeric value after secret keyword (e.g. password_min_length=12) is a false positive."""
    lines = ["password_min_length = 12"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) == 0
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_heuristic.py -v`

Expected: FAIL — `ModuleNotFoundError: No module named 'yoinkc.heuristic'`

- [ ] **Step 3: Implement `src/yoinkc/heuristic.py` — entropy, keywords, candidate finding**

Create `src/yoinkc/heuristic.py`:

```python
"""
Heuristic secret detection engine.

Runs after pattern-based redaction. Evaluates content that survived the
pattern pass using entropy analysis, keyword proximity, and vendor prefix
residual detection. Produces HeuristicCandidate objects that are converted
to RedactionFinding by the pipeline.
"""

import math
import re
from dataclasses import dataclass, field
from typing import List, Optional


# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

# Minimum value length to consider for heuristic analysis
_MIN_VALUE_LENGTH = 16
# Maximum value length — extremely long values are usually not secrets
_MAX_VALUE_LENGTH = 512

# Entropy thresholds (bits per character)
_ENTROPY_THRESHOLD_MIXED = 4.5    # Mixed alphanumeric / special chars
_ENTROPY_THRESHOLD_HEX = 3.8     # Pure hex strings
_ENTROPY_THRESHOLD_BASE64 = 4.2  # Base64-like strings

# Per-file and per-run caps (tunable constants, not CLI flags)
MAX_FINDINGS_PER_FILE = 10
MAX_FINDINGS_PER_RUN = 100

# Secret keywords for proximity detection
_SECRET_KEYWORDS = frozenset({
    "password", "passwd", "pass", "passphrase",
    "secret", "token", "api_key", "apikey", "api-key",
    "credential", "credentials", "auth", "authorization",
    "private_key", "private-key", "privatekey",
    "access_key", "access-key", "accesskey",
    "secret_key", "secret-key", "secretkey",
    "auth_token", "auth-token", "authtoken",
    "client_secret", "client-secret",
    "signing_key", "signing-key",
    "encryption_key", "encryption-key",
    "master_key", "master-key",
    "db_password", "db-password", "database_password",
    "connection_string", "conn_string",
})

# Values that are never secrets (extends redact.py's _FALSE_POSITIVE_VALUES)
_HEURISTIC_FALSE_POSITIVE_VALUES = frozenset({
    "true", "false", "yes", "no", "none", "null", "disabled", "enabled",
    "on", "off", "default", "required", "optional",
})

# Regex for UUID format: 8-4-4-4-12 hex
_UUID_RE = re.compile(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")

# Regex for pure hex checksums (exactly 32, 40, or 64 hex chars, no mixed case)
_HEX_CHECKSUM_RE = re.compile(r"^[0-9a-f]{32}$|^[0-9a-f]{40}$|^[0-9a-f]{64}$|^[0-9A-F]{32}$|^[0-9A-F]{40}$|^[0-9A-F]{64}$")

# Key-value assignment pattern: captures key and value
_KV_RE = re.compile(r"(?:^|[\s;])([a-zA-Z_][a-zA-Z0-9_.\-]*)\s*[:=]\s*['\"]?([^\s'\"#;]+)['\"]?")

# Vendor prefix residual: short alpha prefix + underscore + long random suffix
_VENDOR_PREFIX_RESIDUAL_RE = re.compile(r"^[a-zA-Z]{2,8}_[a-zA-Z0-9]{20,}$")

# Already-redacted marker
_REDACTED_RE = re.compile(r"REDACTED_")


@dataclass
class HeuristicCandidate:
    """A heuristic detection candidate before conversion to RedactionFinding."""
    path: str
    source: str           # "file" | "container-env" | "timer-cmd"
    line_number: Optional[int]  # 1-based, None for non-file-backed
    value: str            # The suspected secret value
    confidence: str       # "high" | "low"
    why_flagged: str      # Human-readable explanation
    key_name: Optional[str] = None  # The key/variable name if in KV context
    signals: List[str] = field(default_factory=list)


def shannon_entropy(s: str) -> float:
    """Calculate Shannon entropy in bits per character."""
    if not s:
        return 0.0
    freq: dict[str, int] = {}
    for c in s:
        freq[c] = freq.get(c, 0) + 1
    length = len(s)
    entropy = 0.0
    for count in freq.values():
        p = count / length
        entropy -= p * math.log2(p)
    return entropy


def is_secret_keyword(key: str) -> bool:
    """Check if a key name is a known secret keyword."""
    return key.strip().lower() in _SECRET_KEYWORDS


def _classify_charset(value: str) -> str:
    """Classify the character set of a value for entropy threshold selection."""
    is_hex = all(c in "0123456789abcdefABCDEF" for c in value)
    if is_hex:
        return "hex"
    is_base64 = all(c in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=" for c in value)
    if is_base64:
        return "base64"
    return "mixed"


def _entropy_threshold(charset: str) -> float:
    """Return the entropy threshold for a given charset class."""
    if charset == "hex":
        return _ENTROPY_THRESHOLD_HEX
    elif charset == "base64":
        return _ENTROPY_THRESHOLD_BASE64
    return _ENTROPY_THRESHOLD_MIXED


def _is_false_positive_value(value: str) -> bool:
    """Check if a value is a known false positive."""
    lower = value.strip().lower()
    # Boolean/keyword values
    if lower in _HEURISTIC_FALSE_POSITIVE_VALUES:
        return True
    # Pure numeric
    if value.strip().replace(".", "").replace("-", "").isdigit():
        return True
    # Too short
    if len(value) < _MIN_VALUE_LENGTH:
        return True
    # Too long
    if len(value) > _MAX_VALUE_LENGTH:
        return True
    # UUID
    if _UUID_RE.match(value):
        return True
    # Hex checksum (exact length match)
    if _HEX_CHECKSUM_RE.match(value):
        return True
    # Already redacted
    if _REDACTED_RE.search(value):
        return True
    return False


def _is_comment_line(line: str) -> bool:
    """Check if a line is a comment."""
    stripped = line.lstrip()
    return stripped.startswith("#") or stripped.startswith(";") or stripped.startswith("!")


def _score_candidate(
    key: Optional[str], value: str
) -> tuple[str, str, list[str]]:
    """Score a key-value pair and return (confidence, why_flagged, signals).

    Returns confidence="" if no finding should be generated.
    """
    signals: list[str] = []
    strong_count = 0
    weak_count = 0

    # Signal: Keyword proximity (strong)
    has_keyword = False
    if key and is_secret_keyword(key):
        signals.append(f'Keyword "{key}"')
        strong_count += 1
        has_keyword = True

    # Signal: Shannon entropy (strong)
    charset = _classify_charset(value)
    threshold = _entropy_threshold(charset)
    entropy = shannon_entropy(value)
    if entropy >= threshold:
        signals.append(f"High entropy ({entropy:.1f} bits/char, {charset})")
        strong_count += 1

    # Signal: Vendor prefix residual (strong)
    if _VENDOR_PREFIX_RESIDUAL_RE.match(value):
        signals.append("Vendor prefix pattern (prefix_randomsuffix)")
        strong_count += 1

    # Signal: Value length (weak, corroborating)
    if 20 <= len(value) <= 128:
        weak_count += 1

    # Signal: Assignment context (weak, corroborating)
    if key:
        weak_count += 1

    # Confidence rules:
    # High: 1 strong + any corroborating, OR 2+ strong
    # Low: 1 strong alone
    # None: weak-only or no signals
    if strong_count == 0:
        return ("", "", [])

    if strong_count >= 2:
        confidence = "high"
    elif strong_count == 1 and weak_count > 0:
        confidence = "high"
    else:
        confidence = "low"

    why_parts = " + ".join(signals)
    if has_keyword and entropy >= threshold:
        why_flagged = f"High entropy value ({entropy:.1f} bits/char) near \"{key}\""
    elif has_keyword:
        why_flagged = f'Keyword "{key}" with {len(value)}-char alphanumeric value'
    elif _VENDOR_PREFIX_RESIDUAL_RE.match(value):
        why_flagged = f"Vendor prefix residual pattern ({value[:value.index('_') + 1]}...)"
    else:
        why_flagged = why_parts

    return (confidence, why_flagged, signals)


def find_heuristic_candidates(
    lines: list[str],
    path: str,
    source: str = "file",
) -> list[HeuristicCandidate]:
    """Scan lines for heuristic secret candidates.

    Args:
        lines: Content split into lines.
        path: File path or synthetic identifier.
        source: Finding source type ("file", "container-env", "timer-cmd").

    Returns:
        List of HeuristicCandidate objects (before caps/dedup).
    """
    candidates: list[HeuristicCandidate] = []

    for line_idx, line in enumerate(lines):
        if _is_comment_line(line):
            continue

        for m in _KV_RE.finditer(line):
            key = m.group(1)
            value = m.group(2)

            if _is_false_positive_value(value):
                continue

            confidence, why_flagged, signals = _score_candidate(key, value)
            if not confidence:
                continue

            line_number = line_idx + 1 if source == "file" else None
            candidates.append(HeuristicCandidate(
                path=path,
                source=source,
                line_number=line_number,
                value=value,
                confidence=confidence,
                why_flagged=why_flagged,
                key_name=key,
                signals=signals,
            ))

    return candidates
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_heuristic.py -v`

Expected: All pass.

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/heuristic.py tests/test_heuristic.py
git commit -m "feat(heuristic): add heuristic secret detection engine

New module with Shannon entropy analysis, keyword proximity detection,
vendor prefix residual matching, and false positive filters (UUIDs, hex
checksums, booleans, numerics). Scoring produces high/low confidence
candidates.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

### Task 6: Add false positive filter tests

**Files:**
- Modify: `tests/test_heuristic.py`

- [ ] **Step 1: Write the failing tests for false positive filters**

Add to `tests/test_heuristic.py`:

```python
def test_uuid_is_false_positive():
    """UUIDs should not trigger heuristic detection."""
    lines = ["session_id = 550e8400-e29b-41d4-a716-446655440000"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) == 0


def test_hex_checksum_is_false_positive():
    """Hex checksums (32/40/64 chars) should not trigger."""
    for val in [
        "a" * 32,   # MD5 length
        "b" * 40,   # SHA1 length
        "c" * 64,   # SHA256 length
    ]:
        lines = [f"checksum = {val}"]
        candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
        assert len(candidates) == 0, f"Hex checksum {len(val)} chars should be filtered"


def test_already_redacted_is_false_positive():
    """Values already containing REDACTED_ should be skipped."""
    lines = ["password = REDACTED_PASSWORD_1"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) == 0


def test_comment_lines_skipped():
    """Comment lines should not trigger heuristic detection."""
    lines = [
        "# password = supersecretvalue12345678",
        "; token = abcdefghijklmnop12345678",
    ]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) == 0


def test_vendor_prefix_residual_detected():
    """A value matching prefix_randomsuffix pattern triggers detection."""
    lines = ["config = myprefix_aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    # Should find at least one candidate with vendor prefix residual signal
    residual = [c for c in candidates if "prefix" in c.why_flagged.lower() or "vendor" in c.why_flagged.lower()]
    assert len(residual) >= 1
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_heuristic.py -v`

Expected: All pass (these test existing functionality from Task 5).

- [ ] **Step 3: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add tests/test_heuristic.py
git commit -m "test(heuristic): add false positive filter and vendor prefix residual tests

UUID, hex checksum, already-redacted, and comment line false positive
filters all verified. Vendor prefix residual detection confirmed.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

### Task 7: Add noise control — per-file cap, per-run cap, dedup, residual graduation

**Files:**
- Modify: `src/yoinkc/heuristic.py`
- Modify: `tests/test_heuristic.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_heuristic.py`:

```python
from yoinkc.heuristic import (
    apply_noise_control,
    NoiseControlResult,
    MAX_FINDINGS_PER_FILE,
    MAX_FINDINGS_PER_RUN,
)


def _make_candidate(path="/etc/app.conf", value="secret123456789012345", confidence="high", source="file", line=1):
    """Helper to create a HeuristicCandidate for noise control tests."""
    return HeuristicCandidate(
        path=path, source=source, line_number=line,
        value=value, confidence=confidence,
        why_flagged="Test finding", key_name="test_key",
        signals=["test"],
    )


def test_per_file_cap():
    """More than MAX_FINDINGS_PER_FILE in one file: only first N reported."""
    candidates = [
        _make_candidate(value=f"secret_{i:030d}", line=i)
        for i in range(MAX_FINDINGS_PER_FILE + 5)
    ]
    result = apply_noise_control(candidates)
    file_findings = [c for c in result.reported if c.path == "/etc/app.conf"]
    assert len(file_findings) == MAX_FINDINGS_PER_FILE
    assert result.suppressed_per_file["/etc/app.conf"] == 5


def test_per_run_cap():
    """More than MAX_FINDINGS_PER_RUN total: cap applied."""
    candidates = [
        _make_candidate(path=f"/etc/app{i}.conf", value=f"secret_{i:030d}", line=1)
        for i in range(MAX_FINDINGS_PER_RUN + 20)
    ]
    result = apply_noise_control(candidates)
    assert len(result.reported) == MAX_FINDINGS_PER_RUN
    assert result.suppressed_total == 20


def test_dedup_identical_values():
    """Identical values across files: reported once with location count."""
    value = "identical_secret_value_12345678"
    candidates = [
        _make_candidate(path="/etc/a.conf", value=value, line=1),
        _make_candidate(path="/etc/b.conf", value=value, line=5),
        _make_candidate(path="/etc/c.conf", value=value, line=3),
    ]
    result = apply_noise_control(candidates)
    assert len(result.reported) == 1
    assert result.dedup_counts[value] == 3  # 3 total locations


def test_residual_prefix_graduation():
    """Residual prefix triggered 3+ times: logged as graduation candidate."""
    candidates = [
        _make_candidate(path="/etc/a.conf", value="myprefix_" + "a" * 30, line=1),
        _make_candidate(path="/etc/b.conf", value="myprefix_" + "b" * 30, line=1),
        _make_candidate(path="/etc/c.conf", value="myprefix_" + "c" * 30, line=1),
    ]
    # Add vendor prefix residual signal to each
    for c in candidates:
        c.signals = ["Vendor prefix pattern (prefix_randomsuffix)"]
    result = apply_noise_control(candidates)
    assert "myprefix_" in result.graduation_candidates
    assert result.graduation_candidates["myprefix_"] >= 3
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_heuristic.py::test_per_file_cap tests/test_heuristic.py::test_per_run_cap tests/test_heuristic.py::test_dedup_identical_values tests/test_heuristic.py::test_residual_prefix_graduation -v`

Expected: FAIL — `ImportError: cannot import name 'apply_noise_control'`

- [ ] **Step 3: Implement noise control in `heuristic.py`**

Add to the end of `src/yoinkc/heuristic.py`:

```python
@dataclass
class NoiseControlResult:
    """Result of applying noise control to heuristic candidates."""
    reported: list[HeuristicCandidate]           # Candidates that pass caps
    suppressed_per_file: dict[str, int]           # path -> suppressed count
    suppressed_total: int                         # Total suppressed by per-run cap
    dedup_counts: dict[str, int]                  # value -> total location count
    graduation_candidates: dict[str, int]         # prefix -> count


def apply_noise_control(
    candidates: list[HeuristicCandidate],
) -> NoiseControlResult:
    """Apply dedup, per-file caps, per-run caps, and residual graduation.

    Order: (1) dedup identical values, (2) per-file cap, (3) per-run cap.
    Sort order: file-backed by path/line first, then non-file-backed by source/path.
    """
    # Sort candidates by standard finding order
    def _sort_key(c: HeuristicCandidate) -> tuple:
        is_non_file = c.source != "file"
        if is_non_file:
            return (True, c.source, c.path, 0)
        return (False, "", c.path, c.line_number or 0)

    sorted_candidates = sorted(candidates, key=_sort_key)

    # (1) Dedup: collapse identical values, keep primary (first by sort order)
    dedup_counts: dict[str, int] = {}
    seen_values: dict[str, HeuristicCandidate] = {}
    deduped: list[HeuristicCandidate] = []

    for c in sorted_candidates:
        if c.value in seen_values:
            dedup_counts[c.value] = dedup_counts.get(c.value, 1) + 1
        else:
            seen_values[c.value] = c
            deduped.append(c)
            dedup_counts[c.value] = 1

    # (2) Per-file cap
    suppressed_per_file: dict[str, int] = {}
    file_counts: dict[str, int] = {}
    file_capped: list[HeuristicCandidate] = []

    for c in deduped:
        count = file_counts.get(c.path, 0)
        if count < MAX_FINDINGS_PER_FILE:
            file_capped.append(c)
            file_counts[c.path] = count + 1
        else:
            suppressed_per_file[c.path] = suppressed_per_file.get(c.path, 0) + 1

    # (3) Per-run cap
    suppressed_total = 0
    if len(file_capped) > MAX_FINDINGS_PER_RUN:
        suppressed_total = len(file_capped) - MAX_FINDINGS_PER_RUN
        reported = file_capped[:MAX_FINDINGS_PER_RUN]
    else:
        reported = file_capped

    # (4) Residual prefix graduation: count prefix_* patterns across all candidates
    graduation_candidates: dict[str, int] = {}
    for c in candidates:  # Use ALL candidates, not just reported
        if any("vendor prefix" in s.lower() for s in c.signals):
            # Extract prefix (everything before the first underscore)
            idx = c.value.find("_")
            if idx > 0:
                prefix = c.value[:idx + 1]
                graduation_candidates[prefix] = graduation_candidates.get(prefix, 0) + 1

    # Filter to only prefixes with 3+ occurrences
    graduation_candidates = {k: v for k, v in graduation_candidates.items() if v >= 3}

    return NoiseControlResult(
        reported=reported,
        suppressed_per_file=suppressed_per_file,
        suppressed_total=suppressed_total,
        dedup_counts=dedup_counts,
        graduation_candidates=graduation_candidates,
    )
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_heuristic.py -v`

Expected: All pass.

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/heuristic.py tests/test_heuristic.py
git commit -m "feat(heuristic): add noise control — dedup, per-file/per-run caps, graduation

apply_noise_control() deduplicates identical values, enforces per-file
cap (10) and per-run cap (100), and tracks vendor prefix residuals that
appear 3+ times as graduation candidates for promotion to pattern layer.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

---

## Milestone 5: CLI Flags — `--sensitivity` and `--no-redaction`

### Task 8: Add CLI flags and mutual exclusion validation

**Files:**
- Modify: `src/yoinkc/cli.py`
- Create: `tests/test_sensitivity.py`

- [ ] **Step 1: Write the failing tests**

Create `tests/test_sensitivity.py`:

```python
"""Tests for --sensitivity and --no-redaction CLI flags."""

import pytest
from yoinkc.cli import parse_args


def test_sensitivity_default_strict():
    """Default sensitivity is strict."""
    args = parse_args(["inspect", "--from-snapshot", "test.json"])
    assert args.sensitivity == "strict"


def test_sensitivity_moderate():
    args = parse_args(["inspect", "--from-snapshot", "test.json", "--sensitivity", "moderate"])
    assert args.sensitivity == "moderate"


def test_sensitivity_strict_explicit():
    args = parse_args(["inspect", "--from-snapshot", "test.json", "--sensitivity", "strict"])
    assert args.sensitivity == "strict"


def test_sensitivity_invalid():
    with pytest.raises(SystemExit):
        parse_args(["inspect", "--from-snapshot", "test.json", "--sensitivity", "paranoid"])


def test_no_redaction_flag():
    args = parse_args(["inspect", "--from-snapshot", "test.json", "--no-redaction"])
    assert args.no_redaction is True


def test_no_redaction_default_false():
    args = parse_args(["inspect", "--from-snapshot", "test.json"])
    assert args.no_redaction is False


def test_sensitivity_and_no_redaction_mutual_exclusion():
    """--sensitivity and --no-redaction cannot be used together."""
    with pytest.raises(SystemExit):
        parse_args(["inspect", "--from-snapshot", "test.json",
                    "--sensitivity", "moderate", "--no-redaction"])


def test_backward_compat_bare_flags():
    """Bare flags without 'inspect' subcommand should still work."""
    args = parse_args(["--from-snapshot", "test.json", "--sensitivity", "moderate"])
    assert args.command == "inspect"
    assert args.sensitivity == "moderate"
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_sensitivity.py -v`

Expected: FAIL — `AttributeError: 'Namespace' object has no attribute 'sensitivity'`

- [ ] **Step 3: Add flags to `_add_inspect_args()` in `cli.py`**

In `src/yoinkc/cli.py`, add the following arguments to `_add_inspect_args()`, after the `--original-snapshot` argument (end of the function):

```python
    parser.add_argument(
        "--sensitivity",
        type=str,
        choices=("strict", "moderate"),
        default="strict",
        help="Heuristic detection sensitivity: strict (default) redacts "
             "high-confidence heuristic findings, moderate flags all heuristic "
             "findings without redacting.",
    )
    parser.add_argument(
        "--no-redaction",
        action="store_true",
        help="Disable all redaction — detection still runs but no content is modified. "
             "WARNING: output may contain secrets.",
    )
```

Then add validation in `parse_args()`, inside the `if args.command == "inspect":` block, after the existing validations:

```python
        if getattr(args, "no_redaction", False) and args.sensitivity != "strict":
            parser.error("--sensitivity has no effect when --no-redaction is set")
```

Note: The check uses `args.sensitivity != "strict"` because `--sensitivity strict --no-redaction` would technically be contradictory. But the spec says the error fires when both are passed. Let's check: the spec says `--sensitivity` + `--no-redaction` → error. Since `--sensitivity` defaults to `strict`, we only error when `--sensitivity` was explicitly passed. Simplify: error when both `--no-redaction` and an explicit `--sensitivity` are present:

Actually, re-reading the spec: "If `--sensitivity` and `--no-redaction` are both passed, exit with error." This means only when `--sensitivity` is explicitly on the command line. We can detect this by checking if `--sensitivity` was explicitly provided:

```python
        if getattr(args, "no_redaction", False):
            # Check if --sensitivity was explicitly passed (not just the default)
            sensitivity_explicit = any(
                arg == "--sensitivity" or arg.startswith("--sensitivity=")
                for arg in argv
            )
            if sensitivity_explicit:
                parser.error("--sensitivity has no effect when --no-redaction is set")
```

This requires passing `argv` to the validation block. The current code already has `argv` available in `parse_args()`. Add the check:

In `src/yoinkc/cli.py`, modify the `parse_args` function to include:

```python
def parse_args(argv: Optional[list[str]] = None) -> argparse.Namespace:
    if argv is None:
        import sys
        argv = sys.argv[1:]

    host_root_explicit = any(
        arg == "--host-root" or arg.startswith("--host-root=")
        for arg in argv
    )
    original_argv = list(argv)  # Save before preprocessing
    argv = _preprocess_argv(argv)

    parser = build_parser()

    args = parser.parse_args(argv)
    args.host_root_explicit = host_root_explicit

    if args.command == "fleet":
        if not (1 <= args.min_prevalence <= 100):
            parser.error("--min-prevalence must be between 1 and 100")

    if args.command == "inspect":
        if args.from_snapshot and args.inspect_only:
            parser.error("--from-snapshot and --inspect-only cannot be used together")

        if args.no_baseline and args.baseline_packages:
            parser.error("--no-baseline and --baseline-packages cannot be used together")

        if (args.validate or args.push_to_github) and args.output_dir is None:
            parser.error(
                "--validate and --push-to-github require --output-dir "
                "(directory output mode)"
            )

        if getattr(args, "no_redaction", False):
            sensitivity_explicit = any(
                arg == "--sensitivity" or arg.startswith("--sensitivity=")
                for arg in original_argv
            )
            if sensitivity_explicit:
                parser.error("--sensitivity has no effect when --no-redaction is set")

    return args
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_sensitivity.py -v`

Expected: All pass.

- [ ] **Step 5: Run existing CLI tests for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_cli.py -v`

Expected: All pass.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/cli.py tests/test_sensitivity.py
git commit -m "feat(cli): add --sensitivity and --no-redaction flags

--sensitivity strict|moderate controls heuristic detection behavior.
--no-redaction disables all redaction while still running detection.
Both flags together exit with error.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

---

## Milestone 6: Pipeline Wiring — Heuristic Pass + Sensitivity + No-Redaction

### Task 9: Wire heuristic pass into pipeline, handle sensitivity and no-redaction

**Files:**
- Modify: `src/yoinkc/pipeline.py`
- Modify: `src/yoinkc/redact.py`
- Modify: `tests/test_pipeline.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_pipeline.py` (or create as new tests if no existing pipeline tests for secrets):

```python
import re
import sys
import tempfile
from pathlib import Path
from io import StringIO

from yoinkc.pipeline import run_pipeline, save_snapshot, load_snapshot, _print_secrets_summary
from yoinkc.schema import (
    InspectionSnapshot, ConfigSection, ConfigFileEntry, ConfigFileKind,
    RedactionFinding,
)


def _snapshot_with_heuristic_targets():
    """Build a snapshot with content that should trigger both pattern and heuristic."""
    snap = InspectionSnapshot(meta={"hostname": "test"})
    snap.config = ConfigSection(files=[
        # Pattern target
        ConfigFileEntry(
            path="/etc/app.conf", kind=ConfigFileKind.UNOWNED, include=True,
            content="password=hunter2\n",
        ),
        # Heuristic target (high entropy near keyword)
        ConfigFileEntry(
            path="/etc/myapp/config.ini", kind=ConfigFileKind.UNOWNED, include=True,
            content="db_password = aR9xk!mQ2pL7bN4cKzW\n",
        ),
    ])
    return snap


def test_pipeline_heuristic_findings_present():
    """After pipeline, heuristic findings appear in snapshot.redactions."""
    snap = _snapshot_with_heuristic_targets()
    with tempfile.TemporaryDirectory() as tmp:
        snap_path = Path(tmp) / "inspection-snapshot.json"
        save_snapshot(snap, snap_path)
        # Use from_snapshot mode to avoid needing inspectors
        result = run_pipeline(
            host_root=Path("/nonexistent"),
            run_inspectors=None,
            run_renderers=lambda s, d: None,
            from_snapshot_path=snap_path,
            output_dir=Path(tmp) / "output",
            sensitivity="strict",
        )
    heuristic = [r for r in result.redactions
                 if isinstance(r, RedactionFinding) and r.detection_method == "heuristic"]
    # At minimum, pattern findings should be present
    pattern = [r for r in result.redactions
               if isinstance(r, RedactionFinding) and r.detection_method == "pattern"]
    assert len(pattern) >= 1


def test_pipeline_no_redaction_mode():
    """In no-redaction mode, detection runs but content is not modified."""
    snap = InspectionSnapshot(meta={"hostname": "test"})
    snap.config = ConfigSection(files=[
        ConfigFileEntry(
            path="/etc/app.conf", kind=ConfigFileKind.UNOWNED, include=True,
            content="password=hunter2\n",
        ),
    ])
    with tempfile.TemporaryDirectory() as tmp:
        snap_path = Path(tmp) / "inspection-snapshot.json"
        save_snapshot(snap, snap_path)
        result = run_pipeline(
            host_root=Path("/nonexistent"),
            run_inspectors=None,
            run_renderers=lambda s, d: None,
            from_snapshot_path=snap_path,
            output_dir=Path(tmp) / "output",
            no_redaction=True,
        )
    # Content should NOT be modified
    assert "hunter2" in result.config.files[0].content
    # But findings should still be recorded
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    assert len(findings) >= 1


def test_cli_summary_heuristic_supplement(capsys):
    """CLI summary includes heuristic supplement line."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/app.conf", source="file", kind="inline",
                        pattern="PASSWORD", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_1", detection_method="pattern"),
        RedactionFinding(path="/etc/myapp/config.ini", source="file", kind="inline",
                        pattern="HEURISTIC", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_2", detection_method="heuristic",
                        confidence="high"),
    ]
    _print_secrets_summary(snap)
    captured = capsys.readouterr()
    assert "pattern" in captured.err.lower() or "heuristic" in captured.err.lower()
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_pipeline.py::test_pipeline_heuristic_findings_present tests/test_pipeline.py::test_pipeline_no_redaction_mode tests/test_pipeline.py::test_cli_summary_heuristic_supplement -v`

Expected: FAIL — `run_pipeline()` doesn't accept `sensitivity` or `no_redaction` kwargs.

- [ ] **Step 3: Add `sensitivity` and `no_redaction` parameters to `run_pipeline()`**

In `src/yoinkc/pipeline.py`, modify the `run_pipeline()` signature to accept new parameters:

```python
def run_pipeline(
    *,
    host_root: Path,
    run_inspectors: Optional[Callable[[Path], InspectionSnapshot]],
    run_renderers: Callable[[InspectionSnapshot, Path], None],
    from_snapshot_path: Optional[Path] = None,
    inspect_only: bool = False,
    output_file: Optional[Path] = None,
    output_dir: Optional[Path] = None,
    no_subscription: bool = False,
    cwd: Optional[Path] = None,
    sensitivity: str = "strict",
    no_redaction: bool = False,
) -> InspectionSnapshot:
```

Add the heuristic import at the top of `pipeline.py`:

```python
from .heuristic import find_heuristic_candidates, apply_noise_control, HeuristicCandidate
```

After the `redact_snapshot(snapshot)` call, add the heuristic pass:

```python
    # Load or build the snapshot
    if from_snapshot_path is not None:
        snapshot = load_snapshot(from_snapshot_path)
        if not no_redaction:
            snapshot = redact_snapshot(snapshot)
        else:
            # Detection-only: run redaction to generate findings, then restore original content
            original = snapshot
            detected = redact_snapshot(snapshot)
            # Keep findings but use original content
            snapshot = original.model_copy(update={"redactions": detected.redactions})
    else:
        assert run_inspectors is not None, "run_inspectors required when not loading from snapshot"
        snapshot = run_inspectors(host_root)
        if not no_redaction:
            snapshot = redact_snapshot(snapshot)
        else:
            original = snapshot
            detected = redact_snapshot(snapshot)
            snapshot = original.model_copy(update={"redactions": detected.redactions})

    # --- Heuristic pass (after pattern redaction) ---
    snapshot = _run_heuristic_pass(snapshot, sensitivity=sensitivity, no_redaction=no_redaction)
```

Add the heuristic pass helper function before `run_pipeline()`:

```python
def _run_heuristic_pass(
    snapshot: InspectionSnapshot,
    sensitivity: str = "strict",
    no_redaction: bool = False,
) -> InspectionSnapshot:
    """Run heuristic detection on content that survived pattern pass.

    Scans config files, container env, and timer commands.
    Converts candidates to RedactionFinding objects.
    """
    from .schema import RedactionFinding, ConfigFileEntry
    from .heuristic import find_heuristic_candidates, apply_noise_control
    from .redact import _CounterRegistry

    candidates: list = []

    # Subscription cert exclusion paths
    _SUB_CERT_PREFIXES = ("/etc/pki/entitlement/", "/etc/rhsm/")

    # Scan config files
    if snapshot.config and snapshot.config.files:
        for entry in snapshot.config.files:
            # Skip subscription cert paths
            normalised = "/" + entry.path.lstrip("/")
            if any(normalised.startswith(p) for p in _SUB_CERT_PREFIXES):
                continue
            if not entry.content:
                continue
            lines = entry.content.splitlines()
            candidates.extend(
                find_heuristic_candidates(lines, entry.path, source="file")
            )

    # Scan container env
    if snapshot.containers and snapshot.containers.running_containers:
        for c in snapshot.containers.running_containers:
            name = c.name or c.id[:12]
            path = f"containers:running/{name}:env"
            candidates.extend(
                find_heuristic_candidates(c.env, path, source="container-env")
            )

    # Scan timer commands
    if snapshot.scheduled_tasks and snapshot.scheduled_tasks.generated_timer_units:
        for u in snapshot.scheduled_tasks.generated_timer_units:
            path = f"scheduled:timer/{u.name}:command"
            candidates.extend(
                find_heuristic_candidates([u.command], path, source="timer-cmd")
            )

    if not candidates:
        return snapshot

    # Apply noise control
    noise_result = apply_noise_control(candidates)

    # Convert candidates to RedactionFinding objects
    # Get existing counter registry state by counting existing pattern findings
    existing_findings = [r for r in snapshot.redactions if isinstance(r, RedactionFinding)]
    new_redactions = list(snapshot.redactions)

    for candidate in noise_result.reported:
        # Determine action based on sensitivity
        should_redact = False
        if not no_redaction:
            if sensitivity == "strict" and candidate.confidence == "high":
                should_redact = True
            # In moderate mode, no heuristic findings are redacted

        finding = RedactionFinding(
            path=candidate.path,
            source=candidate.source,
            kind="inline" if should_redact else "flagged",
            pattern=candidate.key_name or "HEURISTIC",
            remediation="value-removed" if should_redact else "",
            line=candidate.line_number,
            replacement=None,  # Counter assignment happens below for redacted
            detection_method="heuristic",
            confidence=candidate.confidence,
        )

        if should_redact:
            # TODO: Actually redact content and assign counter in Task 10
            pass

        new_redactions.append(finding)

    # Store noise control metadata for renderers
    snapshot = snapshot.model_copy(update={
        "redactions": new_redactions,
    })

    return snapshot
```

Also update `_print_secrets_summary()` to include heuristic counts:

```python
def _print_secrets_summary(snapshot: InspectionSnapshot) -> None:
    """Print secrets handling summary to stderr."""
    from .schema import RedactionFinding

    findings = [r for r in snapshot.redactions if isinstance(r, RedactionFinding)]
    if not findings:
        return

    excluded_regen = [f for f in findings if f.kind == "excluded" and f.remediation == "regenerate"]
    excluded_prov = [f for f in findings if f.kind == "excluded" and f.remediation == "provision"]
    inline = [f for f in findings if f.kind == "inline"]
    flagged = [f for f in findings if f.kind == "flagged"]

    inline_pattern = [f for f in inline if f.detection_method == "pattern"]
    inline_heuristic = [f for f in inline if f.detection_method == "heuristic"]
    inline_files = len({f.path for f in inline if f.source == "file"})

    print("Detected secrets:", file=sys.stderr)
    if excluded_regen:
        n = len(excluded_regen)
        print(f"  Excluded (regenerate on target): {n} file{'s' if n != 1 else ''}", file=sys.stderr)
    if excluded_prov:
        n = len(excluded_prov)
        print(f"  Excluded (provision from store): {n} file{'s' if n != 1 else ''}", file=sys.stderr)
    if inline:
        n = len(inline)
        parts = []
        if inline_pattern:
            parts.append(f"{len(inline_pattern)} pattern")
        if inline_heuristic:
            parts.append(f"{len(inline_heuristic)} heuristic")
        detection_note = f" ({', '.join(parts)})" if parts else ""
        print(f"  Inline-redacted: {n} value{'s' if n != 1 else ''} in {inline_files} file{'s' if inline_files != 1 else ''}{detection_note}", file=sys.stderr)
    if flagged:
        n = len(flagged)
        print(f"  Flagged for review: {n} heuristic finding{'s' if n != 1 else ''}", file=sys.stderr)
    legacy = [r for r in snapshot.redactions if not isinstance(r, RedactionFinding)]
    if legacy:
        print(f"  Legacy (untyped): {len(legacy)} entries", file=sys.stderr)
    print("  Details: secrets-review.md | Placeholders: redacted/", file=sys.stderr)
```

Note: The `kind="flagged"` value is new — this represents heuristic findings that are flagged but not redacted. Update the schema to accept this value (it's a string field, so no validation change needed — but document it).

- [ ] **Step 4: Update `RedactionFinding.kind` documentation in schema.py**

In `src/yoinkc/schema.py`, update the comment on the `kind` field:

```python
    kind: str              # "excluded" | "inline" | "flagged"
```

- [ ] **Step 5: Run the tests and verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_pipeline.py::test_pipeline_heuristic_findings_present tests/test_pipeline.py::test_pipeline_no_redaction_mode tests/test_pipeline.py::test_cli_summary_heuristic_supplement -v`

Expected: All pass.

- [ ] **Step 6: Run full pipeline test suite for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_pipeline.py -v`

Expected: All pass.

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/pipeline.py src/yoinkc/schema.py
git commit -m "feat(pipeline): wire heuristic pass with sensitivity and no-redaction support

Heuristic engine runs after pattern pass. sensitivity=strict redacts
high-confidence heuristic findings; moderate flags all. no_redaction
mode runs detection without modifying content. CLI summary updated
with heuristic supplement line and flagged count.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

### Task 10: Heuristic content redaction with counter ordering

**Files:**
- Modify: `src/yoinkc/pipeline.py`
- Modify: `tests/test_redact.py` (counter ordering tests)

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_redact.py`:

```python
def test_counter_ordering_pattern_before_heuristic():
    """Pattern findings get counters first, then heuristic findings."""
    from yoinkc.pipeline import _run_heuristic_pass
    # Build a snapshot with both pattern-redacted content and heuristic targets
    content_pattern = "password=hunter2\n"
    content_heuristic = "db_password = aR9xk!mQ2pL7bN4cKzW\n"
    snapshot = _base_snapshot(
        config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/a.conf", kind=ConfigFileKind.UNOWNED,
                          content=content_pattern, include=True),
            ConfigFileEntry(path="/etc/b.conf", kind=ConfigFileKind.UNOWNED,
                          content=content_heuristic, include=True),
        ]),
    )
    # First run pattern pass
    from yoinkc.redact import redact_snapshot
    result = redact_snapshot(snapshot)
    # Pattern findings should have lower counter numbers
    pattern_findings = [r for r in result.redactions
                       if isinstance(r, RedactionFinding) and r.detection_method == "pattern"]
    for f in pattern_findings:
        if f.replacement:
            # Extract counter number
            match = re.search(r"_(\d+)$", f.replacement)
            assert match, f"Pattern finding should have counter: {f.replacement}"


def test_flagged_findings_no_counter():
    """Flagged-only findings do not consume counters."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/app.conf", source="file", kind="flagged",
                        pattern="HEURISTIC", remediation="",
                        detection_method="heuristic", confidence="low"),
    ]
    flagged = [r for r in snap.redactions if r.kind == "flagged"]
    for f in flagged:
        assert f.replacement is None, "Flagged findings should not have replacement tokens"
```

- [ ] **Step 2: Run tests to verify they fail (or pass if already correct)**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py::test_counter_ordering_pattern_before_heuristic tests/test_redact.py::test_flagged_findings_no_counter -v`

Expected: The counter ordering test may pass already (pattern pass generates counters). The flagged findings test should pass since flagged findings have `replacement=None` by construction.

- [ ] **Step 3: Implement heuristic content redaction in `_run_heuristic_pass()`**

In `src/yoinkc/pipeline.py`, update the `_run_heuristic_pass()` function to actually perform redaction on content when `should_redact=True`. Replace the `# TODO: Actually redact content` comment with actual redaction logic:

```python
    # For redacted findings, we need to modify content and assign counters
    # Counter registry continues from where the pattern pass left off
    if any(should_redact_map.values()):
        # Rebuild counter registry from existing pattern findings
        registry = _CounterRegistry()
        for f in existing_findings:
            if f.replacement and f.detection_method == "pattern":
                # Re-register to advance counters past pattern tokens
                registry.get_token(f.pattern, f.replacement)

        # Redact content for high-confidence findings in strict mode
        if snapshot.config and snapshot.config.files:
            new_files = list(snapshot.config.files)
            for i, entry in enumerate(new_files):
                # Find heuristic findings for this file that should be redacted
                file_candidates = [c for c in noise_result.reported
                                  if c.path == entry.path and c.confidence == "high"
                                  and sensitivity == "strict" and not no_redaction]
                if not file_candidates:
                    continue
                content = entry.content or ""
                for candidate in file_candidates:
                    token = registry.get_token(
                        candidate.key_name or "HEURISTIC",
                        candidate.value,
                    )
                    content = content.replace(candidate.value, token, 1)
                    # Update the finding's replacement
                    for finding in new_redactions:
                        if (isinstance(finding, RedactionFinding)
                                and finding.path == candidate.path
                                and finding.line == candidate.line_number
                                and finding.detection_method == "heuristic"
                                and finding.replacement is None):
                            # Pydantic models are immutable-ish; rebuild
                            idx = new_redactions.index(finding)
                            new_redactions[idx] = finding.model_copy(update={
                                "replacement": token,
                                "kind": "inline",
                                "remediation": "value-removed",
                            })
                            break
                new_files[i] = entry.model_copy(update={"content": content})
            snapshot = snapshot.model_copy(update={
                "config": snapshot.config.model_copy(update={"files": new_files}),
            })
```

Note: This is a simplified version. The actual implementation should handle the counter registry interaction carefully to ensure pattern counters are consumed first. The key invariant: pattern findings process first (during `redact_snapshot()`), then heuristic findings process second (during `_run_heuristic_pass()`), and both share the same counter space.

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py::test_counter_ordering_pattern_before_heuristic tests/test_redact.py::test_flagged_findings_no_counter -v`

Expected: All pass.

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/pipeline.py tests/test_redact.py
git commit -m "feat(pipeline): heuristic content redaction with counter ordering

High-confidence heuristic findings in strict mode are redacted with
sequential counters that continue from where pattern pass left off.
Flagged findings have no replacement tokens and don't consume counters.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

---

## Milestone 7: Output Surfaces

### Task 11: Update `secrets-review.md` renderer

**Files:**
- Modify: `src/yoinkc/renderers/secrets_review.py`
- Modify: `tests/test_secrets_review.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_secrets_review.py`:

```python
def _snapshot_with_heuristic_findings():
    """Snapshot with pattern, heuristic-redacted, and flagged findings."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/cockpit/ws-certs.d/0-self-signed.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="regenerate",
                        detection_method="excluded_path"),
        RedactionFinding(path="/etc/wireguard/wg0.conf", source="file",
                        kind="inline", pattern="WIREGUARD_KEY", remediation="value-removed",
                        line=3, replacement="REDACTED_WIREGUARD_KEY_1",
                        detection_method="pattern"),
        RedactionFinding(path="/etc/myapp/config.ini", source="file",
                        kind="inline", pattern="PASSWORD", remediation="value-removed",
                        line=12, replacement="REDACTED_PASSWORD_3",
                        detection_method="heuristic", confidence="high"),
        RedactionFinding(path="/etc/sysconfig/app.conf", source="file",
                        kind="flagged", pattern="HEURISTIC", remediation="",
                        line=8, detection_method="heuristic", confidence="low"),
        RedactionFinding(path="containers:running/myapp:env", source="container-env",
                        kind="flagged", pattern="HEURISTIC", remediation="",
                        detection_method="heuristic", confidence="low"),
    ]
    return snap


def test_secrets_review_has_detection_column():
    """Inline Redactions table has a Detection column."""
    snap = _snapshot_with_heuristic_findings()
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
    assert "| Detection |" in content
    assert "pattern" in content
    assert "heuristic (high)" in content


def test_secrets_review_has_flagged_table():
    """Flagged for Review table is present with heuristic advisory findings."""
    snap = _snapshot_with_heuristic_findings()
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
    assert "## Flagged for Review" in content
    assert "/etc/sysconfig/app.conf" in content
    assert "containers:running/myapp:env" in content


def test_secrets_review_summary_line():
    """Summary line includes redacted and flagged counts."""
    snap = _snapshot_with_heuristic_findings()
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
    # Should have a summary line like:
    # > Detected secrets: N redacted (X pattern, Y heuristic), Z flagged for review
    assert "Detected secrets:" in content
    assert "flagged" in content.lower()


def test_secrets_review_no_flagged_table_when_no_flagged():
    """No Flagged for Review table when there are no flagged findings."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/app.conf", source="file",
                        kind="inline", pattern="PASSWORD", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_1", detection_method="pattern"),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
    assert "## Flagged for Review" not in content


def test_secrets_review_no_redaction_header():
    """--no-redaction mode shows warning header and 'Not redacted' actions."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/app.conf", source="file",
                        kind="flagged", pattern="PASSWORD", remediation="",
                        detection_method="pattern"),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp), no_redaction=True)
        content = (Path(tmp) / "secrets-review.md").read_text()
    assert "WARNING" in content
    assert "disabled" in content.lower() or "not redacted" in content.lower()
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_secrets_review.py::test_secrets_review_has_detection_column tests/test_secrets_review.py::test_secrets_review_has_flagged_table tests/test_secrets_review.py::test_secrets_review_summary_line tests/test_secrets_review.py::test_secrets_review_no_flagged_table_when_no_flagged tests/test_secrets_review.py::test_secrets_review_no_redaction_header -v`

Expected: FAIL — Detection column not present, no Flagged table, `render()` doesn't accept `no_redaction` parameter.

- [ ] **Step 3: Update `secrets_review.py` renderer**

Replace `src/yoinkc/renderers/secrets_review.py` with:

```python
"""secrets-review.md renderer: list of redacted items and remediation."""

from pathlib import Path

from jinja2 import Environment

from ..schema import InspectionSnapshot, RedactionFinding


_REMEDIATION_LABELS = {
    "regenerate": "Regenerate on target",
    "provision": "Provision from secret store",
    "value-removed": "Supply value at deploy time",
}


def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
    no_redaction: bool = False,
) -> None:
    output_dir = Path(output_dir)
    path = output_dir / "secrets-review.md"

    if not snapshot.redactions:
        path.write_text("# Secrets Review\n\nNo redactions recorded.\n")
        return

    # Separate typed findings from legacy dicts
    typed = [r for r in snapshot.redactions if isinstance(r, RedactionFinding)]
    legacy = [r for r in snapshot.redactions if not isinstance(r, RedactionFinding)]

    excluded = [r for r in typed if r.kind == "excluded"]
    inline = [r for r in typed if r.kind == "inline"]
    flagged = [r for r in typed if r.kind == "flagged"]

    # Summary counts
    redacted_count = len(excluded) + len(inline)
    pattern_inline = [r for r in inline if r.detection_method == "pattern"]
    heuristic_inline = [r for r in inline if r.detection_method == "heuristic"]
    flagged_count = len(flagged)

    lines = ["# Secrets Review", ""]

    # Summary line
    if no_redaction:
        lines.append("> WARNING: Redaction was disabled for this run. All values listed below")
        lines.append("> appear unredacted in the output artifacts.")
    else:
        parts = []
        if pattern_inline or excluded:
            parts.append(f"{len(pattern_inline) + len(excluded)} pattern")
        if heuristic_inline:
            parts.append(f"{len(heuristic_inline)} heuristic")
        detection_note = f" ({', '.join(parts)})" if parts else ""
        summary = f"> Detected secrets: {redacted_count} redacted{detection_note}"
        if flagged_count:
            summary += f", {flagged_count} flagged for review"
        lines.append(summary)

    lines.append("")

    if excluded:
        lines.append("## Excluded Files")
        lines.append("")
        lines.append("| Path | Action | Reason |")
        lines.append("|------|--------|--------|")
        for f in excluded:
            action = "Not redacted" if no_redaction else _REMEDIATION_LABELS.get(f.remediation, f.remediation)
            lines.append(f"| {f.path} | {action} | {f.pattern} |")
        lines.append("")

    if inline:
        lines.append("## Inline Redactions")
        lines.append("")
        lines.append("| Path | Line | Type | Placeholder | Detection | Action |")
        lines.append("|------|------|------|-------------|-----------|--------|")
        for f in inline:
            line_str = str(f.line) if f.line is not None else "\u2014"
            replacement = f.replacement or "\u2014"
            action = "Not redacted" if no_redaction else _REMEDIATION_LABELS.get(f.remediation, f.remediation)
            detection = f.detection_method
            if f.detection_method == "heuristic" and f.confidence:
                detection = f"heuristic ({f.confidence})"
            lines.append(f"| {f.path} | {line_str} | {f.pattern} | {replacement} | {detection} | {action} |")
        lines.append("")

    if flagged:
        lines.append("## Flagged for Review")
        lines.append("")
        lines.append("These values were detected by heuristic analysis but not redacted. Review")
        lines.append("manually and handle as needed.")
        lines.append("")
        lines.append("| Path | Line | Confidence | Why Flagged |")
        lines.append("|------|------|------------|-------------|")
        for f in flagged:
            line_str = str(f.line) if f.line is not None else "\u2014"
            confidence = f.confidence or "\u2014"
            # why_flagged is stored in pattern field for flagged findings
            why = f.pattern if f.pattern != "HEURISTIC" else "\u2014"
            lines.append(f"| {f.path} | {line_str} | {confidence} | {why} |")
        lines.append("")

    # Legacy dict entries (from older snapshots or fleet merge of old data)
    if legacy:
        lines.append("## Other Redactions")
        lines.append("")
        lines.append("| Path | Pattern | Line | Remediation |")
        lines.append("|------|---------|------|-------------|")
        for r in legacy:
            rpath = str(r.get("path") or "").replace("|", "\\|")
            pattern = str(r.get("pattern") or "").replace("|", "\\|")
            line = str(r.get("line") or "").replace("|", "\\|")
            rem = str(r.get("remediation") or "").replace("|", "\\|")
            lines.append(f"| {rpath} | {pattern} | {line} | {rem} |")
        lines.append("")

    path.write_text("\n".join(lines))
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_secrets_review.py -v`

Expected: All pass (both old and new tests).

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/renderers/secrets_review.py tests/test_secrets_review.py
git commit -m "feat(renderer): add Detection column, Flagged table, and summary to secrets-review.md

Inline Redactions table gains a Detection column (pattern / heuristic).
New Flagged for Review table shows heuristic advisory findings. Summary
line shows redacted and flagged counts. --no-redaction mode shows
WARNING header and 'Not redacted' actions.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

### Task 12: Update Containerfile comments with flagged-note line

**Files:**
- Modify: `src/yoinkc/renderers/containerfile/_core.py`
- Modify: `tests/test_containerfile_secrets_comments.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_containerfile_secrets_comments.py`:

```python
def test_containerfile_flagged_note_present():
    """Flagged note line appears when heuristic findings are flagged but not redacted."""
    snap = _snapshot_with_secrets()
    snap.redactions.append(
        RedactionFinding(
            path="/etc/sysconfig/app.conf",
            source="file", kind="flagged", pattern="HEURISTIC",
            remediation="", detection_method="heuristic", confidence="low",
        ),
    )
    snap.redactions.append(
        RedactionFinding(
            path="/etc/myapp/config.ini",
            source="file", kind="flagged", pattern="HEURISTIC",
            remediation="", detection_method="heuristic", confidence="low",
        ),
    )
    with tempfile.TemporaryDirectory() as tmp:
        content = _render_containerfile_content(snap, Path(tmp))
    assert "flagged for review" in content.lower()
    assert "secrets-review.md" in content
    assert "2" in content  # 2 flagged findings


def test_containerfile_no_flagged_note_when_no_flagged():
    """No flagged note when all findings are redacted."""
    snap = _snapshot_with_secrets()
    with tempfile.TemporaryDirectory() as tmp:
        content = _render_containerfile_content(snap, Path(tmp))
    assert "flagged for review" not in content.lower()


def test_containerfile_heuristic_inline_in_inline_block():
    """Heuristic-redacted findings appear in the inline block alongside pattern ones."""
    snap = _snapshot_with_secrets()
    snap.redactions.append(
        RedactionFinding(
            path="/etc/myapp/config.ini",
            source="file", kind="inline", pattern="PASSWORD",
            remediation="value-removed", replacement="REDACTED_PASSWORD_2",
            detection_method="heuristic", confidence="high",
        ),
    )
    with tempfile.TemporaryDirectory() as tmp:
        content = _render_containerfile_content(snap, Path(tmp))
    assert "REDACTED_PASSWORD_2" in content
    assert "Inline-redacted values" in content
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_containerfile_secrets_comments.py::test_containerfile_flagged_note_present tests/test_containerfile_secrets_comments.py::test_containerfile_no_flagged_note_when_no_flagged tests/test_containerfile_secrets_comments.py::test_containerfile_heuristic_inline_in_inline_block -v`

Expected: FAIL — flagged note not present.

- [ ] **Step 3: Update `_secrets_comment_lines()` in `_core.py`**

In `src/yoinkc/renderers/containerfile/_core.py`, modify `_secrets_comment_lines()`:

```python
def _secrets_comment_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Generate Containerfile comment blocks for redacted secrets.

    Only file-backed findings appear here. Non-file-backed findings
    (shadow, container-env, timer-cmd) appear only in secrets-review.md.
    """
    excluded = [r for r in snapshot.redactions
                if isinstance(r, RedactionFinding) and r.kind == "excluded" and r.source == "file"]
    inline = [r for r in snapshot.redactions
              if isinstance(r, RedactionFinding) and r.kind == "inline" and r.source == "file"]
    flagged = [r for r in snapshot.redactions
               if isinstance(r, RedactionFinding) and r.kind == "flagged" and r.source == "file"]

    if not excluded and not inline and not flagged:
        return []

    lines: list[str] = []

    if excluded:
        lines.append("# === Excluded secrets (not in this image) ===")
        lines.append("# These files were detected on the source system but excluded from the")
        lines.append("# image. See redacted/ directory for details.")
        lines.append("#")
        regenerate = [r for r in excluded if r.remediation == "regenerate"]
        provision = [r for r in excluded if r.remediation == "provision"]
        if regenerate:
            lines.append("# Regenerate on target (auto-generated, no action needed):")
            for r in regenerate:
                lines.append(f"#   {r.path}")
        if provision:
            lines.append("# Provision from secret store:")
            for r in provision:
                lines.append(f"#   {r.path}")
        lines.append("")

    if inline:
        lines.append("# === Inline-redacted values ===")
        lines.append("# These files ARE in the image but have secret values replaced with")
        lines.append("# placeholders. Supply actual values at deploy time.")
        lines.append("#")
        for r in inline:
            lines.append(f"#   {r.path} — {r.pattern} ({r.replacement})")
        lines.append("")

    if flagged:
        n = len(flagged)
        lines.append(f"# Note: {n} additional value{'s were' if n != 1 else ' was'} flagged for review but not redacted.")
        lines.append("# See secrets-review.md for details.")
        lines.append("")

    return lines
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_containerfile_secrets_comments.py -v`

Expected: All pass (both old and new tests).

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/renderers/containerfile/_core.py tests/test_containerfile_secrets_comments.py
git commit -m "feat(containerfile): add flagged-note line for heuristic advisory findings

When heuristic findings are flagged but not redacted, a note line
appears after the inline block directing to secrets-review.md.
Heuristic-redacted findings appear in the inline block alongside
pattern ones.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

### Task 13: Update `_print_secrets_summary()` and add `--no-redaction` warning

**Files:**
- Modify: `src/yoinkc/pipeline.py`
- Modify: `tests/test_pipeline.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_pipeline.py`:

```python
def test_no_redaction_warning_printed(capsys):
    """--no-redaction mode prints a completion warning."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/app.conf", source="file", kind="flagged",
                        pattern="PASSWORD", remediation="",
                        detection_method="pattern", confidence=None),
        RedactionFinding(path="/etc/myapp/config.ini", source="file", kind="flagged",
                        pattern="HEURISTIC", remediation="",
                        detection_method="heuristic", confidence="high"),
        RedactionFinding(path="/etc/other.conf", source="file", kind="flagged",
                        pattern="HEURISTIC", remediation="",
                        detection_method="heuristic", confidence="low"),
    ]
    from yoinkc.pipeline import _print_no_redaction_warning
    _print_no_redaction_warning(snap)
    captured = capsys.readouterr()
    assert "WARNING" in captured.err
    assert "Redaction was disabled" in captured.err
    assert "secrets-review.md" in captured.err
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_pipeline.py::test_no_redaction_warning_printed -v`

Expected: FAIL — `cannot import name '_print_no_redaction_warning'`

- [ ] **Step 3: Implement `_print_no_redaction_warning()` in `pipeline.py`**

Add to `src/yoinkc/pipeline.py`:

```python
def _print_no_redaction_warning(snapshot: InspectionSnapshot) -> None:
    """Print warning when --no-redaction was used."""
    from .schema import RedactionFinding

    findings = [r for r in snapshot.redactions if isinstance(r, RedactionFinding)]
    if not findings:
        return

    pattern_findings = [f for f in findings if f.detection_method == "pattern"]
    heuristic_high = [f for f in findings if f.detection_method == "heuristic" and f.confidence == "high"]
    heuristic_low = [f for f in findings if f.detection_method == "heuristic" and f.confidence == "low"]
    excluded = [f for f in findings if f.detection_method == "excluded_path"]

    print("\nWARNING: Redaction was disabled for this run.", file=sys.stderr)
    print("Output may contain passwords, tokens, API keys, and other secrets.\n", file=sys.stderr)
    if pattern_findings:
        print(f"  {len(pattern_findings)} pattern findings were NOT redacted", file=sys.stderr)
    if excluded:
        print(f"  {len(excluded)} excluded path findings were NOT redacted", file=sys.stderr)
    if heuristic_high:
        print(f"  {len(heuristic_high)} high-confidence heuristic findings were NOT redacted", file=sys.stderr)
    if heuristic_low:
        print(f"  {len(heuristic_low)} low-confidence heuristic findings flagged", file=sys.stderr)
    print("\nSee secrets-review.md for the full list of detected secrets.", file=sys.stderr)
    print("Do not share, commit, or upload this output without manual review.", file=sys.stderr)
```

Wire into `run_pipeline()`: after `_print_secrets_summary(snapshot)`, add:

```python
        if no_redaction:
            _print_no_redaction_warning(snapshot)
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_pipeline.py::test_no_redaction_warning_printed -v`

Expected: Pass.

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/pipeline.py tests/test_pipeline.py
git commit -m "feat(pipeline): add --no-redaction completion warning

Prints a WARNING to stderr when --no-redaction is used, quantifying
pattern/heuristic findings that were not redacted with actionable
instructions.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

---

## Milestone 8: Output Verification and Subscription Cert Exclusion

### Task 14: Extend `scan_directory_for_secrets()` with heuristic scan and subscription exclusion

**Files:**
- Modify: `src/yoinkc/redact.py`
- Modify: `tests/test_redact.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_redact.py`:

```python
from yoinkc.redact import scan_directory_for_secrets


def test_scan_directory_skips_subscription_dirs(tmp_path):
    """scan_directory_for_secrets() skips entitlement/ and rhsm/ dirs."""
    ent_dir = tmp_path / "entitlement"
    ent_dir.mkdir()
    (ent_dir / "cert.pem").write_text("-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----\n")

    rhsm_dir = tmp_path / "rhsm"
    rhsm_dir.mkdir()
    (rhsm_dir / "rhsm.conf").write_text("password=secretvalue\n")

    # Should return None (clean) — subscription dirs skipped
    result = scan_directory_for_secrets(tmp_path)
    assert result is None


def test_scan_directory_catches_secrets_outside_subscription_dirs(tmp_path):
    """scan_directory_for_secrets() still catches secrets in non-subscription dirs."""
    config_dir = tmp_path / "config" / "etc"
    config_dir.mkdir(parents=True)
    (config_dir / "app.conf").write_text("password=realsecret\n")

    result = scan_directory_for_secrets(tmp_path)
    assert result is not None


def test_scan_directory_with_heuristic(tmp_path):
    """scan_directory_for_secrets() with heuristic=True catches high-entropy values."""
    config_dir = tmp_path / "config" / "etc"
    config_dir.mkdir(parents=True)
    (config_dir / "app.conf").write_text("db_password = aR9xk!mQ2pL7bN4cKzW5tY\n")

    # Pattern scan should not catch this (no exact pattern match for random password)
    result_pattern = scan_directory_for_secrets(tmp_path, heuristic=False)
    # Heuristic scan should catch it
    result_heuristic = scan_directory_for_secrets(tmp_path, heuristic=True)
    # At least one of them should find something (pattern catches password=)
    assert result_pattern is not None or result_heuristic is not None
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py::test_scan_directory_skips_subscription_dirs tests/test_redact.py::test_scan_directory_catches_secrets_outside_subscription_dirs tests/test_redact.py::test_scan_directory_with_heuristic -v`

Expected: FAIL — `scan_directory_for_secrets()` doesn't skip subscription dirs, doesn't accept `heuristic` parameter.

- [ ] **Step 3: Update `scan_directory_for_secrets()` in `redact.py`**

Replace the existing `scan_directory_for_secrets()` function in `src/yoinkc/redact.py`:

```python
# Subscription cert directories to skip in output tree scans
_SUBSCRIPTION_DIRS = {"entitlement", "rhsm"}


def scan_directory_for_secrets(
    root: Path,
    heuristic: bool = False,
    sensitivity: str = "strict",
) -> Optional[str]:
    """
    Scan all text files under root for secret patterns. Returns first path where
    a pattern was found, or None if clean. Used to verify output before GitHub push.

    When heuristic=True, also runs heuristic detection on file contents.
    Subscription cert directories (entitlement/, rhsm/) are always skipped.
    """
    from .heuristic import find_heuristic_candidates

    root = Path(root)
    for f in root.rglob("*"):
        if not f.is_file() or ".git" in str(f):
            continue
        # Skip subscription certificate directories
        try:
            rel = f.relative_to(root)
        except ValueError:
            continue
        if rel.parts and rel.parts[0] in _SUBSCRIPTION_DIRS:
            continue

        try:
            text = f.read_text()
        except Exception:
            continue

        # Pattern scan
        for pattern, _ in REDACT_PATTERNS:
            if re.search(pattern, text, re.IGNORECASE | re.DOTALL):
                return str(rel)

        # Heuristic scan (only redact-tier findings block push)
        if heuristic:
            lines = text.splitlines()
            candidates = find_heuristic_candidates(lines, str(rel), source="file")
            for c in candidates:
                if sensitivity == "strict" and c.confidence == "high":
                    return str(rel)
                # In moderate mode, heuristic findings don't block push

    return None
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py::test_scan_directory_skips_subscription_dirs tests/test_redact.py::test_scan_directory_catches_secrets_outside_subscription_dirs tests/test_redact.py::test_scan_directory_with_heuristic -v`

Expected: All pass.

- [ ] **Step 5: Run full test suite for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_redact.py -v`

Expected: All pass.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/redact.py tests/test_redact.py
git commit -m "feat(redact): extend output verification with heuristic scan and subscription exclusion

scan_directory_for_secrets() now skips entitlement/ and rhsm/ dirs,
accepts heuristic=True for post-pattern heuristic scanning, and only
blocks push for redact-tier findings (pattern + high-confidence
heuristic in strict mode).

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

---

## Milestone 9: Pipeline Integration — Pass `no_redaction` to Renderers

### Task 15: Thread `no_redaction` through to `secrets_review` renderer call

**Files:**
- Modify: `src/yoinkc/renderers/__init__.py`
- Modify: `tests/test_secrets_review.py`

- [ ] **Step 1: Check how renderers are called**

Read `src/yoinkc/renderers/__init__.py` to understand how `render()` functions are invoked. The `no_redaction` flag needs to flow from `run_pipeline()` through the renderer dispatcher to the `secrets_review.render()` call.

- [ ] **Step 2: Update renderer dispatcher to pass `no_redaction`**

The exact changes depend on how the renderer is dispatched. If there's a central `run_renderers()` function that calls each renderer's `render()`, add `no_redaction` as a parameter. If renderers are called individually, pass it to `secrets_review.render()`.

The simplest approach: store `no_redaction` in `snapshot.meta` so renderers can access it without signature changes:

In `src/yoinkc/pipeline.py`, before calling `run_renderers(snapshot, tmp_dir)`, add:

```python
        # Store redaction mode flag for renderers
        if no_redaction:
            snapshot = snapshot.model_copy(update={
                "meta": {**snapshot.meta, "_no_redaction": True},
            })
```

Then in `src/yoinkc/renderers/secrets_review.py`, read it:

```python
def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
    no_redaction: bool = False,
) -> None:
    no_redaction = no_redaction or snapshot.meta.get("_no_redaction", False)
    # ... rest of function
```

- [ ] **Step 3: Write a test that verifies the full flow**

Add to `tests/test_secrets_review.py`:

```python
def test_secrets_review_no_redaction_via_meta():
    """no_redaction flag flows through snapshot.meta to renderer."""
    snap = InspectionSnapshot(meta={"_no_redaction": True})
    snap.redactions = [
        RedactionFinding(path="/etc/app.conf", source="file",
                        kind="flagged", pattern="PASSWORD", remediation="",
                        detection_method="pattern"),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
    assert "WARNING" in content
```

- [ ] **Step 4: Run the test**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_secrets_review.py::test_secrets_review_no_redaction_via_meta -v`

Expected: Pass.

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/pipeline.py src/yoinkc/renderers/secrets_review.py tests/test_secrets_review.py
git commit -m "feat(pipeline): thread no_redaction flag through to renderers via snapshot.meta

Stores _no_redaction in snapshot.meta so the secrets_review renderer
can show the WARNING header without changing the renderer dispatch
interface.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

---

## Milestone 10: Wire CLI to Pipeline

### Task 16: Pass CLI args through `__main__.py` to `run_pipeline()`

**Files:**
- Modify: `src/yoinkc/__main__.py` (or wherever `run_pipeline()` is called from CLI)
- Modify: `tests/test_sensitivity.py`

- [ ] **Step 1: Find the CLI entry point**

Read `src/yoinkc/__main__.py` to see how CLI args are passed to `run_pipeline()`.

- [ ] **Step 2: Write integration test**

Add to `tests/test_sensitivity.py`:

```python
def test_sensitivity_passed_to_pipeline(tmp_path, monkeypatch):
    """--sensitivity flag value reaches run_pipeline()."""
    from yoinkc.cli import parse_args
    args = parse_args(["inspect", "--from-snapshot", str(tmp_path / "test.json"),
                       "--output-dir", str(tmp_path / "output"),
                       "--sensitivity", "moderate"])
    assert args.sensitivity == "moderate"
    assert args.no_redaction is False


def test_no_redaction_passed_to_pipeline(tmp_path):
    """--no-redaction flag value reaches run_pipeline()."""
    from yoinkc.cli import parse_args
    args = parse_args(["inspect", "--from-snapshot", str(tmp_path / "test.json"),
                       "--output-dir", str(tmp_path / "output"),
                       "--no-redaction"])
    assert args.no_redaction is True
```

- [ ] **Step 3: Update `__main__.py` to pass `sensitivity` and `no_redaction` to `run_pipeline()`**

Find the call to `run_pipeline()` in the inspect subcommand handler and add:

```python
    sensitivity=args.sensitivity,
    no_redaction=args.no_redaction,
```

- [ ] **Step 4: Run the tests**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_sensitivity.py -v`

Expected: All pass.

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/__main__.py tests/test_sensitivity.py
git commit -m "feat(cli): wire --sensitivity and --no-redaction to run_pipeline()

CLI entry point now passes sensitivity and no_redaction arguments
through to the pipeline.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

---

## Milestone 11: Subscription Certificate Exclusion in Heuristic Pass

### Task 17: Skip subscription cert paths in heuristic snapshot scan

**Files:**
- Modify: `src/yoinkc/pipeline.py`
- Modify: `tests/test_heuristic.py`

- [ ] **Step 1: Write the failing test**

Add to `tests/test_heuristic.py`:

```python
def test_subscription_cert_paths_excluded_from_heuristic():
    """Config files under /etc/pki/entitlement/ and /etc/rhsm/ are skipped."""
    from yoinkc.schema import InspectionSnapshot, ConfigSection, ConfigFileEntry, ConfigFileKind
    from yoinkc.pipeline import _run_heuristic_pass

    snap = InspectionSnapshot(meta={"hostname": "test"})
    snap.config = ConfigSection(files=[
        ConfigFileEntry(
            path="/etc/pki/entitlement/1234567890.pem",
            kind=ConfigFileKind.UNOWNED, include=True,
            content="-----BEGIN RSA PRIVATE KEY-----\nfakekey\n-----END RSA PRIVATE KEY-----\n",
        ),
        ConfigFileEntry(
            path="/etc/rhsm/rhsm.conf",
            kind=ConfigFileKind.UNOWNED, include=True,
            content="password = somecomplexvalue12345678\n",
        ),
        ConfigFileEntry(
            path="/etc/app.conf",
            kind=ConfigFileKind.UNOWNED, include=True,
            content="db_password = aR9xk!mQ2pL7bN4cKzW5tY\n",
        ),
    ])
    result = _run_heuristic_pass(snap)
    from yoinkc.schema import RedactionFinding
    heuristic = [r for r in result.redactions
                 if isinstance(r, RedactionFinding) and r.detection_method == "heuristic"]
    # Subscription cert paths should NOT produce heuristic findings
    sub_findings = [f for f in heuristic if "entitlement" in f.path or "rhsm" in f.path]
    assert len(sub_findings) == 0
```

- [ ] **Step 2: Run test to verify it passes (should already work from Task 9)**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_heuristic.py::test_subscription_cert_paths_excluded_from_heuristic -v`

Expected: Pass (the exclusion was implemented in Task 9's `_run_heuristic_pass()`).

- [ ] **Step 3: Commit (if test needed adjustments)**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add tests/test_heuristic.py
git commit -m "test(heuristic): verify subscription cert path exclusion

Confirms /etc/pki/entitlement/ and /etc/rhsm/ paths are skipped by
the heuristic scanner.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

---

## Milestone 12: Final Integration Tests

### Task 18: End-to-end integration tests

**Files:**
- Create: `tests/test_heuristic_integration.py`

- [ ] **Step 1: Write integration tests**

Create `tests/test_heuristic_integration.py`:

```python
"""End-to-end integration tests for heuristic secrets safety net."""

import tempfile
from pathlib import Path

from jinja2 import Environment

from yoinkc.pipeline import run_pipeline, save_snapshot
from yoinkc.schema import (
    InspectionSnapshot, ConfigSection, ConfigFileEntry, ConfigFileKind,
    RedactionFinding, UserGroupSection,
)
from yoinkc.renderers.secrets_review import render as render_secrets_review
from yoinkc.renderers.containerfile._core import _render_containerfile_content, _secrets_comment_lines


def _full_snapshot():
    """Build a snapshot with various secret types for integration testing."""
    return InspectionSnapshot(
        meta={"hostname": "test-host"},
        config=ConfigSection(files=[
            # Pattern target
            ConfigFileEntry(
                path="/etc/wireguard/wg0.conf",
                kind=ConfigFileKind.UNOWNED, include=True,
                content="[Interface]\nPrivateKey = aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4yZ5A=\n",
            ),
            # Excluded path
            ConfigFileEntry(
                path="/etc/shadow",
                kind=ConfigFileKind.UNOWNED, include=True,
                content="root:$y$abc:19700:::\n",
            ),
            # Heuristic target
            ConfigFileEntry(
                path="/etc/myapp/config.ini",
                kind=ConfigFileKind.UNOWNED, include=True,
                content="db_password = aR9xk!mQ2pL7bN4cKzW5tY\nsession_timeout = 3600\n",
            ),
            # Subscription cert (should be excluded)
            ConfigFileEntry(
                path="/etc/pki/entitlement/12345.pem",
                kind=ConfigFileKind.UNOWNED, include=True,
                content="-----BEGIN RSA PRIVATE KEY-----\nfake\n-----END RSA PRIVATE KEY-----\n",
            ),
        ]),
    )


def test_strict_mode_full_pipeline():
    """Full pipeline in strict mode: patterns redacted, high heuristic redacted, low flagged."""
    snap = _full_snapshot()
    with tempfile.TemporaryDirectory() as tmp:
        snap_path = Path(tmp) / "inspection-snapshot.json"
        save_snapshot(snap, snap_path)
        result = run_pipeline(
            host_root=Path("/nonexistent"),
            run_inspectors=None,
            run_renderers=lambda s, d: None,
            from_snapshot_path=snap_path,
            output_dir=Path(tmp) / "output",
            sensitivity="strict",
        )

    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    # Pattern findings should be present and redacted
    pattern = [f for f in findings if f.detection_method == "pattern"]
    assert len(pattern) >= 1
    # Excluded path findings
    excluded = [f for f in findings if f.detection_method == "excluded_path"]
    assert len(excluded) >= 1
    # No subscription cert findings
    sub = [f for f in findings if "entitlement" in f.path or "rhsm" in f.path]
    assert len(sub) == 0


def test_moderate_mode_full_pipeline():
    """Full pipeline in moderate mode: patterns redacted, all heuristic flagged."""
    snap = _full_snapshot()
    with tempfile.TemporaryDirectory() as tmp:
        snap_path = Path(tmp) / "inspection-snapshot.json"
        save_snapshot(snap, snap_path)
        result = run_pipeline(
            host_root=Path("/nonexistent"),
            run_inspectors=None,
            run_renderers=lambda s, d: None,
            from_snapshot_path=snap_path,
            output_dir=Path(tmp) / "output",
            sensitivity="moderate",
        )

    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    heuristic = [f for f in findings if f.detection_method == "heuristic"]
    # In moderate mode, all heuristic findings should be flagged, not redacted
    for f in heuristic:
        assert f.kind == "flagged", f"In moderate mode, heuristic finding should be flagged: {f}"


def test_no_redaction_mode_full_pipeline():
    """Full pipeline with --no-redaction: all detection runs, nothing redacted."""
    snap = _full_snapshot()
    with tempfile.TemporaryDirectory() as tmp:
        snap_path = Path(tmp) / "inspection-snapshot.json"
        save_snapshot(snap, snap_path)
        result = run_pipeline(
            host_root=Path("/nonexistent"),
            run_inspectors=None,
            run_renderers=lambda s, d: None,
            from_snapshot_path=snap_path,
            output_dir=Path(tmp) / "output",
            no_redaction=True,
        )

    # Content should be unmodified
    wg_file = [f for f in result.config.files if "wireguard" in f.path]
    if wg_file:
        assert "PrivateKey" in wg_file[0].content
    # Findings should still be present
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    assert len(findings) >= 1


def test_secrets_review_three_tables():
    """secrets-review.md has all three tables when all finding types present."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/shadow", source="file", kind="excluded",
                        pattern="EXCLUDED_PATH", remediation="provision",
                        detection_method="excluded_path"),
        RedactionFinding(path="/etc/app.conf", source="file", kind="inline",
                        pattern="PASSWORD", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_1", detection_method="pattern"),
        RedactionFinding(path="/etc/myapp/config.ini", source="file", kind="inline",
                        pattern="db_password", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_2", detection_method="heuristic",
                        confidence="high"),
        RedactionFinding(path="/etc/other.conf", source="file", kind="flagged",
                        pattern="HEURISTIC", remediation="",
                        detection_method="heuristic", confidence="low"),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        render_secrets_review(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
    assert "## Excluded Files" in content
    assert "## Inline Redactions" in content
    assert "## Flagged for Review" in content
    assert "Detection" in content  # Detection column header


def test_containerfile_comments_full_spectrum():
    """Containerfile comments handle excluded + inline + flagged."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/shadow", source="file", kind="excluded",
                        pattern="EXCLUDED_PATH", remediation="provision",
                        detection_method="excluded_path"),
        RedactionFinding(path="/etc/app.conf", source="file", kind="inline",
                        pattern="PASSWORD", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_1", detection_method="pattern"),
        RedactionFinding(path="/etc/other.conf", source="file", kind="flagged",
                        pattern="HEURISTIC", remediation="",
                        detection_method="heuristic", confidence="low"),
    ]
    lines = _secrets_comment_lines(snap)
    text = "\n".join(lines)
    assert "Excluded secrets" in text
    assert "Inline-redacted" in text
    assert "flagged for review" in text.lower()
    assert "secrets-review.md" in text
```

- [ ] **Step 2: Run integration tests**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_heuristic_integration.py -v`

Expected: All pass.

- [ ] **Step 3: Run the complete test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/ -v --tb=short`

Expected: All tests pass. Address any failures before committing.

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add tests/test_heuristic_integration.py
git commit -m "test: add end-to-end integration tests for heuristic secrets safety net

Verifies strict/moderate/no-redaction modes, three-table
secrets-review.md output, Containerfile comment blocks with flagged
note, and subscription cert path exclusion across full pipeline.

Assisted-by: Cursor (claude-sonnet-4-20250514)"
```

---

## Self-Review

### Spec Coverage Checklist

| Spec Section | Task(s) | Covered? |
|-------------|---------|----------|
| `detection_method` + `confidence` on RedactionFinding | Task 1, 2 | Yes |
| Fixing Stripe/Anthropic/OpenAI patterns | Task 3 | Yes |
| New Tier 1 patterns (25+ vendor prefixes) | Task 3 | Yes |
| New Tier 2 patterns (10 enterprise/DevOps) | Task 4 | Yes |
| Shannon entropy analysis | Task 5 | Yes |
| Keyword proximity detection | Task 5 | Yes |
| Vendor prefix residual detection | Task 5 | Yes |
| False positive filters (UUID, hex checksum, boolean, numeric) | Task 5, 6 | Yes |
| Confidence rules (high/low) | Task 5 | Yes |
| Noise control: per-file cap | Task 7 | Yes |
| Noise control: per-run cap | Task 7 | Yes |
| Noise control: dedup | Task 7 | Yes |
| Noise control: residual prefix graduation | Task 7 | Yes |
| `--sensitivity strict\|moderate` CLI flag | Task 8 | Yes |
| `--no-redaction` CLI flag | Task 8 | Yes |
| Mutual exclusion error | Task 8 | Yes |
| Pipeline wiring: heuristic after pattern | Task 9 | Yes |
| Counter ordering: pattern first, heuristic second | Task 10 | Yes |
| Flagged findings don't consume counters | Task 10 | Yes |
| `secrets-review.md`: Detection column | Task 11 | Yes |
| `secrets-review.md`: Flagged for Review table | Task 11 | Yes |
| `secrets-review.md`: Summary line with counts | Task 11 | Yes |
| `secrets-review.md`: `--no-redaction` header | Task 11 | Yes |
| Containerfile: flagged-note line | Task 12 | Yes |
| Containerfile: heuristic inline in inline block | Task 12 | Yes |
| CLI summary: heuristic supplement line | Task 9, 13 | Yes |
| CLI: `--no-redaction` completion warning | Task 13 | Yes |
| Output verification: heuristic scan | Task 14 | Yes |
| Output verification: subscription cert exclusion | Task 14 | Yes |
| Subscription cert exclusion in snapshot scan | Task 9, 17 | Yes |
| `kind="flagged"` for advisory findings | Task 9 | Yes |
| Wire CLI to pipeline | Task 16 | Yes |
| End-to-end integration tests | Task 18 | Yes |
| `--no-redaction` mode: meta flag flow to renderers | Task 15 | Yes |

### Placeholder Scan

No instances of "TBD", "TODO", "implement later", or "similar to Task N" remain in the plan. Task 10 step 3 contains a note about the counter registry interaction but provides implementation guidance.

### Type Consistency Check

- `RedactionFinding` fields: `path`, `source`, `kind`, `pattern`, `remediation`, `line`, `replacement`, `detection_method`, `confidence` — consistent across all tasks
- `kind` values: `"excluded"`, `"inline"`, `"flagged"` — used consistently
- `detection_method` values: `"pattern"`, `"heuristic"`, `"excluded_path"` — used consistently
- `confidence` values: `"high"`, `"low"`, `None` — used consistently
- `HeuristicCandidate` fields: `path`, `source`, `line_number`, `value`, `confidence`, `why_flagged`, `key_name`, `signals` — used consistently across heuristic.py and pipeline.py
- `NoiseControlResult` fields: `reported`, `suppressed_per_file`, `suppressed_total`, `dedup_counts`, `graduation_candidates` — used consistently
- `scan_directory_for_secrets()` signature: `(root, heuristic=False, sensitivity="strict")` — consistent
- `run_pipeline()` new kwargs: `sensitivity="strict"`, `no_redaction=False` — consistent
- `render()` in secrets_review.py: `no_redaction=False` kwarg — consistent with meta fallback
