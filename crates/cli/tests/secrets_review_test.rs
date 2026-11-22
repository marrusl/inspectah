//! `secrets-review.md` grouped per file, from collector-produced
//! snapshots: each file a heading with its redactions under it and one
//! line on how to supply the real value, the same file on every export
//! path, and no byte of any secret in it.

use crate::sysctl_hosts::{SysctlHost, reload, render_export, scan};
use inspectah_core::aggregate::merge_snapshots;

/// A config file with two secrets the image carries.
const APP_CONF: &str = "/etc/app/app.conf";
/// A second config file with two secrets.
const BACKUP_CONF: &str = "/etc/backup/creds.conf";
/// A sysctl file with one secret, read by the config and sysctl passes.
const SYSCTL_CONF: &str = "/etc/sysctl.d/80-app.conf";
/// A machine-state file whose content redaction also scrubs.
const IPA_CONF: &str = "/etc/ipa/default.conf";

/// Every secret value planted on the host.
const CANARIES: &[&str] = &[
    "canary-app-pw",
    "AKIACANARYAPPKEY0001",
    "canary-backup-1",
    "canary-backup-2",
    "canary-sysctl",
    "canary-ipa",
];

fn secret_host() -> SysctlHost {
    SysctlHost::new()
        .file(
            APP_CONF,
            "[db]\n  password = canary-app-pw\n# note\nAKIACANARYAPPKEY0001\n",
        )
        .file(
            BACKUP_CONF,
            "user = backup\npassword = canary-backup-1\nsecret = canary-backup-2\n",
        )
        .file(
            SYSCTL_CONF,
            "vm.swappiness = 10\ndb_password = canary-sysctl\n",
        )
        .file(
            IPA_CONF,
            "[global]\nrealm = EXAMPLE.COM\npassword = canary-ipa\n",
        )
        .sysctl_a("vm.swappiness = 60\n")
}

/// The `## `path`` section of `md`, up to the next heading of its level.
fn file_section<'a>(md: &'a str, path: &str) -> &'a str {
    let heading = format!("## `{path}`\n");
    let start = md
        .find(&heading)
        .unwrap_or_else(|| panic!("no heading for {path}:\n{md}"));
    let rest = &md[start + heading.len()..];
    let end = rest.find("\n## ").map_or(rest.len(), |i| i + 1);
    &rest[..end]
}

/// The table rows of a file section: lines that start with a cell and
/// are not the header or its rule.
fn rows(section: &str) -> Vec<&str> {
    section
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Line ") && !l.starts_with("|---"))
        .collect()
}

/// The one `**At deploy time:**` line of a file section.
fn advice(section: &str) -> &str {
    let lines: Vec<&str> = section
        .lines()
        .filter(|l| l.starts_with("**At deploy time:** "))
        .collect();
    assert_eq!(lines.len(), 1, "one advice line per file:\n{section}");
    lines[0]
}

fn assert_no_canary(label: &str, md: &str) {
    for canary in CANARIES {
        assert!(!md.contains(canary), "{label}: `{canary}` survives:\n{md}");
    }
}

fn read(tree: &tempfile::TempDir, name: &str) -> String {
    std::fs::read_to_string(tree.path().join(name))
        .unwrap_or_else(|e| panic!("the export's {name} reads: {e}"))
}

/// The scan export groups every finding under its file, each with the
/// redactions on it and the line that fits the kind of file.
#[test]
fn the_scan_export_groups_findings_per_file() {
    let snap = reload(&scan(secret_host()));
    let md = read(&render_export(&snap), "secrets-review.md");
    assert_no_canary("scan export", &md);

    let app = file_section(&md, APP_CONF);
    let app_rows = rows(app);
    assert_eq!(app_rows.len(), 2, "{app}");
    assert!(
        app_rows[0].starts_with("| 2 | `password` | redacted inline | `REDACTED_PASSWORD_<N>` |"),
        "{app}"
    );
    assert!(
        app_rows[1].starts_with("| 4 | `awskey` | redacted inline | `REDACTED_AWSKEY_<N>` |"),
        "{app}"
    );
    assert!(
        advice(app).contains("Carried in the image with placeholders in place"),
        "{app}"
    );

    let backup = file_section(&md, BACKUP_CONF);
    assert_eq!(rows(backup).len(), 2, "{backup}");
    assert!(rows(backup)[0].starts_with("| 2 |"), "{backup}");
    assert!(rows(backup)[1].starts_with("| 3 |"), "{backup}");
    assert!(advice(backup).contains("Carried in the image"), "{backup}");

    let sysctl = file_section(&md, SYSCTL_CONF);
    assert_eq!(rows(sysctl).len(), 1, "{sysctl}");

    // A machine-state file is described once, the way the class
    // describes it, and names the step the provisioning notice names.
    let ipa = file_section(&md, IPA_CONF);
    assert!(!rows(ipa).is_empty(), "{ipa}");
    let ipa_advice = advice(ipa);
    assert!(
        ipa_advice.contains("Left out of the image as this host's own state"),
        "{ipa}"
    );
    assert!(
        ipa_advice.contains("ipa-client-install, realm join or adcli join"),
        "{ipa}"
    );
    assert!(!ipa_advice.contains("Carried"), "{ipa}");
    let audit = read(&render_export(&snap), "audit-report.md");
    assert!(
        audit.contains("ipa-client-install, realm join or adcli join"),
        "the audit report names the same step:\n{audit}"
    );

    // Every file a finding names has one heading, in path order.
    let headings: Vec<&str> = md.lines().filter(|l| l.starts_with("## `")).collect();
    let mut sorted = headings.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(headings, sorted, "{md}");
}

/// The fleet aggregate's export writes the same per-file layout, with no
/// secret from any host.
#[test]
fn the_fleet_export_groups_findings_per_file() {
    let hosts = ["host-a", "host-b"]
        .map(|name| {
            reload(&scan(
                secret_host().file("/etc/hostname", &format!("{name}\n")),
            ))
        })
        .to_vec();
    let (merged, _) = merge_snapshots(hosts, None, None).expect("the hosts merge");
    let merged = reload(&merged);
    let md = read(&render_export(&merged), "secrets-review.md");
    assert_no_canary("fleet export", &md);
    let app = file_section(&md, APP_CONF);
    assert!(!rows(app).is_empty(), "{app}");
    advice(app);
    advice(file_section(&md, BACKUP_CONF));
}
