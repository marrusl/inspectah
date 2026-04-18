# Detection Quick Wins

**Date:** 2026-03-16
**Priority:** P1/P2 mix — all very low effort
**Status:** Proposed
**Source:** Gap audit items #7, A, E, I, C, #8

---

## Problem

Six detection gaps identified in the gap audit where the data either already exists or is trivially capturable, but inspectah doesn't flag, classify, or surface it. Each is a very low effort fix — classification logic, group-by queries on existing data, single-command captures, or one-line enum additions.

---

## Items

### 1. Mixed 32/64-bit Packages (Gap #7)

**Problem:** Both `foo.x86_64` and `foo.i686` can be installed simultaneously. 32-bit packages may not be available in base image repos. inspectah captures `arch` per `PackageEntry` but never flags multi-arch coexistence.

**Detection:** Run on the **full installed package list** (all `PackageEntry` items from `rpm -qa`, before baseline diffing into `packages_added`/`base_image_only`). Group by `name`. Flag any name where both `.x86_64` and `.i686` (or other multi-arch pairs like `.s390x`/`.ppc64le`) exist. This must run before baseline filtering — if `foo.x86_64` is in the base image but `foo.i686` is not, the multiarch pair is still a migration concern.

**Schema:** New field on `RpmSection`:

```python
multiarch_packages: List[str] = Field(default_factory=list)
# Package names that have both 32-bit and 64-bit variants installed
```

**Report:** Warning in HTML report packages tab — yellow alert banner listing affected package names. Audit report: `"- Multi-arch: N packages have both 32-bit and 64-bit variants installed"`. Add to `snapshot.warnings`.

**Containerfile:** FIXME comment per affected 32-bit package noting it may not be available in base image repos.

### 2. Duplicate Packages (Gap A)

**Problem:** Same `name.arch` with multiple versions installed simultaneously (e.g., `foo-1.0.x86_64` and `foo-2.0.x86_64`). Rare but causes silent breakage — `dnf install foo` picks latest, ignoring the older version something may depend on.

**Detection:** Run on the **raw `rpm -qa` output** before deduplication. Standard `rpm -qa` returns one line per installed package instance, so if two versions of the same `name.arch` are installed, both appear. Group by `name.arch`, flag where count > 1. This must run before any deduplication in the RPM inspector — check whether the inspector already deduplicates during collection and ensure duplicates are preserved long enough for this check.

**Schema:** New field on `RpmSection`:

```python
duplicate_packages: List[str] = Field(default_factory=list)
# name.arch keys that have multiple versions installed
```

**Report:** Warning in packages tab — yellow alert. Audit report: `"- Duplicates: N packages have multiple versions installed"`. Add to `snapshot.warnings`.

**Containerfile:** FIXME comment per duplicate listing the installed versions.

### 3. System-wide Crypto Policy (Gap E)

**Problem:** inspectah does not capture the active crypto policy. If the host runs `LEGACY` (common for talking to old TLS endpoints) and the base image enforces `DEFAULT`, TLS connections break silently after bootc switch. Custom subpolicies are also not captured.

**Detection:** Two sources:
1. Read `/host/etc/crypto-policies/config` — single line containing the policy name (e.g., `LEGACY`, `DEFAULT`, `FUTURE`, `FIPS`, or a custom name)
2. Scan `/host/etc/crypto-policies/policies/` for custom policy files (`.pol` extension)

No executor command needed — these are simple file reads. The selinux inspector already detects FIPS mode via `/proc/sys/crypto/fips_enabled`; crypto policy is the broader setting.

**Where:** New method in `config.py` (it's a system configuration concern), or in `kernel_boot.py` (it affects system-wide behavior). Recommend `config.py` since it follows the "detect and classify config files" pattern.

**Schema:** New fields on `ConfigSection` (or a new top-level section if `ConfigSection` doesn't exist — check existing structure):

```python
crypto_policy: str = ""           # e.g., "LEGACY", "DEFAULT", "FIPS"
crypto_policy_custom: bool = False  # True if custom .pol files exist
```

If there's no dedicated config section on the snapshot, add these to whichever section currently holds system-level config state (likely needs a `SystemConfigSection` or similar — check the schema).

Actually, simpler approach: capture crypto policy as a `ConfigFileEntry` with a new `ConfigCategory.CRYPTO_POLICY` category. The file `/etc/crypto-policies/config` is RPM-owned and will show up in `rpm -Va` if modified. The category label ensures it's surfaced prominently. Custom `.pol` files under `/etc/crypto-policies/policies/` will be caught as unowned config files.

**Schema:** Add `CRYPTO_POLICY = "crypto_policy"` to `ConfigCategory` enum.

**Report:** If the policy is not `DEFAULT`, render a warning in the config tab: "System crypto policy is set to X — base image may use DEFAULT." Add to `snapshot.warnings` if policy is `LEGACY` or `FIPS` (the two most likely to cause breakage).

**Containerfile:** If crypto policy differs from default, emit **after** the `dnf install` block (the `crypto-policies-scripts` package provides `update-crypto-policies` and is part of the standard RHEL base image, but must be present):
```dockerfile
# System crypto policy: LEGACY
RUN update-crypto-policies --set LEGACY
```
If custom `.pol` files exist under `/etc/crypto-policies/policies/`, emit a COPY for that directory before the `update-crypto-policies` command.

### 4. nsswitch.conf (Gap I)

**Problem:** `/etc/nsswitch.conf` defines name resolution order (passwd, group, hosts, etc.). A host with `passwd: files sss` (SSSD-integrated) and a base image with `passwd: files` means domain users silently can't log in. inspectah doesn't detect this file at all (it only appears in `redact.py` as a false-positive filter).

**Detection:** `/etc/nsswitch.conf` is RPM-owned. If modified, `rpm -Va` will flag it and the config inspector will capture it as `RPM_OWNED_MODIFIED`. The gap is classification — it currently gets `ConfigCategory.OTHER`.

**Schema:** Add `IDENTITY = "identity"` to `ConfigCategory` enum. Assign to paths matching `/etc/nsswitch.conf`, `/etc/sssd/`, `/etc/krb5.conf`, `/etc/krb5.conf.d/`, `/etc/ipa/`.

**Report:** Files classified as `IDENTITY` render with the identity category label in the config tab. No special warning needed beyond the existing modified-config surfacing — the category label makes it visible.

**Note:** This also covers Gap C (SSSD/Kerberos/IPA classification) — same category, different path patterns.

### 5. SSSD/Kerberos/IPA Identity Classification (Gap C)

**Problem:** Config files under `/etc/sssd/`, `/etc/krb5.conf`, `/etc/ipa/` are caught by the config inspector if modified or unowned, but classified as `OTHER`. An operator reviewing the report may not realize these are critical identity-provider configs.

**Detection:** Already captured. Just needs classification.

**Schema:** Covered by the `IDENTITY` category addition in item 4 above.

**Path rules to add to `_CATEGORY_RULES` in `config.py`:**

```python
("/etc/nsswitch.conf", ConfigCategory.IDENTITY),
("/etc/sssd/", ConfigCategory.IDENTITY),
("/etc/krb5.conf", ConfigCategory.IDENTITY),
("/etc/krb5.conf.d/", ConfigCategory.IDENTITY),
("/etc/ipa/", ConfigCategory.IDENTITY),
```

### 6. limits.conf Category (Gap #8)

**Problem:** `/etc/security/limits.conf` and `/etc/security/limits.d/*.conf` are caught as config files if modified or unowned, but classified as `OTHER`.

**Detection:** Already captured. Just needs classification.

**Schema:** Add `LIMITS = "limits"` to `ConfigCategory` enum.

**Path rule:**

```python
("/etc/security/limits", ConfigCategory.LIMITS),
```

This prefix matches both `limits.conf` and `limits.d/` contents.

---

## Summary of Changes

### ConfigCategory enum additions (schema.py)

```python
CRYPTO_POLICY = "crypto_policy"
IDENTITY = "identity"
LIMITS = "limits"
```

### _CATEGORY_RULES additions (config.py)

```python
("/etc/crypto-policies/", ConfigCategory.CRYPTO_POLICY),
("/etc/nsswitch.conf", ConfigCategory.IDENTITY),
("/etc/sssd/", ConfigCategory.IDENTITY),
("/etc/krb5.conf", ConfigCategory.IDENTITY),
("/etc/krb5.conf.d/", ConfigCategory.IDENTITY),
("/etc/ipa/", ConfigCategory.IDENTITY),
("/etc/security/limits", ConfigCategory.LIMITS),
```

### RpmSection additions (schema.py)

```python
multiarch_packages: List[str] = Field(default_factory=list)
duplicate_packages: List[str] = Field(default_factory=list)
```

### RPM inspector additions (rpm.py)

Two post-processing methods after the installed package list is built:
- `_detect_multiarch()` — group by name, flag multi-arch
- `_detect_duplicates()` — group by name.arch, flag count > 1

### Config inspector additions (config.py)

- Ensure `/etc/crypto-policies/config` is captured (verify it's RPM-owned and appears in `rpm -Va` when modified)
- If crypto policy is not `DEFAULT`, add warning to `snapshot.warnings`
- Containerfile: emit `RUN update-crypto-policies --set X` when non-default

### Containerfile renderer (packages.py)

- FIXME comments for multiarch 32-bit packages
- FIXME comments for duplicate packages listing versions

### HTML report

- Yellow warning banners for multiarch and duplicate packages (in packages tab)
- Category labels for CRYPTO_POLICY, IDENTITY, LIMITS render in config tab (existing label rendering handles new categories automatically if using the enum value as the label)

### Audit report

- Summary lines for multiarch count, duplicate count
- Warnings for non-default crypto policy

### SCHEMA_VERSION

All changes are additive with defaults. If the P0 spec has already been implemented and bumped the version to 9.1, these changes can ride on 9.1. If not, bump to 9.1 here.

### _CATEGORY_RULES ordering

New rules can be appended to the end of the `_CATEGORY_RULES` list. Matching is first-match-wins, but the new paths (`/etc/crypto-policies/`, `/etc/nsswitch.conf`, `/etc/sssd/`, etc.) do not overlap with existing rules, so ordering does not matter.

---

## Testing

### driftify additions

- Modify `/etc/nsswitch.conf` to add `sss` to passwd line (may already be done by a driftify profile)
- Create a custom crypto policy: `update-crypto-policies --set LEGACY` (or touch a `.pol` file)
- Install a 32-bit package alongside its 64-bit counterpart: `dnf install glibc.i686` (glibc.x86_64 is always present)
- Create a limits.d file: `echo "elasticsearch - nofile 65535" > /etc/security/limits.d/elasticsearch.conf`

### Unit tests

- `_detect_multiarch()`: packages with same name different arch → flagged; same arch → not flagged; single package → not flagged
- `_detect_duplicates()`: same name.arch multiple versions → flagged; different names → not flagged
- `classify_config_path()`: verify all new path rules map to correct categories
- Containerfile: FIXME comments for multiarch and duplicate packages
- Crypto policy warning: LEGACY → warning, DEFAULT → no warning

---

## Out of Scope

- Structured parsing of nsswitch.conf, limits.conf, krb5.conf — file content is already captured; parsing is a future enhancement
- Sysctl smart flagging (separate spec per future-inspection-coverage.md)
- PAM stack parsing (separate spec, medium-high effort)
