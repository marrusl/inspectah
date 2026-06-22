# Subscription Preserve Flow

How RHEL entitlement certs flow from host to scan output, and how cert
expiry is parsed and displayed.

## Host filesystem sources

The `SubscriptionInspector` collects from these paths:

| Path | Contents | Required? |
|------|----------|-----------|
| `/etc/pki/entitlement/*.pem` | Entitlement cert+key pairs (matched by serial number) | Yes (at least one matched pair) |
| `/etc/rhsm/rhsm.conf` | RHSM client configuration | Yes |
| `/etc/rhsm/ca/*.pem` | CA certificates for CDN validation | Yes |
| `/etc/yum.repos.d/redhat.repo` | Repo definitions for RHEL content | Yes |
| `/etc/pki/consumer/cert.pem` | Consumer identity cert (metadata only, not collected) | No |

Missing any required item sets `section.incomplete = true` and emits a
warning. The consumer cert is used only to extract `org_id`,
`system_uuid`, and `rhsm_server` metadata.

## Pipeline: inspector -> types -> snapshot -> rendering

1. **Inspector** (`crates/collect/src/inspectors/subscription.rs`):
   `SubscriptionInspector::inspect()` collects PEM files, base64-encodes
   content into `SubscriptionFile` structs, parses X.509 cert expiry
   from entitlement certs, evaluates bundle completeness by serial-number
   matching, and returns `SectionData::Subscription(SubscriptionSection)`.

2. **Types** (`crates/core/src/types/subscription.rs`):
   - `SubscriptionFile` — path, base64 content, size, `cert_expiry: Option<time::OffsetDateTime>`
   - `SubscriptionSection` — vecs of files, `earliest_expiry: Option<time::OffsetDateTime>`,
     `incomplete: bool`, org metadata, source hostname
   - `EntitlementPair` — cert+key matched by serial number (not serialized)
   - Expiry serializes as RFC 3339 via `time::serde::rfc3339::option`

3. **Snapshot** (`crates/core/src/snapshot.rs`):
   - `subscription: Option<SubscriptionSection>` — the full section data
   - `preserved_subscription: bool` — flag set by scan CLI when `--preserve subscription`

4. **Rendering surfaces:**
   - CLI scan summary (`crates/cli/src/commands/scan.rs`): `build_sensitivity_notice()` calls
     `format_cert_expiry_line()` to show expiry date, days remaining, and warnings
   - README (`crates/pipeline/src/render/readme.rs`): subscription build instructions section
     includes a blockquote with expiry date and warning level
   - Secrets review (`crates/pipeline/src/render/secrets.rs`): documents mount commands
     (does not currently show expiry)

## Cert expiry parsing

**Crate:** `x509-parser` 0.16 (already in `crates/collect/Cargo.toml`)

**Flow:** For each entitlement PEM that is not a key file (`-key.pem`):
1. Base64-decode the stored content back to raw PEM text
2. Extract DER bytes from PEM markers (`pem_to_der()` helper)
3. Parse X.509 with `x509_parser::parse_x509_certificate()`
4. Read `validity().not_after.timestamp()` -> epoch seconds
5. Convert to `time::OffsetDateTime::from_unix_timestamp()`
6. Set `cert_file.cert_expiry = Some(expiry)`
7. Track the earliest expiry across all certs -> `section.earliest_expiry`

**Why earliest:** Multiple entitlement certs can exist (different serials
for different content sets). The earliest expiry is the binding constraint
because *any* expired cert breaks the builds that need that content set.

## Display thresholds

| Condition | CLI output | README output |
|-----------|-----------|---------------|
| > 7 days remaining | `Subscription certs expire: YYYY-MM-DD (N days)` | Informational blockquote |
| 1-6 days remaining | `⚠ ... expire: ... — rebuild soon` | WARNING blockquote |
| Already expired | `⚠ ... EXPIRED: ... — will not work on unregistered systems` | WARNING + "Re-scan with fresh certs" |
| No expiry available | Line omitted | Section omitted |

## Gotchas

- **Key files skipped:** Files ending in `-key.pem` are private keys, not
  certificates. The parser skips them (they have no `Not After` field).

- **Malformed PEM gracefully skipped:** If a `.pem` file can't be decoded
  or parsed as X.509, parsing continues silently. The cert just won't have
  an expiry. This is intentional — broken certs shouldn't block the scan.

- **Symlink boundary checks:** All collected files go through
  `is_symlink_safe()` which follows the full symlink chain and rejects
  paths that resolve outside `/etc/pki/entitlement`, `/etc/rhsm`, or
  `/etc/yum.repos.d/redhat.repo`.

- **`time` crate, not `chrono`:** The codebase uses `time` 0.3 for
  datetime handling in subscription types. The `chrono` crate exists in
  some Cargo.tomls but is not used for subscription expiry. Don't mix them.

- **CLI crate needs `time` dependency:** The `format_cert_expiry_line()`
  function in scan.rs uses `time::format_description` and
  `time::OffsetDateTime::now_utc()`. The CLI crate has `time` in its
  Cargo.toml with the `formatting` feature.
