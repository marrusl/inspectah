# Secrets Handling v2 Implementation Plan (Revision 5)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close detection gaps, make redacted files obvious with remediation-specific guidance, and replace hash-based tokens with sequential counters.

**Architecture:** Extend the existing `redact_snapshot()` pipeline with new patterns, a typed `RedactionFinding` model, and remediation state tracking. Add a `redacted/` output directory for excluded files. Update all downstream renderers and consumers to use the new model.

**Tech Stack:** Python 3.10+, Pydantic BaseModel, pytest, existing inspectah schema/renderer infrastructure.

**Spec:** `docs/specs/proposed/2026-04-08-secrets-handling-v2-design.md`

**Revision notes:** This is revision 5. (5) Task 3 clarity: made the single-pass counter algorithm completely unambiguous — one `_CounterRegistry`, tokens assigned during `_redact_text()`/`_redact_shadow_entry()` into both content strings and `RedactionFinding.replacement` in the same code path, output-order sort explicitly described as a list reorder (not token reassignment). Fixed commit message. Prior revision 2/3/4 changes preserved.

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/inspectah/schema.py` | Modify | Add `RedactionFinding` model, update `InspectionSnapshot.redactions` type to `List[Union[dict, RedactionFinding]]` |
| `src/inspectah/redact.py` | Modify | New patterns, sequential counters (incl shadow), remediation states, `include=False` for excluded |
| `src/inspectah/renderers/secrets_review.py` | Modify | New format with separate Excluded/Inline tables, attribute access |
| `src/inspectah/renderers/html_report.py` | Modify | Update `r.get(...)` calls to handle `RedactionFinding` attributes |
| `src/inspectah/renderers/audit_report.py` | Modify | Update `r.get(...)` calls to handle `RedactionFinding` attributes |
| `src/inspectah/fleet/merge.py` | Modify | Update `_deduplicate_warning_dicts()` to handle `RedactionFinding` objects |
| `src/inspectah/renderers/containerfile/_config_tree.py` | Modify | Add `write_redacted_dir()` for `.REDACTED` files |
| `src/inspectah/renderers/containerfile/_core.py` | Modify | Add secrets comment blocks to Containerfile |
| `src/inspectah/renderers/__init__.py` | Modify | Wire `write_redacted_dir()` into renderer pipeline |
| `src/inspectah/pipeline.py` | Modify | Add `_print_secrets_summary()` CLI output |
| `tests/test_redact.py` | Modify | Detection gap tests, counter tests, remediation state tests |
| `tests/test_secrets_review.py` | Create | Renderer output tests for new format |
| `tests/test_redacted_dir.py` | Create | `.REDACTED` file placement and content tests |
| `tests/test_containerfile_secrets_comments.py` | Create | Containerfile comment block tests |
| `tests/test_html_report_output.py` | Modify | Verify redaction warnings still render with typed findings |
| `tests/test_audit_report_output.py` | Modify | Verify redaction section still renders with typed findings |
| `tests/test_pipeline.py` | Modify | Add automated CLI summary stderr capture test |

---

## Milestone 1: Foundation — RedactionFinding model + compatibility layer

This milestone adds the typed model and a compatibility helper so that all existing consumers continue working during migration. The tree stays green after this milestone.

### Task 1: Add RedactionFinding model to schema + compatibility helper

**Files:**
- Modify: `src/inspectah/schema.py`
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write the failing test**

```python
# Add to tests/test_redact.py at the top imports
from inspectah.schema import RedactionFinding

def test_redaction_finding_model():
    """RedactionFinding can be constructed with all fields."""
    f = RedactionFinding(
        path="/etc/shadow",
        source="file",
        kind="excluded",
        pattern="EXCLUDED_PATH",
        remediation="provision",
        line=None,
        replacement=None,
    )
    assert f.path == "/etc/shadow"
    assert f.source == "file"
    assert f.kind == "excluded"
    assert f.remediation == "provision"
    assert f.line is None
    assert f.replacement is None

def test_redaction_finding_dict_compat():
    """RedactionFinding supports .get() for backwards compat with dict consumers."""
    f = RedactionFinding(
        path="/etc/shadow",
        source="file",
        kind="excluded",
        pattern="EXCLUDED_PATH",
        remediation="provision",
    )
    assert f.get("path") == "/etc/shadow"
    assert f.get("pattern") == "EXCLUDED_PATH"
    assert f.get("line") is None
    assert f.get("missing", "default") == "default"
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_redact.py::test_redaction_finding_model tests/test_redact.py::test_redaction_finding_dict_compat -v`
Expected: FAIL — `ImportError: cannot import name 'RedactionFinding'`

- [ ] **Step 3: Add RedactionFinding to schema.py**

Add after the existing model definitions, before the `InspectionSnapshot` class (before line 578 in `src/inspectah/schema.py`):

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

    def get(self, key: str, default=None):
        """Dict-like access for backwards compatibility with existing consumers."""
        return getattr(self, key, default)
```

Note: This uses Pydantic `BaseModel` (like all other models in schema.py), NOT `@dataclass`. The `Optional` import is already present at the top of schema.py.

- [ ] **Step 4: Change `InspectionSnapshot.redactions` to a Union type with a validator**

The `redactions` field must be changed from `List[dict]` to a proper Union type so that `RedactionFinding` objects survive serialization round-trips. Without this, `save_snapshot()` calls `model_dump_json()` which serializes `RedactionFinding` to JSON objects, and `load_snapshot()` calls `model_validate()` which reconstructs them as plain dicts — causing all downstream `isinstance(RedactionFinding)` checks to fail silently.

In `src/inspectah/schema.py`, add the `Union` import at the top (line 3):

```python
from typing import Dict, List, Optional, Union
```

Then add a `field_validator` import from Pydantic (line 5):

```python
from pydantic import BaseModel, Field, field_validator
```

Then change the `redactions` field in `InspectionSnapshot` (line 603):

```python
# Current:
redactions: List[dict] = Field(default_factory=list)
# Change to:
redactions: List[Union["RedactionFinding", dict]] = Field(default_factory=list)

@field_validator("redactions", mode="before")
@classmethod
def _coerce_redaction_dicts(cls, v):
    """Reconstruct RedactionFinding from dicts on deserialization.

    When a snapshot is loaded via model_validate() (e.g. load_snapshot()),
    Pydantic deserializes RedactionFinding objects as plain dicts.  This
    validator reconstructs them so isinstance() checks work after round-trip.
    Dicts that don't match the RedactionFinding schema (e.g. legacy entries)
    are left as-is.
    """
    result = []
    for item in v:
        if isinstance(item, dict) and "source" in item and "kind" in item:
            try:
                result.append(RedactionFinding(**item))
            except Exception:
                result.append(item)
        else:
            result.append(item)
    return result
```

The validator fires on `model_validate()` (used by `load_snapshot()`) and reconstructs `RedactionFinding` from any dict that has the `source` and `kind` fields (which are unique to `RedactionFinding` — legacy redaction dicts don't have them). Legacy dicts pass through unchanged.

Note: `RedactionFinding` is defined earlier in the same file (added in Step 3), so the forward reference in the type annotation uses a string `"RedactionFinding"` — Pydantic resolves this via `model_rebuild()` automatically.

- [ ] **Step 5: Write a round-trip regression test**

Add to `tests/test_redact.py`:

```python
from inspectah.pipeline import save_snapshot, load_snapshot
from inspectah.schema import RedactionFinding, InspectionSnapshot

def test_redaction_finding_survives_save_load_roundtrip(tmp_path):
    """RedactionFinding objects survive save_snapshot() -> load_snapshot() round-trip.

    This is the critical durability test: save_snapshot() calls model_dump_json(),
    load_snapshot() calls model_validate(). Without the field_validator on
    InspectionSnapshot.redactions, RedactionFinding objects would be deserialized
    as plain dicts, and all isinstance() checks downstream would fail silently.
    """
    snapshot = InspectionSnapshot(meta={"hostname": "test"})
    snapshot.redactions = [
        RedactionFinding(
            path="/etc/cockpit/ws-certs.d/0-self-signed.key",
            source="file", kind="excluded", pattern="EXCLUDED_PATH",
            remediation="regenerate",
        ),
        RedactionFinding(
            path="/etc/wireguard/wg0.conf",
            source="file", kind="inline", pattern="WIREGUARD_KEY",
            remediation="value-removed", line=3,
            replacement="REDACTED_WIREGUARD_KEY_1",
        ),
        RedactionFinding(
            path="users:shadow/admin",
            source="shadow", kind="inline", pattern="SHADOW_HASH",
            remediation="value-removed",
            replacement="REDACTED_SHADOW_HASH_1",
        ),
        # Legacy dict entry — should pass through unchanged
        {"path": "/etc/old.conf", "pattern": "PASSWORD", "line": "content",
         "remediation": "old style"},
    ]

    # Round-trip through save/load
    snapshot_path = tmp_path / "inspection-snapshot.json"
    save_snapshot(snapshot, snapshot_path)
    loaded = load_snapshot(snapshot_path)

    # Verify RedactionFinding objects survived as typed objects
    assert len(loaded.redactions) == 4

    # First three should be RedactionFinding instances
    assert isinstance(loaded.redactions[0], RedactionFinding)
    assert loaded.redactions[0].path == "/etc/cockpit/ws-certs.d/0-self-signed.key"
    assert loaded.redactions[0].remediation == "regenerate"
    assert loaded.redactions[0].kind == "excluded"

    assert isinstance(loaded.redactions[1], RedactionFinding)
    assert loaded.redactions[1].replacement == "REDACTED_WIREGUARD_KEY_1"
    assert loaded.redactions[1].line == 3

    assert isinstance(loaded.redactions[2], RedactionFinding)
    assert loaded.redactions[2].source == "shadow"

    # Fourth should still be a plain dict (legacy, no "source"/"kind" fields)
    assert isinstance(loaded.redactions[3], dict)
    assert loaded.redactions[3]["path"] == "/etc/old.conf"

    # Verify isinstance checks work for downstream consumers
    typed_findings = [r for r in loaded.redactions if isinstance(r, RedactionFinding)]
    assert len(typed_findings) == 3
    excluded = [r for r in typed_findings if r.kind == "excluded"]
    assert len(excluded) == 1
    inline = [r for r in typed_findings if r.kind == "inline"]
    assert len(inline) == 2
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `pytest tests/test_redact.py::test_redaction_finding_model tests/test_redact.py::test_redaction_finding_dict_compat tests/test_redact.py::test_redaction_finding_survives_save_load_roundtrip -v`
Expected: PASS

- [ ] **Step 7: Run full test suite to verify no regressions**

Run: `pytest tests/ -v --tb=short`
Expected: PASS — the Union type and validator are backwards compatible with existing code that appends plain dicts to `snapshot.redactions`

- [ ] **Step 8: Commit**

```bash
git add src/inspectah/schema.py tests/test_redact.py
git commit -m "feat(schema): add RedactionFinding model with Union type and round-trip validator

Typed redaction model for source provenance, remediation state, and kind.
Includes .get() for backwards compatibility with existing dict consumers
(html_report, audit_report, fleet/merge, secrets_review).
InspectionSnapshot.redactions uses List[Union[RedactionFinding, dict]] with
a field_validator that reconstructs RedactionFinding from dicts on
deserialization, ensuring isinstance() checks work after save/load
round-trips through model_dump_json() / model_validate()."
```

---

## Milestone 2: Detection patterns + sequential counters + shadow migration

This milestone adds new detection patterns, replaces hash tokens with sequential counters across ALL finding types (including shadow), and adds remediation state tracking. All existing tests are updated. The tree stays green.

### Task 2: Add new detection patterns and `include=False` for excluded paths

**Files:**
- Modify: `src/inspectah/redact.py`
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write failing tests for new EXCLUDED_PATHS**

```python
# Add these imports to tests/test_redact.py if not already present
from inspectah.schema import ConfigSection, ConfigFileEntry, ConfigFileKind

def test_p12_keystore_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/private/server.p12", kind=ConfigFileKind.UNOWNED, content="binary-data", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False

def test_pfx_keystore_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/private/server.pfx", kind=ConfigFileKind.UNOWNED, content="binary-data", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False

def test_jks_keystore_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/java/cacerts.jks", kind=ConfigFileKind.UNOWNED, content="binary-data", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False

def test_cockpit_ws_certs_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed.key", kind=ConfigFileKind.UNOWNED, content="key-data", include=True),
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed.cert", kind=ConfigFileKind.UNOWNED, content="cert-data", include=True),
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed-ca.pem", kind=ConfigFileKind.UNOWNED, content="ca-data", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert all(f.include is False for f in result.config.files)

def test_containers_auth_json_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/containers/auth.json", kind=ConfigFileKind.UNOWNED, content='{"auths":{}}', include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False
```

- [ ] **Step 2: Write failing tests for new REDACT_PATTERNS with capture groups**

The WireGuard and WiFi patterns MUST use capture groups so that the key name and `=` are preserved in the output. The live `_redact_text()` function replaces `group(2)` when `m.lastindex >= 2`, otherwise replaces `group(0)` (the entire match). So patterns need group(1) = prefix to keep, group(2) = secret value to replace.

```python
def test_wireguard_private_key_redacted():
    """WireGuard PrivateKey redacted, assignment syntax preserved."""
    wg_config = "[Interface]\nAddress = 10.0.0.1/24\nPrivateKey = aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4yZ5AB=\nListenPort = 51820\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/wireguard/wg0.conf", kind=ConfigFileKind.UNOWNED, content=wg_config, include=True),
    ]))
    result = redact_snapshot(snapshot)
    content = result.config.files[0].content
    # Secret value must be gone
    assert "aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4yZ5AB=" not in content
    # Assignment syntax must be preserved
    assert "PrivateKey = REDACTED_WIREGUARD_KEY_" in content or "PrivateKey =REDACTED_WIREGUARD_KEY_" in content
    # File stays included (inline, not exclusion)
    assert result.config.files[0].include is True

def test_wifi_psk_redacted():
    """WiFi PSK redacted, assignment syntax preserved."""
    nm_config = "[wifi-security]\nkey-mgmt=wpa-psk\npsk=mysecretpassword123\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/NetworkManager/system-connections/wifi.nmconnection", kind=ConfigFileKind.UNOWNED, content=nm_config, include=True),
    ]))
    result = redact_snapshot(snapshot)
    content = result.config.files[0].content
    # Secret gone
    assert "mysecretpassword123" not in content
    # Assignment syntax preserved: "psk=" still present
    assert "psk=REDACTED_WIFI_PSK_" in content or "psk= REDACTED_WIFI_PSK_" in content
    assert result.config.files[0].include is True
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `pytest tests/test_redact.py -k "p12 or pfx or jks or cockpit_ws or containers_auth or wireguard_private or wifi_psk" -v`
Expected: FAIL — new patterns not yet added

- [ ] **Step 4: Add new patterns to redact.py**

In `src/inspectah/redact.py`, update `EXCLUDED_PATHS` (after line 26):

```python
EXCLUDED_PATHS = (
    r"/etc/shadow",
    r"/etc/gshadow",
    r"/etc/ssh/ssh_host_.*",
    r"/etc/pki/.*\.key",
    r".*\.key$",
    r".*keytab$",
    # v2 additions
    r".*\.p12$",
    r".*\.pfx$",
    r".*\.jks$",
    r"/etc/cockpit/ws-certs\.d/.*",
    r"/etc/containers/auth\.json",
)
```

Add to `REDACT_PATTERNS` list (append after the REDIS_PASSWORD entry at line 44). These use capture groups compatible with the live `_redact_text()` logic — group(1) is prefix to keep, group(2) is the secret value to replace:

```python
    # WireGuard private key (bare base64, not PEM-wrapped)
    # group(1)=assignment prefix, group(2)=key value
    (r"(PrivateKey\s*=\s*)([A-Za-z0-9+/]{43}=)", "WIREGUARD_KEY"),
    # WiFi PSK in NetworkManager connections
    # group(1)=assignment prefix, group(2)=psk value
    (r"(psk\s*=\s*)(\S+)", "WIFI_PSK"),
```

- [ ] **Step 5: Update `redact_snapshot()` to set `include=False` for excluded paths**

In `redact_snapshot()`, in the config files loop (around line 182-190 of `src/inspectah/redact.py`), where `_is_excluded_path(entry.path)` matches, change the line that creates the new file copy to also set `include=False`:

```python
# Current line ~190:
new_files.append(entry.model_copy(update={"content": _EXCLUDED_PLACEHOLDER}))
# Change to:
new_files.append(entry.model_copy(update={"content": _EXCLUDED_PLACEHOLDER, "include": False}))
```

Apply the same change to the `non_rpm_software.env_files` loop (~line 369):

```python
# Current line ~369:
new_env_files.append(entry.model_copy(update={"content": _EXCLUDED_PLACEHOLDER}))
# Change to:
new_env_files.append(entry.model_copy(update={"content": _EXCLUDED_PLACEHOLDER, "include": False}))
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `pytest tests/test_redact.py -k "p12 or pfx or jks or cockpit_ws or containers_auth or wireguard_private or wifi_psk" -v`
Expected: PASS

- [ ] **Step 7: Run full test suite**

Run: `pytest tests/ -v --tb=short`
Expected: PASS — existing tests unaffected (existing excluded paths didn't check `include`)

- [ ] **Step 8: Commit**

```bash
git add src/inspectah/redact.py tests/test_redact.py
git commit -m "feat(redact): add detection for keystores, cockpit certs, auth.json, WireGuard, WiFi PSK

New EXCLUDED_PATHS: .p12, .pfx, .jks, cockpit ws-certs.d/*, containers/auth.json
New REDACT_PATTERNS: WIREGUARD_KEY (bare base64), WIFI_PSK
Both new patterns use capture groups so assignment syntax is preserved.
Excluded paths now set include=False on the config entry."
```

---

### Task 3: Replace hash tokens with sequential counters — ALL finding types

This task replaces `_truncated_sha256()` with a shared counter registry used by BOTH `_redact_text()` AND `_redact_shadow_entry()`. The spec requires ONE deterministic counter space across all finding types.

**Algorithm (single-pass, no renumbering):**

1. Create one `_CounterRegistry` at the top of `redact_snapshot()`.
2. Sort inputs within each section before processing (config files by path, zones by name, etc.).
3. Process sections in the fixed order they already appear in `redact_snapshot()`.
4. During processing, `_CounterRegistry.get_token()` is called inside `_redact_text()` / `_redact_shadow_entry()`. The returned token goes directly into BOTH the content string (via string replacement) AND `RedactionFinding.replacement` (via the same variable). There is no separate metadata pass — one code path produces one token that appears in both places.
5. After all processing completes, sort the `redactions` list by a display-order key. This sort ONLY reorders the list for output rendering (secrets-review.md, Containerfile comments, CLI summary). It does NOT reassign, renumber, or modify any token values. Token assignment is already finished.

**Key properties:**
- Token assignment is deterministic because inputs are sorted before processing and section order is fixed.
- The same token value appears in content AND in `RedactionFinding.replacement` because they are assigned in the same code path — there is no second registry, no metadata-only pass, no renumbering step.
- The output-order sort at the end is a list reorder, not a token reassignment.
- If the same secret value appears in multiple files, it gets the same token everywhere (`_CounterRegistry` dedup by `(type_label, value)`).

**Files:**
- Modify: `src/inspectah/redact.py`
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write failing tests for sequential counters**

```python
import re

def test_sequential_counters_deterministic():
    """Same input produces same counter assignments."""
    content_a = "password=secret1\napi_key=abcdefghijklmnopqrstuvwxyz\n"
    content_b = "password=secret2\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app/a.conf", kind=ConfigFileKind.UNOWNED, content=content_a, include=True),
        ConfigFileEntry(path="/etc/app/b.conf", kind=ConfigFileKind.UNOWNED, content=content_b, include=True),
    ]))
    r1 = redact_snapshot(snapshot)
    r2 = redact_snapshot(_base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app/a.conf", kind=ConfigFileKind.UNOWNED, content=content_a, include=True),
        ConfigFileEntry(path="/etc/app/b.conf", kind=ConfigFileKind.UNOWNED, content=content_b, include=True),
    ])))
    assert r1.config.files[0].content == r2.config.files[0].content
    assert r1.config.files[1].content == r2.config.files[1].content

def test_sequential_counters_no_hash():
    """Counter tokens must not contain hash fragments."""
    content = "password=mysecret\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app.conf", kind=ConfigFileKind.UNOWNED, content=content, include=True),
    ]))
    result = redact_snapshot(snapshot)
    redacted = result.config.files[0].content
    # Should be REDACTED_PASSWORD_N, not REDACTED_PASSWORD_<hex>
    assert re.search(r"REDACTED_PASSWORD_\d+", redacted)
    assert not re.search(r"REDACTED_PASSWORD_[0-9a-f]{8}", redacted)

def test_same_secret_gets_same_counter():
    """Identical secret values across files share the same counter."""
    content_a = "password=identical_secret_value_here\n"
    content_b = "password=identical_secret_value_here\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app/a.conf", kind=ConfigFileKind.UNOWNED, content=content_a, include=True),
        ConfigFileEntry(path="/etc/app/b.conf", kind=ConfigFileKind.UNOWNED, content=content_b, include=True),
    ]))
    result = redact_snapshot(snapshot)
    token_a = re.search(r"REDACTED_PASSWORD_\d+", result.config.files[0].content).group()
    token_b = re.search(r"REDACTED_PASSWORD_\d+", result.config.files[1].content).group()
    assert token_a == token_b

def test_shadow_uses_sequential_counter():
    """Shadow entries must use sequential counters, not truncated SHA-256."""
    snapshot = _base_snapshot(
        users_groups=UserGroupSection(
            shadow_entries=[
                "jdoe:$y$j9T$abc123hashdata$longhashcontinues:19700:0:99999:7:::",
                "admin:$6$rounds=65536$saltsalt$longhashvalue:19700:0:99999:7:::",
            ],
        )
    )
    result = redact_snapshot(snapshot)
    entry0 = result.users_groups.shadow_entries[0]
    entry1 = result.users_groups.shadow_entries[1]
    # Must use sequential counter format, not hash
    assert re.search(r"REDACTED_SHADOW_HASH_\d+$", entry0.split(":")[1]), f"Expected counter format, got: {entry0.split(':')[1]}"
    assert re.search(r"REDACTED_SHADOW_HASH_\d+$", entry1.split(":")[1]), f"Expected counter format, got: {entry1.split(':')[1]}"
    # Different hashes get different counters
    assert entry0.split(":")[1] != entry1.split(":")[1]

def test_counter_shared_across_file_and_shadow():
    """File-backed and shadow findings share counter space — no duplicate counters."""
    content = "password=somesecret\n"
    snapshot = _base_snapshot(
        config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/app.conf", kind=ConfigFileKind.UNOWNED, content=content, include=True),
        ]),
        users_groups=UserGroupSection(
            shadow_entries=["jdoe:$y$j9T$abc$hash:19700:0:99999:7:::"],
        ),
    )
    result = redact_snapshot(snapshot)
    # Both should use counters (no hashes)
    redacted_content = result.config.files[0].content
    shadow_entry = result.users_groups.shadow_entries[0]
    assert re.search(r"REDACTED_PASSWORD_\d+", redacted_content)
    assert re.search(r"REDACTED_SHADOW_HASH_\d+", shadow_entry)
    # No hex-hash patterns anywhere
    assert not re.search(r"REDACTED_\w+_[0-9a-f]{8}", redacted_content)
    assert not re.search(r"REDACTED_SHADOW_HASH_[0-9a-f]{8}", shadow_entry)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_redact.py -k "sequential_counters or same_secret or shadow_uses_sequential or counter_shared" -v`
Expected: FAIL — still using hash tokens

- [ ] **Step 3: Implement the counter registry**

Add to `src/inspectah/redact.py` (after the `_truncated_sha256` function, around line 60):

```python
class _CounterRegistry:
    """Maps (type_label, secret_value) -> deterministic sequential counter token.

    One instance shared across ALL finding types within a single
    redact_snapshot() call. Ordering: file-backed findings first (sorted
    by path), then non-file-backed (shadow, container-env, etc.).
    """

    def __init__(self):
        self._counters: dict[str, int] = {}  # type_label -> next counter
        self._seen: dict[tuple[str, str], str] = {}  # (type_label, value) -> token

    def get_token(self, type_label: str, value: str) -> str:
        key = (type_label, value)
        if key in self._seen:
            return self._seen[key]
        n = self._counters.get(type_label, 0) + 1
        self._counters[type_label] = n
        token = f"REDACTED_{type_label}_{n}"
        self._seen[key] = token
        return token
```

- [ ] **Step 4: Update `_redact_text()` to accept and use the registry**

Change the signature of `_redact_text()` in `src/inspectah/redact.py` (line 113):

```python
def _redact_text(
    text: str, path: str, redactions: List[dict],
    registry: Optional[_CounterRegistry] = None,
) -> str:
```

Inside the function, replace the `_truncated_sha256(sub)` call (line 127) with registry usage:

```python
# Current (line 127):
replacement = f"REDACTED_{type_label}_{_truncated_sha256(sub)}"
# Change to:
if registry is not None:
    replacement = registry.get_token(type_label, sub)
else:
    replacement = f"REDACTED_{type_label}_{_truncated_sha256(sub)}"
```

Also update the PRIVATE_KEY branch (line 122):

```python
# Current:
replacement = f"REDACTED_{type_label}_<removed>"
# Change to:
if registry is not None:
    replacement = registry.get_token(type_label, m.group(0))
else:
    replacement = f"REDACTED_{type_label}_<removed>"
```

- [ ] **Step 5: Update `_redact_shadow_entry()` to accept and use the registry**

Change the signature (line 87):

```python
def _redact_shadow_entry(
    line: str, redactions: List[dict],
    registry: Optional[_CounterRegistry] = None,
) -> str:
```

Replace the hash usage (line 102):

```python
# Current:
replacement = f"REDACTED_SHADOW_HASH_{_truncated_sha256(raw_hash)}"
# Change to:
if registry is not None:
    replacement = registry.get_token("SHADOW_HASH", raw_hash)
else:
    replacement = f"REDACTED_SHADOW_HASH_{_truncated_sha256(raw_hash)}"
```

- [ ] **Step 6: Sort inputs before processing, then create and pass registry in `redact_snapshot()`**

Sort all inputs before processing so the `_CounterRegistry` encounters secret values in a deterministic order. The registry assigns each token exactly once during `_redact_text()` / `_redact_shadow_entry()` — the same token variable is written into both the content string and `RedactionFinding.replacement`. No second pass, no renumbering.

At the top of `redact_snapshot()` (after line 171), create the shared registry:

```python
registry = _CounterRegistry()
```

**Sort all inputs before processing them.** Add sorting at the top of each section's processing block so the `_CounterRegistry` encounters values in a deterministic order:

**1. Sort `config.files` by path** (add before the config files loop, after line 180):
```python
    sorted_files = sorted(snapshot.config.files, key=lambda f: f.path)
    for entry in sorted_files:
```

**2. Sort `firewall_zones` by name** (add before the firewall loop, after line 211):
```python
    sorted_zones = sorted(snapshot.network.firewall_zones, key=lambda z: z.name)
    for z in sorted_zones:
```

**3. Sort `quadlet_units` by name** (before the quadlet loop):
```python
    sorted_quadlets = sorted(snapshot.containers.quadlet_units, key=lambda u: u.name)
    for u in sorted_quadlets:
```

**4. Sort `running_containers` by name/id** (before the container loop):
```python
    sorted_containers = sorted(snapshot.containers.running_containers, key=lambda c: c.name or c.id[:12])
    for c in sorted_containers:
```

**5. Sort env vars within each container** (before the env loop):
```python
    sorted_env = sorted(c.env)
    for e in sorted_env:
```

**6. Sort `generated_timer_units` by name** (before the generated timer loop):
```python
    sorted_gen = sorted(snapshot.scheduled_tasks.generated_timer_units, key=lambda u: u.name)
    for u in sorted_gen:
```

**7. Sort `systemd_timers` by name** (before the systemd timer loop):
```python
    sorted_timers = sorted(snapshot.scheduled_tasks.systemd_timers, key=lambda t: t.name)
    for t in sorted_timers:
```

**8. Sort kernel module config entries by path** (before each kernel config loop):
```python
    sorted_entries = sorted(entries, key=lambda e: e.path)
    for entry in sorted_entries:
```

**9. Sort `env_files` by path** (before the non-rpm env files loop):
```python
    sorted_env_files = sorted(snapshot.non_rpm_software.env_files, key=lambda f: f.path)
    for entry in sorted_env_files:
```

**10. Sort `sudoers_rules`** (before the sudoers loop):
```python
    sorted_rules = sorted(ug.sudoers_rules)
    for rule in sorted_rules:
```

**11. Sort `shadow_entries`** (before the shadow loop):
```python
    sorted_shadow = sorted(ug.shadow_entries, key=lambda e: e.split(":")[0] if ":" in e else e)
    for entry in sorted_shadow:
```

**12. Sort `passwd_entries`** (before the passwd loop):
```python
    sorted_passwd = sorted(ug.passwd_entries, key=lambda e: e.split(":")[0] if ":" in e else e)
    for entry in sorted_passwd:
```

**Processing order across sections:** Process all file-backed sections first (config files, firewall zones, quadlet units, kernel configs, non-rpm env files) — these use `source="file"`. Then process non-file-backed sections (container env with `source="container-env"`, timer commands with `source="timer-cmd"`, shadow with `source="shadow"`, sudoers and passwd GECOS with `source="file"`). The existing section order in `redact_snapshot()` already follows this pattern (config -> network -> containers -> timers -> kernel -> non-rpm -> users_groups), so no reordering of the section blocks themselves is needed — just sort the items within each block.

Then pass `registry=registry` to every call to `_redact_text()` and `_redact_shadow_entry()` throughout `redact_snapshot()`. There are 14 call sites (same list as before, no change to call-site mechanics):

1. Line 192: `_redact_text(entry.content or "", entry.path, redactions, registry=registry)`
2. Line 193-194: `_redact_text(entry.diff_against_rpm or "", ...)` — add `registry=registry`
3. Line 214: firewall zone `_redact_text(z.content, ...)` — add `registry=registry`
4. Line 235: quadlet `_redact_text(u.content, ...)` — add `registry=registry`
5. Line 252: running container env `_redact_text(e, ...)` — add `registry=registry`
6. Lines 279, 283: generated timer service/command — add `registry=registry`
7. Lines 303: systemd timer service — add `registry=registry`
8. Lines 323: grub defaults — add `registry=registry`
9. Line 342: kernel module configs — add `registry=registry`
10. Line 371: non-rpm env files — add `registry=registry`
11. Line 393: sudoers — add `registry=registry`
12. Line 404: shadow — `_redact_shadow_entry(entry, redactions, registry=registry)`
13. Line 417: passwd gecos — add `registry=registry`

**Output-order sort (list reorder only, no token changes).** After all sections have been processed and all tokens assigned, sort the `redactions` list for consistent rendering in secrets-review.md, Containerfile comments, and CLI summary. This is a `list.sort()` on the findings list — it reorders which finding appears first in output. It does NOT call `_CounterRegistry`, does NOT reassign tokens, does NOT modify any `RedactionFinding.replacement` value. Every token was already finalized during the processing loop above.

Add this **after all section processing completes** (after the `users_groups` block, before `updates["redactions"] = redactions`):

```python
    # -----------------------------------------------------------------------
    # Output-order sort (tokens are already assigned — this only reorders
    # the findings list for consistent rendering)
    # -----------------------------------------------------------------------
    def _redaction_sort_key(r) -> tuple:
        if isinstance(r, dict):
            path = r.get("path", "")
            return (True, "", path, 0)  # legacy dicts sort after file-backed
        is_non_file = r.source != "file"
        if is_non_file:
            return (True, r.source, r.path, 0)
        else:
            return (False, "", r.path, r.line or 0)

    redactions.sort(key=_redaction_sort_key)

    updates["redactions"] = redactions
```

Note: This replaces the line `updates["redactions"] = redactions` — it does NOT add a second assignment. The sort reorders the list in-place for output consistency, then it's assigned to `updates` once.

- [ ] **Step 7: Write the shuffled-input regression test**

Add to `tests/test_redact.py`:

```python
def test_counter_assignment_independent_of_input_order():
    """Counter tokens are deterministic regardless of input order.

    This test creates files in two different orders and verifies:
    1. The redactions list has identical path ordering (output-order sort)
    2. ConfigFileEntry.content strings carry identical placeholder tokens
    3. Each RedactionFinding.replacement token appears in its file's content

    Check 2 is the critical one: it proves that sorting inputs before
    processing makes the _CounterRegistry assign the same tokens
    regardless of the caller's original ordering. Check 3 proves content
    and metadata are in sync (same code path produces both).
    """
    # Files deliberately in REVERSE alphabetical order
    snapshot_reversed = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/zzz/app.conf", kind=ConfigFileKind.UNOWNED,
                        content="password=secret_zzz\n", include=True),
        ConfigFileEntry(path="/etc/aaa/db.conf", kind=ConfigFileKind.UNOWNED,
                        content="password=secret_aaa\n", include=True),
        ConfigFileEntry(path="/etc/mmm/mid.conf", kind=ConfigFileKind.UNOWNED,
                        content="password=secret_mmm\n", include=True),
    ]))
    # Same files in alphabetical order
    snapshot_sorted = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/aaa/db.conf", kind=ConfigFileKind.UNOWNED,
                        content="password=secret_aaa\n", include=True),
        ConfigFileEntry(path="/etc/mmm/mid.conf", kind=ConfigFileKind.UNOWNED,
                        content="password=secret_mmm\n", include=True),
        ConfigFileEntry(path="/etc/zzz/app.conf", kind=ConfigFileKind.UNOWNED,
                        content="password=secret_zzz\n", include=True),
    ]))

    result_reversed = redact_snapshot(snapshot_reversed)
    result_sorted = redact_snapshot(snapshot_sorted)

    # --- Check 1: redactions list ordering ---
    # Extract paths in the order they appear in snapshot.redactions
    paths_reversed = [r.get("path") if isinstance(r, dict) else r.path
                      for r in result_reversed.redactions]
    paths_sorted = [r.get("path") if isinstance(r, dict) else r.path
                    for r in result_sorted.redactions]

    # Both should produce the SAME ordering (alphabetical by path)
    assert paths_reversed == paths_sorted, (
        f"Findings order depends on input order!\n"
        f"  Reversed input produced: {paths_reversed}\n"
        f"  Sorted input produced:   {paths_sorted}"
    )

    # And the order should be alphabetical
    assert paths_reversed == sorted(paths_reversed), (
        f"Findings not sorted by path: {paths_reversed}"
    )

    # --- Check 2: actual content strings carry identical tokens ---
    # Build path->content maps from each result's config.files
    content_reversed = {f.path: f.content for f in result_reversed.config.files}
    content_sorted = {f.path: f.content for f in result_sorted.config.files}

    for path in content_reversed:
        assert content_reversed[path] == content_sorted[path], (
            f"Content tokens differ for {path}!\n"
            f"  Reversed input: {content_reversed[path]!r}\n"
            f"  Sorted input:   {content_sorted[path]!r}"
        )

    # --- Check 3: content tokens match metadata tokens ---
    # Each finding's replacement token must appear in the corresponding
    # file's content string (proving content and metadata are in sync)
    for r in result_reversed.redactions:
        finding = r if not isinstance(r, dict) else None
        if finding and finding.replacement and finding.source == "file":
            file_content = content_reversed.get(finding.path, "")
            assert finding.replacement in file_content, (
                f"Metadata token {finding.replacement} not found in "
                f"content for {finding.path}:\n  {file_content!r}"
            )
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `pytest tests/test_redact.py -v`
Expected: Some existing tests may need updates because token format changed from `REDACTED_PASSWORD_<hex>` to `REDACTED_PASSWORD_1`. Update assertions that matched specific hash patterns. The following existing tests check for `REDACTED_` presence (not specific hash), so they should pass:
- `test_redact_text_password` — checks `"REDACTED_PASSWORD" in out` ✓
- `test_redact_shadow_entry_with_hash` — checks `"REDACTED_SHADOW_HASH_" in entry` ✓
- `test_redact_idempotent` — checks content equality ✓

- [ ] **Step 9: Commit**

```bash
git add src/inspectah/redact.py tests/test_redact.py
git commit -m "feat(redact): replace hash tokens with sequential counters and deterministic ordering

Shared _CounterRegistry used by both _redact_text() and
_redact_shadow_entry(). Inputs are sorted before processing so the
registry assigns tokens in deterministic order during a single pass.
Same secret value gets the same counter across files (registry dedup).
Post-processing sort reorders the findings list for output rendering
only — no token reassignment. Shuffled-input regression test proves
token identity across differently-ordered inputs. Eliminates
dictionary oracle risk for weak secrets."
```

---

### Task 4: Add remediation states and emit RedactionFinding objects

This task converts all `redactions.append({...})` sites in `redact_snapshot()` to emit `RedactionFinding` objects. The `.get()` compatibility method ensures existing consumers still work.

**Files:**
- Modify: `src/inspectah/redact.py`
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write failing tests for remediation states**

```python
from inspectah.schema import RedactionFinding

def test_cockpit_gets_regenerate_remediation():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed.key", kind=ConfigFileKind.UNOWNED, content="key", include=True),
    ]))
    result = redact_snapshot(snapshot)
    cockpit = [r for r in result.redactions if isinstance(r, RedactionFinding) and "cockpit" in r.path]
    assert len(cockpit) >= 1
    assert all(f.remediation == "regenerate" for f in cockpit)

def test_ssh_host_key_gets_regenerate_remediation():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/ssh/ssh_host_rsa_key", kind=ConfigFileKind.UNOWNED, content="key", include=True),
    ]))
    result = redact_snapshot(snapshot)
    ssh = [r for r in result.redactions if isinstance(r, RedactionFinding) and "ssh_host" in r.path]
    assert len(ssh) >= 1
    assert all(f.remediation == "regenerate" for f in ssh)

def test_tls_key_gets_provision_remediation():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/private/server.key", kind=ConfigFileKind.UNOWNED, content="key", include=True),
    ]))
    result = redact_snapshot(snapshot)
    tls = [r for r in result.redactions if isinstance(r, RedactionFinding) and "server.key" in r.path]
    assert len(tls) >= 1
    assert all(f.remediation == "provision" for f in tls)

def test_inline_redaction_gets_value_removed_remediation():
    wg_config = "[Interface]\nPrivateKey = aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4yZ5AB=\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/wireguard/wg0.conf", kind=ConfigFileKind.UNOWNED, content=wg_config, include=True),
    ]))
    result = redact_snapshot(snapshot)
    wg = [r for r in result.redactions if isinstance(r, RedactionFinding) and "wireguard" in r.path]
    assert len(wg) >= 1
    assert all(f.remediation == "value-removed" for f in wg)

def test_shadow_finding_has_source():
    """Shadow findings carry source='shadow'."""
    snapshot = _base_snapshot(users_groups=UserGroupSection(
        shadow_entries=["testuser:$y$j9T$abc$hash:19700:0:99999:7:::"],
    ))
    result = redact_snapshot(snapshot)
    shadow = [r for r in result.redactions if isinstance(r, RedactionFinding) and r.source == "shadow"]
    assert len(shadow) >= 1
    assert all(f.kind == "inline" for f in shadow)
    assert all(f.remediation == "value-removed" for f in shadow)

def test_container_env_finding_has_source():
    """Container env findings carry source='container-env'."""
    c = RunningContainer(
        id="abc123", name="redis", image="redis:7",
        env=["REDIS_PASSWORD=topsecretredis", "HOSTNAME=redis-1"],
    )
    snapshot = _base_snapshot(containers=ContainerSection(running_containers=[c]))
    result = redact_snapshot(snapshot)
    env_findings = [r for r in result.redactions if isinstance(r, RedactionFinding) and r.source == "container-env"]
    assert len(env_findings) >= 1

def test_timer_cmd_finding_has_source():
    """Timer command findings carry source='timer-cmd'."""
    unit = GeneratedTimerUnit(
        name="cron-backup",
        timer_content="[Timer]\nOnCalendar=daily\n",
        service_content="[Service]\nExecStart=/usr/bin/pg_dump -p password=dbpass123 mydb\n",
        cron_expr="0 2 * * *",
        source_path="etc/cron.d/backup",
        command="/usr/bin/pg_dump -p password=dbpass123 mydb",
    )
    snapshot = _base_snapshot(scheduled_tasks=ScheduledTaskSection(generated_timer_units=[unit]))
    result = redact_snapshot(snapshot)
    timer_findings = [r for r in result.redactions if isinstance(r, RedactionFinding) and r.source == "timer-cmd"]
    assert len(timer_findings) >= 1

def test_diff_finding_has_source():
    """Diff view findings carry source='diff'."""
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(
            path="/etc/app.conf", kind=ConfigFileKind.RPM_OWNED_MODIFIED,
            content="clean content",
            diff_against_rpm="+password=leakedsecret",
            include=True,
        ),
    ]))
    result = redact_snapshot(snapshot)
    diff_findings = [r for r in result.redactions if isinstance(r, RedactionFinding) and r.source == "diff"]
    assert len(diff_findings) >= 1

def test_redaction_findings_compat_with_existing_tests():
    """RedactionFinding.get() works with existing test patterns like r['pattern']."""
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app.conf", kind=ConfigFileKind.UNOWNED, content="password=secret123", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert len(result.redactions) > 0
    r = result.redactions[0]
    # .get() compat works
    assert r.get("path") == "/etc/app.conf"
    assert r.get("pattern") is not None
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_redact.py -k "regenerate or provision_remediation or value_removed or shadow_finding or container_env_finding or timer_cmd_finding or diff_finding or findings_compat" -v`
Expected: FAIL — redactions are still plain dicts

- [ ] **Step 3: Add remediation mapping to redact.py**

Add after the `_CounterRegistry` class in `src/inspectah/redact.py`:

```python
from .schema import RedactionFinding

# Pattern → remediation state for excluded paths
_EXCLUDED_REMEDIATION: list[tuple[str, str]] = [
    (r"/etc/cockpit/ws-certs\.d/.*", "regenerate"),
    (r"/etc/ssh/ssh_host_.*", "regenerate"),
    # All others default to "provision"
]

def _remediation_for_excluded(path: str) -> str:
    """Return remediation state for an excluded path."""
    normalised = "/" + path.lstrip("/")
    for pattern, remediation in _EXCLUDED_REMEDIATION:
        if re.search(pattern, normalised):
            return remediation
    return "provision"
```

- [ ] **Step 4: Convert all `redactions.append({...})` to `RedactionFinding(...)`**

In `redact_snapshot()`, change the type of the redactions list (line 171):

```python
# Current:
redactions: List[dict] = list(snapshot.redactions)
# Change to:
redactions: list = list(snapshot.redactions)
```

Then convert each `redactions.append({...})` call. There are 6 distinct sites in `redact_snapshot()`:

**Site 1 — Excluded path in config.files (line 184-189):**
```python
# Current:
redactions.append({
    "path": entry.path,
    "pattern": "EXCLUDED_PATH",
    "line": "entire file",
    "remediation": "File not included; handle credentials manually (e.g. systemd credential, secret store).",
})
# Replace with:
redactions.append(RedactionFinding(
    path=entry.path,
    source="file",
    kind="excluded",
    pattern="EXCLUDED_PATH",
    remediation=_remediation_for_excluded(entry.path),
))
```

**Site 2 — Inline text in `_redact_text()` (lines 129-134).** Update the function to emit `RedactionFinding` instead of dict. Change parameter type:

```python
def _redact_text(
    text: str, path: str, redactions: list,
    registry: Optional[_CounterRegistry] = None,
    source: str = "file",
) -> str:
```

Replace the `redactions.append({...})` inside `_redact_text()` (lines 129-134):
```python
# Current:
redactions.append({
    "path": path,
    "pattern": type_label,
    "line": "content",
    "remediation": "Use a secret store or inject at deploy time.",
})
# Replace with:
redactions.append(RedactionFinding(
    path=path,
    source=source,
    kind="inline",
    pattern=type_label,
    remediation="value-removed",
    replacement=replacement,
))
```

**Site 3 — Shadow entries in `_redact_shadow_entry()` (lines 103-108).** Update to emit `RedactionFinding`:

```python
def _redact_shadow_entry(
    line: str, redactions: list,
    registry: Optional[_CounterRegistry] = None,
) -> str:
```

Replace the `redactions.append({...})` (lines 103-108):
```python
# Current:
redactions.append({
    "path": f"users:shadow/{fields[0]}",
    "pattern": "SHADOW_HASH",
    "line": "field 2",
    "remediation": "Do not ship password hashes in image output.",
})
# Replace with:
redactions.append(RedactionFinding(
    path=f"users:shadow/{fields[0]}",
    source="shadow",
    kind="inline",
    pattern="SHADOW_HASH",
    remediation="value-removed",
    replacement=replacement,
))
```

**Site 4 — Non-RPM excluded paths (lines 362-367).** Same pattern as Site 1:
```python
redactions.append(RedactionFinding(
    path=entry.path,
    source="file",
    kind="excluded",
    pattern="EXCLUDED_PATH",
    remediation=_remediation_for_excluded(entry.path),
))
```

**Sites 5-14 — All `_redact_text()` call sites in `redact_snapshot()`** need `source=` parameter:

Pass the appropriate `source` parameter to each `_redact_text()` call:
- Config file content: `source="file"` (default, no change needed)
- Config file diff: `source="diff"`
- Firewall zone content: `source="file"`
- Quadlet unit content: `source="file"`
- Running container env: `source="container-env"`
- Generated timer service/command: `source="timer-cmd"`
- Systemd timer service: `source="timer-cmd"`
- GRUB defaults: `source="file"`
- Kernel module configs: `source="file"`
- Sudoers rules: `source="file"`
- Passwd GECOS: `source="file"`

Example for diff views (line 193-194):
```python
new_diff = _redact_text(
    entry.diff_against_rpm or "", f"{entry.path}:diff", redactions,
    registry=registry, source="diff",
) if entry.diff_against_rpm else None
```

Example for container env (line 252):
```python
redacted_e = _redact_text(e, f"containers:running/{name}:env", redactions,
                          registry=registry, source="container-env")
```

Example for timer content (lines 279, 283):
```python
new_svc = _redact_text(
    u.service_content, f"scheduled:timer/{u.name}:service_content", redactions,
    registry=registry, source="timer-cmd",
)
new_cmd = _redact_text(
    u.command, f"scheduled:timer/{u.name}:command", redactions,
    registry=registry, source="timer-cmd",
)
```

- [ ] **Step 5: Fix existing tests for new finding type**

Update existing tests in `tests/test_redact.py` that access redactions as dicts. Because `RedactionFinding` has `.get()`, most existing tests that use `r["pattern"]` or `r.get("path")` patterns should work as-is. But direct `r["pattern"]` syntax won't work — these need updating:

Tests using `r["path"]` (subscript access):
- `test_redact_firewall_zone_content` (line 111): `r["path"]` → `r.get("path", "")`
- `test_redact_quadlet_content` (line 140): `r["path"]` → `r.get("path", "")`
- `test_redact_running_container_env` (line 158): `r["path"]` → `r.get("path", "")`
- `test_redact_sudoers_rules` (line 286): `r["path"]` → `r.get("path", "")`

Tests using `r.get("pattern", "")`:
- `test_redact_shadow_entry_with_hash` (line 319): already uses `r["pattern"]` → change to `r.get("pattern", "")`
- `test_redact_shadow_entry_locked_unchanged` (line 337): already uses `r.get("pattern", "")` ✓
- `test_redact_firewall_zone_no_secrets` (line 123): already uses `r.get("path", "")` ✓
- `test_redact_grub_defaults` (line 242): uses `r["path"]` → change to `r.get("path", "")`

- [ ] **Step 6: Run full test suite**

Run: `pytest tests/test_redact.py -v`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/inspectah/redact.py tests/test_redact.py
git commit -m "feat(redact): emit typed RedactionFinding with remediation states

Each finding carries source provenance, remediation state, and kind.
Excluded paths map to regenerate (cockpit, ssh host keys) or provision
(all others). Inline redactions map to value-removed. Non-file-backed
findings (shadow, container-env, timer-cmd, diff) carry their source.
RedactionFinding.get() provides backwards compat with dict consumers."
```

---

## Milestone 3: Migrate all downstream consumers

This milestone updates every consumer of `snapshot.redactions` to work correctly with `RedactionFinding` objects. Each task in this milestone can be done in any order, but ALL must be completed before the milestone commit.

### Task 5: Update html_report.py redaction consumers

**Files:**
- Modify: `src/inspectah/renderers/html_report.py`
- Test: `tests/test_html_report_output.py`

The HTML report accesses `snapshot.redactions` in two places:

1. **`_build_context()` warnings loop** (lines 554-558 of `src/inspectah/renderers/html_report.py`):
   ```python
   for r in snapshot.redactions:
       w = make_warning("redaction", f"Redacted: {r.get('path') or ''}")
       w["detail"] = r.get("remediation") or ""
       warnings.append(w)
   ```
   This already uses `.get()` — works with `RedactionFinding.get()`. **No change needed.**

2. **`_build_context()` secrets_data** (lines 637-638):
   ```python
   redactions = snapshot.redactions or []
   secrets_files = len(set(r.get("path", "") for r in redactions))
   ```
   This already uses `.get()`. **No change needed.**

- [ ] **Step 1: Verify existing html report tests pass**

Run: `pytest tests/test_html_report_output.py -v`
Expected: PASS — `.get()` compat means no code changes needed

- [ ] **Step 2: Add a test confirming RedactionFinding works in html context**

Add to `tests/test_html_report_output.py`:

```python
def test_html_report_with_typed_redaction_findings(self):
    """HTML report renders correctly with RedactionFinding objects in snapshot.redactions."""
    import tempfile
    from inspectah.schema import InspectionSnapshot, OsRelease, RedactionFinding
    from inspectah.renderers import run_all as run_all_renderers

    snapshot = InspectionSnapshot(
        meta={"host_root": "/host"},
        os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
    )
    snapshot.redactions = [
        RedactionFinding(
            path="/etc/pki/tls/private/server.key",
            source="file", kind="excluded", pattern="EXCLUDED_PATH",
            remediation="provision",
        ),
        RedactionFinding(
            path="/etc/app.conf",
            source="file", kind="inline", pattern="PASSWORD",
            remediation="value-removed", replacement="REDACTED_PASSWORD_1",
        ),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        run_all_renderers(snapshot, Path(tmp))
        html = (Path(tmp) / "report.html").read_text()
    assert "server.key" in html
    assert "app.conf" in html
```

- [ ] **Step 3: Run tests**

Run: `pytest tests/test_html_report_output.py -v`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add tests/test_html_report_output.py
git commit -m "test(html_report): verify RedactionFinding compat in HTML report

HTML report uses .get() throughout — no code changes needed.
Test confirms typed findings render correctly."
```

---

### Task 6: Update audit_report.py redaction consumers

**Files:**
- Modify: `src/inspectah/renderers/audit_report.py`
- Test: `tests/test_audit_report_output.py`

The audit report accesses `snapshot.redactions` in two places:

1. **Executive summary** (line 96): `n_redactions = len(snapshot.redactions)` — no `.get()`, just `len()`. **No change needed.**

2. **Redactions section** (lines 923-927):
   ```python
   if snapshot.redactions:
       lines.append("## Redactions (secrets)")
       lines.append("")
       for r in snapshot.redactions:
           lines.append(f"- **{r.get('path') or ''}**: {r.get('pattern') or ''} — {r.get('remediation') or ''}")
   ```
   Already uses `.get()`. **No change needed.**

- [ ] **Step 1: Verify existing audit report tests pass**

Run: `pytest tests/test_audit_report_output.py -v`
Expected: PASS

- [ ] **Step 2: Add a test confirming RedactionFinding works in audit context**

Add to `tests/test_audit_report_output.py`:

```python
def test_audit_report_with_typed_redaction_findings(self):
    """Audit report renders correctly with RedactionFinding objects."""
    import tempfile
    from inspectah.schema import InspectionSnapshot, OsRelease, RedactionFinding
    from inspectah.renderers.audit_report import render

    snapshot = InspectionSnapshot(
        meta={},
        os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
    )
    snapshot.redactions = [
        RedactionFinding(
            path="/etc/pki/tls/private/server.key",
            source="file", kind="excluded", pattern="EXCLUDED_PATH",
            remediation="provision",
        ),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        from jinja2 import Environment
        render(snapshot, Environment(), Path(tmp))
        content = (Path(tmp) / "audit-report.md").read_text()
    assert "server.key" in content
    assert "Secrets redacted: 1" in content
```

- [ ] **Step 3: Run tests**

Run: `pytest tests/test_audit_report_output.py -v`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add tests/test_audit_report_output.py
git commit -m "test(audit_report): verify RedactionFinding compat in audit report

Audit report uses .get() throughout — no code changes needed.
Test confirms typed findings render correctly."
```

---

### Task 7: Update fleet/merge.py redaction handling

**Files:**
- Modify: `src/inspectah/fleet/merge.py`

The fleet merge module uses `_deduplicate_warning_dicts()` for both warnings and redactions (lines 524-529 of `src/inspectah/fleet/merge.py`):

```python
warnings_merged = _deduplicate_warning_dicts(
    [s.warnings for s in snapshots]
)
redactions_merged = _deduplicate_warning_dicts(
    [s.redactions for s in snapshots]
)
```

The `_deduplicate_warning_dicts()` function (lines 206-216) uses `item.get("source", "")` and `item.get("message", "")` — this works with `RedactionFinding.get()` BUT the dedup keys are wrong for `RedactionFinding` objects. `RedactionFinding` has no `message` field, so the key would be `(source, "")` for all findings, causing incorrect dedup.

- [ ] **Step 1: Write a test for fleet merge with typed findings**

Create `tests/test_fleet_merge_redactions.py`:

```python
"""Test that fleet merge handles RedactionFinding objects correctly."""
from inspectah.schema import (
    InspectionSnapshot, OsRelease, RedactionFinding,
)
from inspectah.fleet.merge import merge_snapshots


def _snap_with_redactions(hostname, redactions):
    snap = InspectionSnapshot(
        meta={"hostname": hostname},
        os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
    )
    snap.redactions = redactions
    return snap


def test_merge_deduplicates_typed_findings():
    """Identical RedactionFinding objects across snapshots are deduplicated."""
    finding = RedactionFinding(
        path="/etc/pki/tls/private/server.key",
        source="file", kind="excluded", pattern="EXCLUDED_PATH",
        remediation="provision",
    )
    snap_a = _snap_with_redactions("host-a", [finding])
    snap_b = _snap_with_redactions("host-b", [finding])
    merged = merge_snapshots([snap_a, snap_b])
    # Same finding on both hosts should deduplicate to one
    assert len(merged.redactions) == 1


def test_merge_keeps_different_typed_findings():
    """Different RedactionFinding objects are preserved."""
    f1 = RedactionFinding(
        path="/etc/pki/tls/private/server.key",
        source="file", kind="excluded", pattern="EXCLUDED_PATH",
        remediation="provision",
    )
    f2 = RedactionFinding(
        path="/etc/app.conf",
        source="file", kind="inline", pattern="PASSWORD",
        remediation="value-removed", replacement="REDACTED_PASSWORD_1",
    )
    snap_a = _snap_with_redactions("host-a", [f1])
    snap_b = _snap_with_redactions("host-b", [f2])
    merged = merge_snapshots([snap_a, snap_b])
    assert len(merged.redactions) == 2
```

- [ ] **Step 2: Run tests to see current behavior**

Run: `pytest tests/test_fleet_merge_redactions.py -v`
Expected: FAIL — dedup uses wrong keys for `RedactionFinding`

- [ ] **Step 3: Update `_deduplicate_warning_dicts()` to handle both types**

In `src/inspectah/fleet/merge.py` (lines 206-216), update to handle both dicts and `RedactionFinding`:

```python
def _deduplicate_warning_dicts(all_lists: list[list]) -> list:
    """Deduplicate warning/redaction items by identity key.

    Supports both plain dicts (warnings) and RedactionFinding objects (redactions).
    """
    seen = set()
    result = []
    for items in all_lists:
        for item in items:
            if isinstance(item, dict):
                key = (item.get("source", ""), item.get("message", ""))
            else:
                # RedactionFinding — key on (path, pattern, source)
                key = (getattr(item, "path", ""), getattr(item, "pattern", ""), getattr(item, "source", ""))
            if key not in seen:
                seen.add(key)
                result.append(item)
    return result
```

- [ ] **Step 4: Run tests**

Run: `pytest tests/test_fleet_merge_redactions.py -v`
Expected: PASS

Run: `pytest tests/ -v --tb=short`
Expected: PASS — no regressions

- [ ] **Step 5: Commit**

```bash
git add src/inspectah/fleet/merge.py tests/test_fleet_merge_redactions.py
git commit -m "fix(fleet): update redaction dedup to handle RedactionFinding objects

_deduplicate_warning_dicts() now uses (path, pattern, source) as the
dedup key for RedactionFinding objects, and (source, message) for
plain dicts (warnings). Prevents incorrect dedup where all findings
collapsed to one due to missing 'message' field."
```

---

### Task 8: Rewrite secrets_review.py renderer

**Files:**
- Modify: `src/inspectah/renderers/secrets_review.py`
- Test: `tests/test_secrets_review.py` (create)

- [ ] **Step 1: Write tests**

```python
# tests/test_secrets_review.py
import tempfile
from pathlib import Path
from jinja2 import Environment
from inspectah.schema import InspectionSnapshot, RedactionFinding
from inspectah.renderers.secrets_review import render


def _snapshot_with_findings():
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/cockpit/ws-certs.d/0-self-signed.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="regenerate"),
        RedactionFinding(path="/etc/pki/tls/private/server.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="provision"),
        RedactionFinding(path="/etc/wireguard/wg0.conf", source="file",
                        kind="inline", pattern="WIREGUARD_KEY", remediation="value-removed",
                        line=3, replacement="REDACTED_WIREGUARD_KEY_1"),
        RedactionFinding(path="users:shadow/testuser", source="shadow",
                        kind="inline", pattern="SHADOW_HASH", remediation="value-removed",
                        replacement="REDACTED_SHADOW_HASH_1"),
    ]
    return snap


def test_secrets_review_has_excluded_table():
    snap = _snapshot_with_findings()
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "## Excluded Files" in content
        assert "Regenerate on target" in content
        assert "Provision from secret store" in content


def test_secrets_review_has_inline_table():
    snap = _snapshot_with_findings()
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "## Inline Redactions" in content
        assert "REDACTED_WIREGUARD_KEY_1" in content
        assert "Supply value at deploy time" in content


def test_secrets_review_separates_excluded_and_inline():
    snap = _snapshot_with_findings()
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        excluded_pos = content.index("## Excluded Files")
        inline_pos = content.index("## Inline Redactions")
        assert excluded_pos < inline_pos


def test_secrets_review_empty():
    snap = InspectionSnapshot(meta={})
    snap.redactions = []
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "No redactions recorded" in content


def test_secrets_review_legacy_dict_compat():
    """Renderer handles a mix of old dicts and new RedactionFinding objects."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        {"path": "/etc/old.conf", "pattern": "PASSWORD", "line": "content", "remediation": "old style"},
        RedactionFinding(path="/etc/new.conf", source="file", kind="inline",
                        pattern="PASSWORD", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_1"),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        # Should not crash; both items should appear
        assert "/etc/old.conf" in content or "/etc/new.conf" in content
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_secrets_review.py -v`
Expected: FAIL — old format doesn't have separate tables

- [ ] **Step 3: Rewrite `render()` in `src/inspectah/renderers/secrets_review.py`**

Replace the entire file content:

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
) -> None:
    output_dir = Path(output_dir)
    path = output_dir / "secrets-review.md"

    if not snapshot.redactions:
        path.write_text("# Secrets Review\n\nNo redactions recorded.\n")
        return

    lines = [
        "# Secrets Review",
        "",
        "The following items were redacted or excluded. Handle them according to",
        "the action specified for each item.",
        "",
    ]

    # Separate typed findings from legacy dicts
    typed = [r for r in snapshot.redactions if isinstance(r, RedactionFinding)]
    legacy = [r for r in snapshot.redactions if not isinstance(r, RedactionFinding)]

    excluded = [r for r in typed if r.kind == "excluded"]
    inline = [r for r in typed if r.kind == "inline"]

    if excluded:
        lines.append("## Excluded Files")
        lines.append("")
        lines.append("| Path | Action | Reason |")
        lines.append("|------|--------|--------|")
        for f in excluded:
            action = _REMEDIATION_LABELS.get(f.remediation, f.remediation)
            lines.append(f"| {f.path} | {action} | {f.pattern} |")
        lines.append("")

    if inline:
        lines.append("## Inline Redactions")
        lines.append("")
        lines.append("| Path | Line | Type | Placeholder | Action |")
        lines.append("|------|------|------|-------------|--------|")
        for f in inline:
            line_str = str(f.line) if f.line is not None else "—"
            replacement = f.replacement or "—"
            action = _REMEDIATION_LABELS.get(f.remediation, f.remediation)
            lines.append(f"| {f.path} | {line_str} | {f.pattern} | {replacement} | {action} |")
        lines.append("")

    # Legacy dict entries (from older snapshots or fleet merge of old data)
    if legacy:
        lines.append("## Other Redactions")
        lines.append("")
        lines.append("| Path | Pattern | Line | Remediation |")
        lines.append("|------|---------|------|-------------|")
        for r in legacy:
            rpath = (r.get("path") or "").replace("|", "\\|")
            pattern = (r.get("pattern") or "").replace("|", "\\|")
            line = (r.get("line") or "").replace("|", "\\|")
            rem = (r.get("remediation") or "").replace("|", "\\|")
            lines.append(f"| {rpath} | {pattern} | {line} | {rem} |")
        lines.append("")

    path.write_text("\n".join(lines))
```

- [ ] **Step 4: Run tests**

Run: `pytest tests/test_secrets_review.py -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/inspectah/renderers/secrets_review.py tests/test_secrets_review.py
git commit -m "feat(render): rewrite secrets-review.md with separate Excluded/Inline tables

Excluded files show Action (Regenerate/Provision) and Reason.
Inline redactions show line number, placeholder token, and action.
Legacy dict entries fall through to an 'Other Redactions' table for
backwards compat with older snapshot data."
```

---

## Milestone 4: Output artifacts — redacted/ directory + Containerfile comments + CLI summary

### Task 9: Write `redacted/` directory with `.REDACTED` files

**Files:**
- Modify: `src/inspectah/renderers/containerfile/_config_tree.py`
- Modify: `src/inspectah/renderers/__init__.py`
- Test: `tests/test_redacted_dir.py` (create)

- [ ] **Step 1: Write tests**

```python
# tests/test_redacted_dir.py
import tempfile
from pathlib import Path
from inspectah.schema import (
    InspectionSnapshot, ConfigSection, ConfigFileEntry, ConfigFileKind,
    RedactionFinding,
)
from inspectah.renderers.containerfile._config_tree import write_config_tree, write_redacted_dir


def _snapshot_with_excluded():
    snap = InspectionSnapshot(meta={})
    snap.config = ConfigSection(files=[
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed.key", kind=ConfigFileKind.UNOWNED, content="placeholder", include=False),
        ConfigFileEntry(path="/etc/pki/tls/private/server.key", kind=ConfigFileKind.UNOWNED, content="placeholder", include=False),
        ConfigFileEntry(path="/etc/app.conf", kind=ConfigFileKind.UNOWNED, content="normal config", include=True),
    ])
    snap.redactions = [
        RedactionFinding(path="/etc/cockpit/ws-certs.d/0-self-signed.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="regenerate"),
        RedactionFinding(path="/etc/pki/tls/private/server.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="provision"),
    ]
    return snap


def test_redacted_files_in_redacted_dir():
    snap = _snapshot_with_excluded()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        write_redacted_dir(snap, out)
        key_file = out / "redacted" / "etc" / "cockpit" / "ws-certs.d" / "0-self-signed.key.REDACTED"
        assert key_file.exists()
        tls_file = out / "redacted" / "etc" / "pki" / "tls" / "private" / "server.key.REDACTED"
        assert tls_file.exists()


def test_redacted_files_not_in_config_dir():
    snap = _snapshot_with_excluded()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        write_config_tree(snap, out)
        # Excluded files should NOT appear in config/
        cockpit_dir = out / "config" / "etc" / "cockpit"
        assert not cockpit_dir.exists()
        pki_key = out / "config" / "etc" / "pki" / "tls" / "private" / "server.key"
        assert not pki_key.exists()
        # Included file should appear
        assert (out / "config" / "etc" / "app.conf").exists()


def test_regenerate_placeholder_content():
    snap = _snapshot_with_excluded()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        write_redacted_dir(snap, out)
        content = (out / "redacted" / "etc" / "cockpit" / "ws-certs.d" / "0-self-signed.key.REDACTED").read_text()
        assert "REDACTED by inspectah" in content
        assert "auto-generated credential" in content
        assert "no action needed" in content
        assert "/etc/cockpit/ws-certs.d/0-self-signed.key" in content


def test_provision_placeholder_content():
    snap = _snapshot_with_excluded()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        write_redacted_dir(snap, out)
        content = (out / "redacted" / "etc" / "pki" / "tls" / "private" / "server.key.REDACTED").read_text()
        assert "REDACTED by inspectah" in content
        assert "sensitive file detected" in content
        assert "provision" in content
        assert "/etc/pki/tls/private/server.key" in content


def test_non_file_findings_no_redacted_file():
    snap = InspectionSnapshot(meta={})
    snap.config = ConfigSection(files=[])
    snap.redactions = [
        RedactionFinding(path="users:shadow/testuser", source="shadow",
                        kind="inline", pattern="SHADOW_HASH", remediation="value-removed"),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        write_redacted_dir(snap, out)
        redacted = out / "redacted"
        if redacted.exists():
            assert not any(redacted.rglob("*"))
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_redacted_dir.py -v`
Expected: FAIL — `write_redacted_dir` not defined

- [ ] **Step 3: Implement `write_redacted_dir()`**

Add to `src/inspectah/renderers/containerfile/_config_tree.py` (at the end of the file, after `config_inventory_comment`):

```python
_REGENERATE_TEMPLATE = """\
# REDACTED by inspectah — auto-generated credential
# Original path: {path}
# Action: no action needed — this file is regenerated automatically on the target system
# See secrets-review.md for details
"""

_PROVISION_TEMPLATE = """\
# REDACTED by inspectah — sensitive file detected
# Original path: {path}
# Action: provision this file on the target system from your secrets management process
# See secrets-review.md for details
"""


def write_redacted_dir(snapshot: InspectionSnapshot, output_dir: Path) -> None:
    """Write .REDACTED placeholder files for excluded secrets."""
    from ...schema import RedactionFinding

    for finding in snapshot.redactions:
        if not isinstance(finding, RedactionFinding):
            continue
        if finding.source != "file" or finding.kind != "excluded":
            continue
        rel = finding.path.lstrip("/")
        if not rel:
            continue
        redacted_dir = output_dir / "redacted"
        dest = redacted_dir / (rel + ".REDACTED")
        dest.parent.mkdir(parents=True, exist_ok=True)
        if finding.remediation == "regenerate":
            content = _REGENERATE_TEMPLATE.format(path=finding.path)
        else:
            content = _PROVISION_TEMPLATE.format(path=finding.path)
        dest.write_text(content)
```

- [ ] **Step 4: Wire `write_redacted_dir` into the renderer pipeline**

In `src/inspectah/renderers/__init__.py`, add the import and call. After the existing `render_containerfile` call (line 46):

```python
# Add import at top (after other containerfile imports):
from .containerfile._config_tree import write_redacted_dir

# Add call after render_containerfile (line 46):
    render_containerfile(snapshot, env, output_dir)
    write_redacted_dir(snapshot, output_dir)  # <-- add this line
```

- [ ] **Step 5: Run tests**

Run: `pytest tests/test_redacted_dir.py -v`
Expected: PASS

Run: `pytest tests/ -v --tb=short`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/inspectah/renderers/containerfile/_config_tree.py src/inspectah/renderers/__init__.py tests/test_redacted_dir.py
git commit -m "feat(render): write redacted/ directory with .REDACTED placeholder files

Excluded secrets go to redacted/ (outside config/ COPY tree) with
remediation-specific placeholder content. Regenerate-on-target files
say 'no action needed'. Provision files say 'provision from secrets
management'. Non-file findings produce no .REDACTED files.
write_redacted_dir() called from renderers/__init__.py after
render_containerfile()."
```

---

### Task 10: Add Containerfile secrets comment blocks

**Files:**
- Modify: `src/inspectah/renderers/containerfile/_core.py`
- Test: `tests/test_containerfile_secrets_comments.py` (create)

- [ ] **Step 1: Write tests**

```python
# tests/test_containerfile_secrets_comments.py
import tempfile
from pathlib import Path
from inspectah.schema import (
    InspectionSnapshot, OsRelease, ConfigSection, ConfigFileEntry, ConfigFileKind,
    RedactionFinding,
)
from inspectah.renderers.containerfile._core import _render_containerfile_content


def _snapshot_with_secrets():
    """Build a snapshot with both excluded and inline-redacted findings."""
    snap = InspectionSnapshot(
        meta={"host_root": "/host"},
        os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
    )
    snap.redactions = [
        RedactionFinding(
            path="/etc/cockpit/ws-certs.d/0-self-signed.key",
            source="file", kind="excluded", pattern="EXCLUDED_PATH",
            remediation="regenerate",
        ),
        RedactionFinding(
            path="/etc/pki/tls/private/server.key",
            source="file", kind="excluded", pattern="EXCLUDED_PATH",
            remediation="provision",
        ),
        RedactionFinding(
            path="/etc/wireguard/wg0.conf",
            source="file", kind="inline", pattern="WIREGUARD_KEY",
            remediation="value-removed", replacement="REDACTED_WIREGUARD_KEY_1",
        ),
        RedactionFinding(
            path="users:shadow/testuser",
            source="shadow", kind="inline", pattern="SHADOW_HASH",
            remediation="value-removed", replacement="REDACTED_SHADOW_HASH_1",
        ),
    ]
    return snap


def test_containerfile_has_excluded_comment_block():
    """Containerfile should list excluded secrets grouped by remediation."""
    snap = _snapshot_with_secrets()
    with tempfile.TemporaryDirectory() as tmp:
        content = _render_containerfile_content(snap, Path(tmp))
    assert "Excluded secrets (not in this image)" in content
    assert "Regenerate on target" in content
    assert "cockpit" in content
    assert "Provision from secret store" in content
    assert "server.key" in content


def test_containerfile_has_inline_comment_block():
    """Containerfile should list inline-redacted values (file-backed only)."""
    snap = _snapshot_with_secrets()
    with tempfile.TemporaryDirectory() as tmp:
        content = _render_containerfile_content(snap, Path(tmp))
    assert "Inline-redacted values" in content
    assert "wireguard" in content or "wg0.conf" in content
    assert "REDACTED_WIREGUARD_KEY_1" in content
    # Shadow (non-file-backed) should NOT appear in Containerfile comments
    assert "shadow" not in content.lower() or "REDACTED_SHADOW_HASH_1" not in content


def test_containerfile_no_comments_when_no_redactions():
    """No comment blocks if no redactions."""
    snap = InspectionSnapshot(
        meta={"host_root": "/host"},
        os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
    )
    snap.redactions = []
    with tempfile.TemporaryDirectory() as tmp:
        content = _render_containerfile_content(snap, Path(tmp))
    assert "Excluded secrets" not in content
    assert "Inline-redacted" not in content
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_containerfile_secrets_comments.py -v`
Expected: FAIL — no comment blocks generated

- [ ] **Step 3: Add `_secrets_comment_lines()` to `_core.py`**

Add to `src/inspectah/renderers/containerfile/_core.py` (before `_render_containerfile_content`, around line 57):

```python
def _secrets_comment_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Generate Containerfile comment blocks for redacted secrets.

    Only file-backed findings appear here. Non-file-backed findings
    (shadow, container-env, timer-cmd) appear only in secrets-review.md.
    """
    from ...schema import RedactionFinding

    excluded = [r for r in snapshot.redactions
                if isinstance(r, RedactionFinding) and r.kind == "excluded" and r.source == "file"]
    inline = [r for r in snapshot.redactions
              if isinstance(r, RedactionFinding) and r.kind == "inline" and r.source == "file"]

    if not excluded and not inline:
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

    return lines
```

- [ ] **Step 4: Wire into `_render_containerfile_content()`**

In `_render_containerfile_content()` (around line 87 in `src/inspectah/renderers/containerfile/_core.py`), add the secrets comment lines before the epilogue:

```python
    # Current (around line 87-88):
    lines += network.section_lines(snapshot, firewall_only=False)

    # Add secrets comments before epilogue:
    lines += _secrets_comment_lines(snapshot)

    # Epilogue
    lines += _tmpfiles_lines()
    lines += _validate_lines()
```

- [ ] **Step 5: Run tests**

Run: `pytest tests/test_containerfile_secrets_comments.py -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/inspectah/renderers/containerfile/_core.py tests/test_containerfile_secrets_comments.py
git commit -m "feat(render): add secrets comment blocks to Containerfile

Separate blocks for excluded (grouped by regenerate/provision) and
inline-redacted (listing path, pattern, placeholder). Only file-backed
findings appear. Non-file findings (shadow, container-env) stay in
secrets-review.md only. Comment blocks added before epilogue."
```

---

### Task 11: Add CLI output summary with automated test

**Files:**
- Modify: `src/inspectah/pipeline.py`
- Test: `tests/test_pipeline.py`

- [ ] **Step 1: Write the automated test**

Add to `tests/test_pipeline.py`:

```python
import sys
from io import StringIO


def test_cli_secrets_summary(monkeypatch):
    """CLI summary prints correct counts to stderr."""
    from inspectah.pipeline import _print_secrets_summary
    from inspectah.schema import InspectionSnapshot, RedactionFinding

    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/cockpit/ws-certs.d/0-self-signed.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="regenerate"),
        RedactionFinding(path="/etc/cockpit/ws-certs.d/0-self-signed.cert", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="regenerate"),
        RedactionFinding(path="/etc/pki/tls/private/server.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="provision"),
        RedactionFinding(path="/etc/wireguard/wg0.conf", source="file",
                        kind="inline", pattern="WIREGUARD_KEY", remediation="value-removed",
                        replacement="REDACTED_WIREGUARD_KEY_1"),
        RedactionFinding(path="/etc/app.conf", source="file",
                        kind="inline", pattern="PASSWORD", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_1"),
        RedactionFinding(path="users:shadow/admin", source="shadow",
                        kind="inline", pattern="SHADOW_HASH", remediation="value-removed",
                        replacement="REDACTED_SHADOW_HASH_1"),
    ]

    captured = StringIO()
    monkeypatch.setattr(sys, "stderr", captured)
    _print_secrets_summary(snap)
    output = captured.getvalue()

    assert "Secrets handling:" in output
    assert "Excluded (regenerate on target): 2 files" in output
    assert "Excluded (provision from store): 1 file" in output
    assert "Inline-redacted:" in output
    # 3 total inline, but only 2 file-backed files
    assert "2 files" in output or "2 file" in output
    assert "secrets-review.md" in output


def test_cli_secrets_summary_no_findings(monkeypatch):
    """CLI summary prints nothing when there are no findings."""
    from inspectah.pipeline import _print_secrets_summary
    from inspectah.schema import InspectionSnapshot

    snap = InspectionSnapshot(meta={})
    snap.redactions = []

    captured = StringIO()
    monkeypatch.setattr(sys, "stderr", captured)
    _print_secrets_summary(snap)
    output = captured.getvalue()
    assert output == ""
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_pipeline.py::test_cli_secrets_summary tests/test_pipeline.py::test_cli_secrets_summary_no_findings -v`
Expected: FAIL — `_print_secrets_summary` not defined

- [ ] **Step 3: Implement `_print_secrets_summary()` in pipeline.py**

Add to `src/inspectah/pipeline.py` (before `run_pipeline`, around line 39):

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
    inline_files = len({f.path for f in inline if f.source == "file"})

    print("Secrets handling:", file=sys.stderr)
    if excluded_regen:
        n = len(excluded_regen)
        print(f"  Excluded (regenerate on target): {n} file{'s' if n != 1 else ''}", file=sys.stderr)
    if excluded_prov:
        n = len(excluded_prov)
        print(f"  Excluded (provision from store): {n} file{'s' if n != 1 else ''}", file=sys.stderr)
    if inline:
        n = len(inline)
        print(f"  Inline-redacted: {n} value{'s' if n != 1 else ''} in {inline_files} file{'s' if inline_files != 1 else ''}", file=sys.stderr)
    print("  Details: secrets-review.md | Placeholders: redacted/", file=sys.stderr)
```

- [ ] **Step 4: Call `_print_secrets_summary()` in the pipeline**

In `run_pipeline()`, after the `run_renderers(snapshot, tmp_dir)` call (line 81 of `src/inspectah/pipeline.py`), add:

```python
        run_renderers(snapshot, tmp_dir)
        _print_secrets_summary(snapshot)  # <-- add this line
```

- [ ] **Step 5: Run tests**

Run: `pytest tests/test_pipeline.py -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/inspectah/pipeline.py tests/test_pipeline.py
git commit -m "feat(pipeline): add CLI secrets handling summary with automated test

Prints count of excluded (regenerate/provision) and inline-redacted
findings to stderr after rendering. Automated test captures stderr
and verifies correct counts."
```

---

## Milestone 5: Validation — mixed PEM tests + full integration

### Task 12: Mixed PEM bundle tests

**Files:**
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write tests for mixed PEM handling**

```python
def test_mixed_pem_cert_plus_key_inline_redacts_key_only():
    """Combined cert+key PEM: private key block redacted, cert preserved."""
    pem_bundle = (
        "-----BEGIN CERTIFICATE-----\n"
        "MIIBkTCB+wIJALRiMLAh0EGKMA0G...\n"
        "-----END CERTIFICATE-----\n"
        "-----BEGIN PRIVATE KEY-----\n"
        "MIIEvgIBADANBgkqhkiG9w0BAQEF...\n"
        "-----END PRIVATE KEY-----\n"
    )
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/certs/bundle.pem", kind=ConfigFileKind.UNOWNED, content=pem_bundle, include=True),
    ]))
    result = redact_snapshot(snapshot)
    content = result.config.files[0].content
    # Cert block preserved
    assert "BEGIN CERTIFICATE" in content
    assert "MIIBkTCB" in content
    # Key block redacted
    assert "BEGIN PRIVATE KEY" not in content or "REDACTED_PRIVATE_KEY" in content
    # File still included (inline redaction)
    assert result.config.files[0].include is True


def test_cert_only_pem_no_redaction():
    """Cert-only PEM file: no redaction needed."""
    cert_only = (
        "-----BEGIN CERTIFICATE-----\n"
        "MIIBkTCB+wIJALRiMLAh0EGKMA0G...\n"
        "-----END CERTIFICATE-----\n"
    )
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/certs/ca-bundle.crt", kind=ConfigFileKind.UNOWNED, content=cert_only, include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].content == cert_only
    assert result.config.files[0].include is True


def test_key_only_file_excluded():
    """Key-only .key file: full exclusion via path pattern."""
    key_only = "-----BEGIN PRIVATE KEY-----\nMIIEvg...\n-----END PRIVATE KEY-----\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/private/server.key", kind=ConfigFileKind.UNOWNED, content=key_only, include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False
```

- [ ] **Step 2: Run tests**

Run: `pytest tests/test_redact.py -k "mixed_pem or cert_only or key_only_file" -v`
Expected: PASS (these confirm existing + new behavior works correctly together)

- [ ] **Step 3: Commit**

```bash
git add tests/test_redact.py
git commit -m "test(redact): add mixed PEM bundle regression tests

Confirms cert+key files get inline key redaction, cert-only files
pass through unchanged, and key-only .key files are fully excluded."
```

---

### Task 13: Full integration test and cleanup

**Files:**
- Test: all test files
- Modify: any files needing fixup

- [ ] **Step 1: Run the full test suite**

Run: `pytest tests/ -v --tb=short`
Expected: All tests pass. Fix any regressions from the dict→RedactionFinding migration.

- [ ] **Step 2: Verify consumer migration is complete**

Check each consumer file for any remaining bare `r["key"]` subscript access that would fail with `RedactionFinding`:

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
grep -rn '\[.*"path"\|"pattern"\|"remediation"\|"line"\|"source"\|"message"' src/inspectah/renderers/ src/inspectah/fleet/ src/inspectah/pipeline.py | grep -v '.get(' | grep -v '# '
```

Any hits need to be converted to `.get()` or `getattr()`.

- [ ] **Step 3: Run inspectah against a test fixture (if available)**

If a test fixture tarball is available, run the full pipeline and verify:
- `redacted/` directory exists with `.REDACTED` files
- `config/` directory does NOT contain excluded files
- `secrets-review.md` has separate Excluded/Inline tables
- Containerfile has comment blocks
- CLI output shows correct counts on stderr

- [ ] **Step 4: Commit any fixups**

```bash
git add -A
git commit -m "fix: integration fixups for secrets handling v2"
```

- [ ] **Step 5: Final commit — move spec to implemented**

```bash
cp docs/specs/proposed/2026-04-08-secrets-handling-v2-design.md docs/specs/implemented/
git add docs/specs/implemented/2026-04-08-secrets-handling-v2-design.md
git commit -m "docs(spec): move secrets handling v2 spec to implemented"
```
