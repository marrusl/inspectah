# Secrets Handling v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close detection gaps, make redacted files obvious with remediation-specific guidance, and replace hash-based tokens with sequential counters.

**Architecture:** Extend the existing `redact_snapshot()` pipeline with new patterns, a typed `RedactionFinding` model, and remediation state tracking. Add a `redacted/` output directory for excluded files. Update all downstream renderers (secrets-review.md, Containerfile comments, CLI output) to use the new model.

**Tech Stack:** Python 3.10+, dataclasses, pytest, existing yoinkc schema/renderer infrastructure.

**Spec:** `docs/specs/proposed/2026-04-08-secrets-handling-v2-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/yoinkc/schema.py` | Modify | Add `RedactionFinding` dataclass, update `InspectionSnapshot.redactions` type |
| `src/yoinkc/redact.py` | Modify | New patterns, sequential counters, remediation states, `include=False` for excluded |
| `src/yoinkc/renderers/containerfile/_config_tree.py` | Modify | Write `redacted/` directory with `.REDACTED` files |
| `src/yoinkc/renderers/containerfile/_core.py` | Modify | Add secrets comment blocks to Containerfile |
| `src/yoinkc/renderers/secrets_review.py` | Modify | New format with separate Excluded/Inline tables |
| `src/yoinkc/pipeline.py` | Modify | CLI output summary after rendering |
| `tests/test_redact.py` | Modify | Detection gap tests, counter tests, remediation state tests |
| `tests/test_secrets_review.py` | Create | Renderer output tests for new format |
| `tests/test_redacted_dir.py` | Create | `.REDACTED` file placement and content tests |
| `tests/test_containerfile_secrets_comments.py` | Create | Containerfile comment block tests |

---

### Task 1: Add RedactionFinding dataclass to schema

**Files:**
- Modify: `src/yoinkc/schema.py`
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write the failing test**

```python
# Add to tests/test_redact.py at the top imports
from yoinkc.schema import RedactionFinding

def test_redaction_finding_dataclass():
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pytest tests/test_redact.py::test_redaction_finding_dataclass -v`
Expected: FAIL — `ImportError: cannot import name 'RedactionFinding'`

- [ ] **Step 3: Add RedactionFinding to schema.py**

Add after the existing dataclass imports in `src/yoinkc/schema.py`:

```python
@dataclass
class RedactionFinding:
    """A single redaction event — drives all downstream output."""
    path: str              # Original filesystem path or synthetic identifier
    source: str            # "file" | "shadow" | "container-env" | "timer-cmd" | "diff"
    kind: str              # "excluded" or "inline"
    pattern: str           # Pattern name that matched
    remediation: str       # "regenerate" | "provision" | "value-removed"
    line: int | None = None       # Line number (inline only, file-backed only)
    replacement: str | None = None  # Replacement token (inline only)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pytest tests/test_redact.py::test_redaction_finding_dataclass -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/schema.py tests/test_redact.py
git commit -m "feat(schema): add RedactionFinding dataclass for typed redaction model"
```

---

### Task 2: Add new detection patterns

**Files:**
- Modify: `src/yoinkc/redact.py`
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write failing tests for new EXCLUDED_PATHS**

```python
# Add to tests/test_redact.py

def test_p12_keystore_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/private/server.p12", content="binary-data", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False

def test_pfx_keystore_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/private/server.pfx", content="binary-data", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False

def test_jks_keystore_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/java/cacerts.jks", content="binary-data", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False

def test_cockpit_ws_certs_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed.key", content="key-data", include=True),
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed.cert", content="cert-data", include=True),
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed-ca.pem", content="ca-data", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert all(f.include is False for f in result.config.files)

def test_containers_auth_json_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/containers/auth.json", content='{"auths":{}}', include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False
```

- [ ] **Step 2: Write failing tests for new REDACT_PATTERNS**

```python
def test_wireguard_private_key_redacted():
    wg_config = "[Interface]\nAddress = 10.0.0.1/24\nPrivateKey = aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4yZ5AB=\nListenPort = 51820\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/wireguard/wg0.conf", content=wg_config, include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert "aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4yZ5AB=" not in result.config.files[0].content
    assert "REDACTED_WIREGUARD_KEY_" in result.config.files[0].content
    # File should still be included (inline redaction, not exclusion)
    assert result.config.files[0].include is True

def test_wifi_psk_redacted():
    nm_config = "[wifi-security]\nkey-mgmt=wpa-psk\npsk=mysecretpassword123\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/NetworkManager/system-connections/wifi.nmconnection", content=nm_config, include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert "mysecretpassword123" not in result.config.files[0].content
    assert "REDACTED_WIFI_PSK_" in result.config.files[0].content
    assert result.config.files[0].include is True
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `pytest tests/test_redact.py -k "p12 or pfx or jks or cockpit_ws or containers_auth or wireguard or wifi_psk" -v`
Expected: FAIL — new patterns not yet added

- [ ] **Step 4: Add new patterns to redact.py**

Add to `EXCLUDED_PATHS` tuple (after line 26):

```python
EXCLUDED_PATHS = (
    # existing entries...
    r"/etc/shadow",
    r"/etc/gshadow",
    r"/etc/ssh/ssh_host_.*",
    r"/etc/pki/.*\.key",
    r".*\.key$",
    r".*keytab$",
    # new entries
    r".*\.p12$",
    r".*\.pfx$",
    r".*\.jks$",
    r"/etc/cockpit/ws-certs\.d/.*",
    r"/etc/containers/auth\.json",
)
```

Add to `REDACT_PATTERNS` list (after line 45):

```python
    # WireGuard private key (bare base64, not PEM-wrapped)
    (re.compile(r"PrivateKey\s*=\s*[A-Za-z0-9+/]{43}="), "WIREGUARD_KEY"),
    # WiFi PSK in NetworkManager connections
    (re.compile(r"psk\s*=\s*\S+"), "WIFI_PSK"),
```

- [ ] **Step 5: Update redact_snapshot() to set include=False for excluded paths**

In the config files loop of `redact_snapshot()`, where `_is_excluded_path()` matches, add:

```python
entry.include = False
```

after the content replacement line.

- [ ] **Step 6: Run tests to verify they pass**

Run: `pytest tests/test_redact.py -k "p12 or pfx or jks or cockpit_ws or containers_auth or wireguard or wifi_psk" -v`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/redact.py tests/test_redact.py
git commit -m "feat(redact): add detection for keystores, cockpit certs, auth.json, WireGuard, WiFi PSK

New EXCLUDED_PATHS: .p12, .pfx, .jks, cockpit ws-certs.d/*, containers/auth.json
New REDACT_PATTERNS: WIREGUARD_KEY (bare base64), WIFI_PSK
Excluded paths now set include=False on the config entry."
```

---

### Task 3: Replace hash tokens with sequential counters

**Files:**
- Modify: `src/yoinkc/redact.py`
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write failing tests for sequential counters**

```python
def test_sequential_counters_deterministic():
    """Same input produces same counter assignments."""
    content_a = "password=secret1\napi_key=abcdefghijklmnopqrstuvwxyz\n"
    content_b = "password=secret2\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app/a.conf", content=content_a, include=True),
        ConfigFileEntry(path="/etc/app/b.conf", content=content_b, include=True),
    ]))
    r1 = redact_snapshot(snapshot)
    r2 = redact_snapshot(_base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app/a.conf", content=content_a, include=True),
        ConfigFileEntry(path="/etc/app/b.conf", content=content_b, include=True),
    ])))
    assert r1.config.files[0].content == r2.config.files[0].content
    assert r1.config.files[1].content == r2.config.files[1].content

def test_sequential_counters_no_hash():
    """Counter tokens must not contain hash fragments."""
    content = "password=mysecret\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app.conf", content=content, include=True),
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
        ConfigFileEntry(path="/etc/app/a.conf", content=content_a, include=True),
        ConfigFileEntry(path="/etc/app/b.conf", content=content_b, include=True),
    ]))
    result = redact_snapshot(snapshot)
    token_a = re.search(r"REDACTED_PASSWORD_\d+", result.config.files[0].content).group()
    token_b = re.search(r"REDACTED_PASSWORD_\d+", result.config.files[1].content).group()
    assert token_a == token_b
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_redact.py -k "sequential_counters or same_secret" -v`
Expected: FAIL — still using hash tokens

- [ ] **Step 3: Implement sequential counters in _redact_text()**

Replace the `_truncated_sha256()` call in `_redact_text()` with a counter-based approach. Add a counter registry as a parameter or closure:

```python
def _make_counter_registry():
    """Returns a function that maps (type_label, secret_value) -> counter token."""
    _counters: dict[str, int] = {}  # type_label -> next counter
    _seen: dict[tuple[str, str], str] = {}  # (type_label, value) -> assigned token

    def get_token(type_label: str, value: str) -> str:
        key = (type_label, value)
        if key in _seen:
            return _seen[key]
        n = _counters.get(type_label, 0) + 1
        _counters[type_label] = n
        token = f"REDACTED_{type_label}_{n}"
        _seen[key] = token
        return token

    return get_token
```

Pass this registry into `_redact_text()` and use it instead of `_truncated_sha256()`. The registry must be created once per `redact_snapshot()` call and shared across all files.

For deterministic ordering, process files sorted by path. Within each file, process lines in order.

- [ ] **Step 4: Run tests to verify they pass**

Run: `pytest tests/test_redact.py -k "sequential_counters or same_secret or no_hash" -v`
Expected: PASS

- [ ] **Step 5: Run full test suite to check for regressions**

Run: `pytest tests/test_redact.py -v`
Expected: Existing tests may need token format updates (hash → counter). Fix any that assert specific hash values.

- [ ] **Step 6: Commit**

```bash
git add src/yoinkc/redact.py tests/test_redact.py
git commit -m "feat(redact): replace hash tokens with sequential counters

Eliminates dictionary oracle risk for weak secrets. Same secret value
gets the same counter across files. Deterministic ordering by file path
then line number."
```

---

### Task 4: Add remediation state to redaction findings

**Files:**
- Modify: `src/yoinkc/redact.py`
- Test: `tests/test_redact.py`

- [ ] **Step 1: Write failing tests for remediation states**

```python
from yoinkc.schema import RedactionFinding

def test_cockpit_gets_regenerate_remediation():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed.key", content="key", include=True),
    ]))
    result = redact_snapshot(snapshot)
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    cockpit = [f for f in findings if "cockpit" in f.path]
    assert len(cockpit) >= 1
    assert all(f.remediation == "regenerate" for f in cockpit)

def test_ssh_host_key_gets_regenerate_remediation():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/ssh/ssh_host_rsa_key", content="key", include=True),
    ]))
    result = redact_snapshot(snapshot)
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    ssh = [f for f in findings if "ssh_host" in f.path]
    assert len(ssh) >= 1
    assert all(f.remediation == "regenerate" for f in ssh)

def test_tls_key_gets_provision_remediation():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/private/server.key", content="key", include=True),
    ]))
    result = redact_snapshot(snapshot)
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    tls = [f for f in findings if "server.key" in f.path]
    assert len(tls) >= 1
    assert all(f.remediation == "provision" for f in tls)

def test_inline_redaction_gets_value_removed_remediation():
    wg_config = "[Interface]\nPrivateKey = aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4yZ5AB=\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/wireguard/wg0.conf", content=wg_config, include=True),
    ]))
    result = redact_snapshot(snapshot)
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    wg = [f for f in findings if "wireguard" in f.path]
    assert len(wg) >= 1
    assert all(f.remediation == "value-removed" for f in wg)

def test_non_file_finding_has_source():
    snapshot = _base_snapshot(users_groups=UserGroupSection(
        shadow_entries=[ShadowEntry(username="testuser", hash="$6$salt$hash", include=True)],
    ))
    result = redact_snapshot(snapshot)
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    shadow = [f for f in findings if f.source == "shadow"]
    assert len(shadow) >= 1
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_redact.py -k "regenerate or provision_remediation or value_removed or non_file_finding" -v`
Expected: FAIL — redactions are still plain dicts

- [ ] **Step 3: Add remediation mapping to redact.py**

Add a remediation mapping for excluded paths:

```python
# Pattern → remediation state for excluded paths
_EXCLUDED_REMEDIATION: dict[str, str] = {
    r"/etc/cockpit/ws-certs\.d/.*": "regenerate",
    r"/etc/ssh/ssh_host_.*": "regenerate",
    # All others default to "provision"
}

def _remediation_for_excluded(path: str) -> str:
    """Return remediation state for an excluded path."""
    for pattern, remediation in _EXCLUDED_REMEDIATION.items():
        if re.match(pattern, path):
            return remediation
    return "provision"
```

- [ ] **Step 4: Migrate redact_snapshot() to emit RedactionFinding objects**

Replace all `redactions.append({...})` calls with `RedactionFinding(...)` construction. For each redaction site:

- **Excluded paths:** `source="file"`, `kind="excluded"`, `remediation=_remediation_for_excluded(path)`
- **Inline text redactions:** `source="file"`, `kind="inline"`, `remediation="value-removed"`, `line=<line_number>`, `replacement=<token>`
- **Shadow entries:** `source="shadow"`, `kind="inline"`, `remediation="value-removed"`
- **Container env vars:** `source="container-env"`, `kind="inline"`, `remediation="value-removed"`
- **Timer commands:** `source="timer-cmd"`, `kind="inline"`, `remediation="value-removed"`
- **Diff views:** `source="diff"`, `kind="inline"`, `remediation="value-removed"`

- [ ] **Step 5: Run tests to verify they pass**

Run: `pytest tests/test_redact.py -v`
Expected: PASS (existing tests may need updates to check RedactionFinding attrs instead of dict keys)

- [ ] **Step 6: Fix any broken existing tests**

Update existing tests that check `snapshot.redactions` as dicts to use `RedactionFinding` attribute access instead. For example:
- `r["pattern"]` → `r.pattern`
- `r["path"]` → `r.path`

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/redact.py src/yoinkc/schema.py tests/test_redact.py
git commit -m "feat(redact): add remediation states and typed RedactionFinding model

Each finding carries source provenance, remediation state, and kind.
Excluded paths map to regenerate or provision. Inline redactions map
to value-removed. Non-file-backed findings (shadow, container-env,
timer-cmd, diff) carry their source type."
```

---

### Task 5: Write `redacted/` directory with `.REDACTED` files

**Files:**
- Modify: `src/yoinkc/renderers/containerfile/_config_tree.py`
- Test: `tests/test_redacted_dir.py` (create)

- [ ] **Step 1: Write failing tests**

```python
# tests/test_redacted_dir.py
import tempfile
from pathlib import Path
from yoinkc.schema import InspectionSnapshot, ConfigSection, ConfigFileEntry, RedactionFinding
from yoinkc.renderers.containerfile._config_tree import write_config_tree, write_redacted_dir

def _snapshot_with_excluded():
    snap = InspectionSnapshot(meta={})
    snap.config = ConfigSection(files=[
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed.key", content="placeholder", include=False),
        ConfigFileEntry(path="/etc/pki/tls/private/server.key", content="placeholder", include=False),
        ConfigFileEntry(path="/etc/app.conf", content="normal config", include=True),
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
        assert not (out / "config" / "etc" / "cockpit").exists()
        assert (out / "config" / "etc" / "app.conf").exists()

def test_regenerate_placeholder_content():
    snap = _snapshot_with_excluded()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        write_redacted_dir(snap, out)
        content = (out / "redacted" / "etc" / "cockpit" / "ws-certs.d" / "0-self-signed.key.REDACTED").read_text()
        assert "REDACTED by yoinkc" in content
        assert "auto-generated credential" in content
        assert "no action needed" in content
        assert "/etc/cockpit/ws-certs.d/0-self-signed.key" in content

def test_provision_placeholder_content():
    snap = _snapshot_with_excluded()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        write_redacted_dir(snap, out)
        content = (out / "redacted" / "etc" / "pki" / "tls" / "private" / "server.key.REDACTED").read_text()
        assert "REDACTED by yoinkc" in content
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
        # redacted/ dir should not exist or be empty — shadow findings don't produce files
        redacted = out / "redacted"
        if redacted.exists():
            assert not any(redacted.rglob("*"))
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_redacted_dir.py -v`
Expected: FAIL — `write_redacted_dir` not defined

- [ ] **Step 3: Implement write_redacted_dir()**

Add to `src/yoinkc/renderers/containerfile/_config_tree.py`:

```python
_REGENERATE_TEMPLATE = """\
# REDACTED by yoinkc — auto-generated credential
# Original path: {path}
# Action: no action needed — this file is regenerated automatically on the target system
# See secrets-review.md for details
"""

_PROVISION_TEMPLATE = """\
# REDACTED by yoinkc — sensitive file detected
# Original path: {path}
# Action: provision this file on the target system from your secrets management process
# See secrets-review.md for details
"""

def write_redacted_dir(snapshot: InspectionSnapshot, output_dir: Path) -> None:
    """Write .REDACTED placeholder files for excluded secrets."""
    from ..schema import RedactionFinding
    redacted_dir = output_dir / "redacted"
    for finding in snapshot.redactions:
        if not isinstance(finding, RedactionFinding):
            continue
        if finding.source != "file" or finding.kind != "excluded":
            continue
        rel = finding.path.lstrip("/")
        if not rel:
            continue
        dest = redacted_dir / (rel + ".REDACTED")
        dest.parent.mkdir(parents=True, exist_ok=True)
        if finding.remediation == "regenerate":
            content = _REGENERATE_TEMPLATE.format(path=finding.path)
        else:
            content = _PROVISION_TEMPLATE.format(path=finding.path)
        dest.write_text(content)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pytest tests/test_redacted_dir.py -v`
Expected: PASS

- [ ] **Step 5: Wire write_redacted_dir into the renderer pipeline**

In the renderer orchestration (find where `write_config_tree` is called — likely in a renderers `__init__.py` or `pipeline.py`), add a call to `write_redacted_dir(snapshot, output_dir)` after `write_config_tree`.

- [ ] **Step 6: Commit**

```bash
git add src/yoinkc/renderers/containerfile/_config_tree.py tests/test_redacted_dir.py
git commit -m "feat(render): write redacted/ directory with .REDACTED placeholder files

Excluded secrets go to redacted/ (outside config/ COPY tree) with
remediation-specific placeholder content. Regenerate-on-target files
say 'no action needed'. Provision files say 'provision from secrets
management'. Non-file findings produce no .REDACTED files."
```

---

### Task 6: Update secrets-review.md renderer

**Files:**
- Modify: `src/yoinkc/renderers/secrets_review.py`
- Test: `tests/test_secrets_review.py` (create)

- [ ] **Step 1: Write failing tests**

```python
# tests/test_secrets_review.py
import tempfile
from pathlib import Path
from yoinkc.schema import InspectionSnapshot, RedactionFinding
from yoinkc.renderers.secrets_review import render

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
        render(snap, {}, Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "## Excluded Files" in content
        assert "Regenerate on target" in content
        assert "Provision from secret store" in content

def test_secrets_review_has_inline_table():
    snap = _snapshot_with_findings()
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, {}, Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "## Inline Redactions" in content
        assert "REDACTED_WIREGUARD_KEY_1" in content
        assert "Supply value at deploy time" in content

def test_secrets_review_separates_excluded_and_inline():
    snap = _snapshot_with_findings()
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, {}, Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        excluded_pos = content.index("## Excluded Files")
        inline_pos = content.index("## Inline Redactions")
        assert excluded_pos < inline_pos

def test_secrets_review_empty():
    snap = InspectionSnapshot(meta={})
    snap.redactions = []
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, {}, Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "No redactions recorded" in content
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_secrets_review.py -v`
Expected: FAIL — old format doesn't have separate tables

- [ ] **Step 3: Rewrite render() in secrets_review.py**

```python
def render(snapshot, env, output_dir):
    """Render secrets-review.md with separate Excluded/Inline tables."""
    from .schema import RedactionFinding
    path = output_dir / "secrets-review.md"

    if not snapshot.redactions:
        path.write_text("# Secrets Review\n\nNo redactions recorded.\n")
        return

    lines = [
        "# Secrets Review\n",
        "The following items were redacted or excluded. Handle them according to",
        "the action specified for each item.\n",
    ]

    # Separate findings by kind
    excluded = [r for r in snapshot.redactions
                if isinstance(r, RedactionFinding) and r.kind == "excluded"]
    inline = [r for r in snapshot.redactions
              if isinstance(r, RedactionFinding) and r.kind == "inline"]

    _REMEDIATION_LABELS = {
        "regenerate": "Regenerate on target",
        "provision": "Provision from secret store",
        "value-removed": "Supply value at deploy time",
    }

    if excluded:
        lines.append("## Excluded Files\n")
        lines.append("| Path | Action | Reason |")
        lines.append("|------|--------|--------|")
        for f in excluded:
            action = _REMEDIATION_LABELS.get(f.remediation, f.remediation)
            lines.append(f"| {f.path} | {action} | {f.pattern} |")
        lines.append("")

    if inline:
        lines.append("## Inline Redactions\n")
        lines.append("| Path | Line | Type | Placeholder | Action |")
        lines.append("|------|------|------|-------------|--------|")
        for f in inline:
            line_str = str(f.line) if f.line is not None else "—"
            replacement = f.replacement or "—"
            action = _REMEDIATION_LABELS.get(f.remediation, f.remediation)
            lines.append(f"| {f.path} | {line_str} | {f.pattern} | {replacement} | {action} |")
        lines.append("")

    path.write_text("\n".join(lines))
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pytest tests/test_secrets_review.py -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/renderers/secrets_review.py tests/test_secrets_review.py
git commit -m "feat(render): rewrite secrets-review.md with separate Excluded/Inline tables

Excluded files show Action (Regenerate/Provision) and Reason.
Inline redactions show line number, placeholder token, and action.
Non-file-backed findings appear in the Inline table with their source."
```

---

### Task 7: Add Containerfile secrets comment blocks

**Files:**
- Modify: `src/yoinkc/renderers/containerfile/_core.py`
- Test: `tests/test_containerfile_secrets_comments.py` (create)

- [ ] **Step 1: Write failing tests**

```python
# tests/test_containerfile_secrets_comments.py
from yoinkc.schema import RedactionFinding

def test_containerfile_has_excluded_comment_block(tmp_path):
    """Containerfile should list excluded secrets grouped by remediation."""
    # Build a snapshot, run the renderer, read the Containerfile
    # Assert it contains "Excluded secrets (not in this image)"
    # Assert it contains "Regenerate on target" and "Provision from secret store" subgroups
    pass  # Implement with actual renderer call

def test_containerfile_has_inline_comment_block(tmp_path):
    """Containerfile should list inline-redacted values."""
    pass  # Implement with actual renderer call

def test_containerfile_no_comments_when_no_redactions(tmp_path):
    """No comment blocks if no redactions."""
    pass  # Implement with actual renderer call
```

Note: These tests need the actual Containerfile renderer infrastructure. Read `_core.py` to understand how to set up the test (what `_render_containerfile_content()` needs as input), then write concrete tests against the actual function signature.

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest tests/test_containerfile_secrets_comments.py -v`
Expected: FAIL

- [ ] **Step 3: Add secrets comment block function to _core.py**

Add a helper function that generates comment lines from `snapshot.redactions`:

```python
def _secrets_comment_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Generate Containerfile comment blocks for redacted secrets."""
    from ..schema import RedactionFinding
    excluded = [r for r in snapshot.redactions
                if isinstance(r, RedactionFinding) and r.kind == "excluded" and r.source == "file"]
    inline = [r for r in snapshot.redactions
              if isinstance(r, RedactionFinding) and r.kind == "inline" and r.source == "file"]

    if not excluded and not inline:
        return []

    lines = []

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

Wire `_secrets_comment_lines(snapshot)` into `_render_containerfile_content()` — append the returned lines after the existing section lines.

- [ ] **Step 4: Run tests to verify they pass**

Run: `pytest tests/test_containerfile_secrets_comments.py -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/renderers/containerfile/_core.py tests/test_containerfile_secrets_comments.py
git commit -m "feat(render): add secrets comment blocks to Containerfile

Separate blocks for excluded (grouped by regenerate/provision) and
inline-redacted (listing path, pattern, placeholder). Only file-backed
findings appear. No comment blocks if no redactions."
```

---

### Task 8: Add CLI output summary

**Files:**
- Modify: `src/yoinkc/pipeline.py`

- [ ] **Step 1: Add CLI summary after rendering**

In `pipeline.py`, after the `run_renderers()` call (around line 77), add:

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

    import sys
    print("Secrets handling:", file=sys.stderr)
    if excluded_regen:
        print(f"  Excluded (regenerate on target): {len(excluded_regen)} files", file=sys.stderr)
    if excluded_prov:
        print(f"  Excluded (provision from store): {len(excluded_prov)} files", file=sys.stderr)
    if inline:
        print(f"  Inline-redacted: {len(inline)} values in {inline_files} files", file=sys.stderr)
    print("  Details: secrets-review.md | Placeholders: redacted/", file=sys.stderr)
```

Call `_print_secrets_summary(snapshot)` after `run_renderers(snapshot, tmp_dir)`.

- [ ] **Step 2: Test manually**

Run yoinkc against a test fixture and verify CLI output appears on stderr with correct counts.

- [ ] **Step 3: Commit**

```bash
git add src/yoinkc/pipeline.py
git commit -m "feat(pipeline): add CLI secrets handling summary

Prints count of excluded (regenerate/provision) and inline-redacted
findings to stderr after rendering."
```

---

### Task 9: Mixed PEM bundle tests

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
        ConfigFileEntry(path="/etc/pki/tls/certs/bundle.pem", content=pem_bundle, include=True),
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
        ConfigFileEntry(path="/etc/pki/tls/certs/ca-bundle.crt", content=cert_only, include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].content == cert_only
    assert result.config.files[0].include is True

def test_key_only_file_excluded():
    """Key-only .key file: full exclusion via path pattern."""
    key_only = "-----BEGIN PRIVATE KEY-----\nMIIEvg...\n-----END PRIVATE KEY-----\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/private/server.key", content=key_only, include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False
```

- [ ] **Step 2: Run tests**

Run: `pytest tests/test_redact.py -k "mixed_pem or cert_only or key_only_file" -v`
Expected: PASS (these should already work with existing patterns — this task confirms it)

- [ ] **Step 3: Commit**

```bash
git add tests/test_redact.py
git commit -m "test(redact): add mixed PEM bundle regression tests

Confirms cert+key files get inline key redaction, cert-only files
pass through unchanged, and key-only files are fully excluded."
```

---

### Task 10: Full integration test and cleanup

**Files:**
- Test: `tests/test_redact.py`
- Modify: any files needing fixup

- [ ] **Step 1: Run the full test suite**

Run: `pytest tests/ -v --tb=short`
Expected: All tests pass. Fix any regressions from the dict→RedactionFinding migration.

- [ ] **Step 2: Run yoinkc against the user's tarball fixture**

If a test fixture is available, run the full pipeline and verify:
- `redacted/` directory exists with `.REDACTED` files
- `config/` directory does NOT contain excluded files
- `secrets-review.md` has separate Excluded/Inline tables
- Containerfile has comment blocks
- CLI output shows correct counts

- [ ] **Step 3: Commit any fixups**

```bash
git add -A
git commit -m "fix: integration test fixups for secrets handling v2"
```

- [ ] **Step 4: Final commit — move spec to implemented**

```bash
cp docs/specs/proposed/2026-04-08-secrets-handling-v2-design.md docs/specs/implemented/
git add docs/specs/implemented/2026-04-08-secrets-handling-v2-design.md
git commit -m "docs(spec): move secrets handling v2 spec to implemented"
```
