//! Secrets review renderer: produces `secrets-review.md`, every
//! redaction finding grouped by the file it is in, each file with one line
//! on how to supply the real value at deploy time.
//!
//! The file holds paths, finding names and placeholder forms only, never a
//! byte of a secret: every cell is a path, an enum-owned label or a
//! finding's own text, and nothing is read from file content.

use std::collections::BTreeMap;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::machine_state::{ProvisioningNotice, ProvisioningStep};
use inspectah_core::types::redaction::{DetectionMethod, RedactionFinding, RedactionKind};

use inspectah_core::types::nonrpm::{FileType, PayloadRedaction, UncheckedPayload};

use super::configtree::{
    Destination, ETC_UNIT_DIR, KARGS_PATH, owed_kernel_arguments, staged_unit_file, staged_writes,
    unmanaged_file_withheld,
};
use super::machine_state::class_rows;
use super::md_literal::{md_code, md_code_cell};
use super::unmanaged::UnmanagedDestination;

/// The path kernel command line findings carry.
const CMDLINE_PATH: &str = "/proc/cmdline";
/// The account database whose hashes the shadow pass replaces.
const SHADOW_PATH: &str = "/etc/shadow";
/// The mount table whose options the storage pass replaces.
const FSTAB_PATH: &str = "/etc/fstab";
/// The directory dnf reads `.repo` files from.
const YUM_REPOS_DIR: &str = "/etc/yum.repos.d/";
/// The prefix redaction gives a running container's environment.
const CONTAINER_PREFIX: &str = "container:";
/// The prefix redaction gives a unit the scheduled section generates from
/// a cron entry: `generated:<name>.service` or `generated:<name>.timer`.
const GENERATED_UNIT_PREFIX: &str = "generated:";

/// How to supply the real value of one file's redactions at deploy time:
/// the one line under each file heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupplyAdvice {
    /// A machine-state file the export leaves out, with the step that
    /// replaces it on a deployed machine.
    MachineStateOwed(ProvisioningStep),
    /// A machine-state file the export leaves out that is recreated
    /// without operator action.
    MachineStateAutomatic,
    /// A machine-state file the export leaves out that the image does not
    /// need ([`ProvisioningNotice::None`]).
    MachineStateNotNeeded,
    /// A machine-state file an operator override carries.
    MachineStateCarried,
    /// Every finding left the whole file out.
    NotInImage,
    /// Nothing was replaced: every finding is flagged for review.
    /// `carried` when the export stages the file, so it ships as the host
    /// had it; otherwise where it goes is not known here.
    ReviewOnly { carried: bool },
    /// The kernel command line; `owed` when the kargs file leaves out
    /// an operator argument because redaction replaced its value, which
    /// the provisioning steps then name.
    KernelArgument { owed: bool },
    /// Password hashes in the account database.
    AccountPassword,
    /// A running container's environment.
    ContainerEnvironment,
    /// Credentials in mount options.
    MountOption,
    /// A package registry credential in a language package manifest.
    PackageRegistry,
    /// A credential in a `.repo` file's URL.
    RepoCredential,
    /// A file the image carries under `/usr`, read-only when deployed.
    CarriedInUsr,
    /// A file the image carries anywhere else, `/etc` in practice.
    Carried,
    /// An unmanaged file the export ships under `unmanaged/` with
    /// placeholders in place; `var_srv` when it is under `/srv`, which the
    /// Containerfile leaves to the `/var` strategy instead of copying.
    UnmanagedRedacted { var_srv: bool },
    /// An unmanaged file the export ships under `unmanaged/` as the scan
    /// read it, with nothing replaced in that copy.
    UnmanagedAsRead { var_srv: bool },
    /// An included unmanaged file the export leaves out because redaction
    /// could not check it.
    UnmanagedWithheld(UncheckedPayload),
    /// A file not matched to anything the export stages. The image may
    /// still carry it another way, so the line says what is not known
    /// instead of asserting it is absent.
    Unmatched,
}

impl SupplyAdvice {
    /// The line for the file at `key`, plain text a person would write.
    fn line(self, key: &str) -> String {
        match self {
            Self::MachineStateOwed(step) => format!(
                "Left out of the image as this host's own state (see Provisioning steps in \
                 the audit report). At provisioning: {}.",
                step.command()
            ),
            Self::MachineStateAutomatic => "Left out of the image as this host's own state, and \
                 recreated on a deployed machine without operator action: nothing to supply."
                .into(),
            Self::MachineStateNotNeeded => "Left out of the image as this host's own state, and \
                 not needed in the image: nothing to supply."
                .into(),
            Self::MachineStateCarried => {
                "Carried in the image by an operator override although it \
                 is this host's own state, with placeholders in place: every machine built from \
                 the image gets this file, so put the real values in place on each machine at \
                 deploy time."
                    .into()
            }
            Self::NotInImage => "Not in the image, so nothing to do there. If a deployed machine \
                 needs this file, provision it at deploy time."
                .into(),
            Self::ReviewOnly { carried: true } => "Nothing was replaced, and the image carries \
                 this file as the host had it. Check the flagged lines, and move any real secret \
                 out of the image into a deploy-time step."
                .into(),
            Self::ReviewOnly { carried: false } => {
                "Nothing was replaced. Check the flagged lines: \
                 if one holds a real secret and the image carries this file, move the value out \
                 of the image into a deploy-time step."
                    .into()
            }
            Self::KernelArgument { owed: true } => format!(
                "A kernel argument whose value was redacted is left out of the image's kernel \
                 arguments file {}: pass each with its real value at install time, for example \
                 with `bootc install to-disk --karg`. Provisioning steps in the audit report \
                 names each one.",
                md_code(KARGS_PATH)
            ),
            Self::KernelArgument { owed: false } => "Not in the image: pass the real kernel \
                 argument at install time, for example with `bootc install to-disk --karg`."
                .into(),
            Self::AccountPassword => "The password hashes here were replaced: set each account's \
                 password at provisioning, for example with kickstart `user --password` or \
                 cloud-init."
                .into(),
            Self::ContainerEnvironment => "A running container's environment, not a file: give \
                 the container the value at deploy time with `podman secret` or an environment \
                 file written at provisioning."
                .into(),
            Self::MountOption => "Keep mount credentials out of the image and supply them at \
                 deploy time. For SMB/CIFS, provision a mode-0600 file referenced by \
                 `credentials=`; for other filesystems, use the mount helper's supported \
                 credential mechanism."
                .into(),
            Self::PackageRegistry => "A registry credential for the build: give it to the build \
                 as a secret (`podman build --secret`) or a credential helper, never in the \
                 manifest."
                .into(),
            Self::RepoCredential => "A repository credential: give it to the build as a secret \
                 (`podman build --secret`), and write it at provisioning if deployed machines use \
                 this repo."
                .into(),
            Self::CarriedInUsr => "Carried in the image under `/usr`, which is read-only on a \
                 deployed machine: configure the program to read a file under `/etc` provisioned \
                 on each deployed machine, or a mounted secret supplied at deploy time."
                .into(),
            Self::Carried => "Carried in the image with placeholders in place: write the real \
                 values on each machine at deploy time, with a drop-in where the program reads \
                 one, a mounted secret, or a provisioning step. `/etc` keeps that local change \
                 across upgrades."
                .into(),
            Self::UnmanagedRedacted { var_srv: false } => format!(
                "Shipped in this export as {}, with placeholders in place, and the \
                 Containerfile copies it into the image, where it is read-only on a deployed \
                 machine: configure the program to read the real value from a file under `/etc` \
                 provisioned on each deployed machine, or from a mounted secret supplied at \
                 deploy time.",
                md_code(&shipped_copy(key))
            ),
            Self::UnmanagedRedacted { var_srv: true } => format!(
                "Shipped in this export as {}, with placeholders in place, for your `/var` \
                 strategy (the Containerfile does not copy it): put the real values in place \
                 when you restore it on each machine.",
                md_code(&shipped_copy(key))
            ),
            Self::UnmanagedAsRead { var_srv } => format!(
                "Shipped in this export as {}, as the scan read it, with nothing replaced in \
                 that copy{}. Check it, and supply any real value at deploy time with a mounted \
                 secret or a provisioning step.",
                md_code(&shipped_copy(key)),
                if var_srv {
                    ", for your `/var` strategy"
                } else {
                    ", and the Containerfile copies it into the image"
                }
            ),
            Self::UnmanagedWithheld(reason) => format!(
                "Left out of this export and the image: inspectah could not check it for \
                 credentials, because {}. Check the file on the host; if a deployed machine \
                 needs it, supply it yourself, with any real value from a mounted secret or a \
                 provisioning step.",
                reason.describe()
            ),
            Self::Unmatched => "Not matched to a file inspectah stages for the image, so check \
                 whether the image carries it another way. Wherever a deployed machine needs the \
                 value, supply it at deploy time with a mounted secret or a provisioning step."
                .into(),
        }
    }
}

/// Where the export ships the unmanaged file at `key`.
fn shipped_copy(key: &str) -> String {
    format!("unmanaged/{}", key.trim_start_matches('/'))
}

/// What the image knows about where findings land: the destinations it
/// carries, the machine-state rows and the unmanaged payloads it ships,
/// each keyed by normalized path.
struct Carriage {
    carried: std::collections::BTreeSet<String>,
    machine_state: BTreeMap<String, SupplyAdvice>,
    /// Each regular unmanaged file the export ships under `unmanaged/`,
    /// with where it lands and what redaction did to the shipped copy.
    unmanaged: BTreeMap<String, (UnmanagedDestination, PayloadRedaction)>,
    /// Each included unmanaged file the export leaves out because
    /// redaction could not check it, with why.
    unmanaged_withheld: BTreeMap<String, UncheckedPayload>,
    /// The kargs file leaves out a kernel argument redaction replaced.
    owed_kargs: bool,
}

impl Carriage {
    fn of(snap: &InspectionSnapshot) -> Self {
        let mut carried: std::collections::BTreeSet<String> = staged_writes(snap)
            .writes
            .keys()
            .map(|d| d.as_str().to_owned())
            .collect();
        // Quadlets are staged under `quadlet/` and copied to
        // `/etc/containers/systemd/` by the containers section.
        if let Some(containers) = &snap.containers {
            carried.extend(
                containers
                    .quadlet_units
                    .iter()
                    .filter(|q| q.disposition.is_included() && !q.content.is_empty())
                    .filter_map(|q| Destination::new(&q.path).ok())
                    .map(|d| d.as_str().to_owned()),
            );
        }
        let machine_state = class_rows(snap)
            .into_iter()
            .filter_map(|row| {
                let key = Destination::new(row.path).ok()?.as_str().to_owned();
                let advice = if row.included {
                    SupplyAdvice::MachineStateCarried
                } else {
                    match row.member.notice() {
                        ProvisioningNotice::Owed { step } => SupplyAdvice::MachineStateOwed(step),
                        ProvisioningNotice::Automatic { .. } => SupplyAdvice::MachineStateAutomatic,
                        ProvisioningNotice::None => SupplyAdvice::MachineStateNotNeeded,
                    }
                };
                Some((key, advice))
            })
            .collect();
        let regular_included = || {
            snap.unmanaged_files
                .iter()
                .flat_map(|s| &s.items)
                .filter(|f| f.disposition.is_included() && f.file_type != FileType::Symlink)
        };
        let unmanaged_withheld = regular_included()
            .filter_map(|f| {
                let reason = f.payload_redaction.withheld()?;
                Some((Destination::new(&f.path).ok()?.as_str().to_owned(), reason))
            })
            .collect();
        let unmanaged = regular_included()
            .filter(|f| unmanaged_file_withheld(f).is_none())
            .filter_map(|f| {
                let key = Destination::new(&f.path).ok()?.as_str().to_owned();
                Some((
                    key,
                    (UnmanagedDestination::of(&f.path), f.payload_redaction),
                ))
            })
            .collect();
        Self {
            carried,
            machine_state,
            unmanaged,
            unmanaged_withheld,
            owed_kargs: !owed_kernel_arguments(snap).is_empty(),
        }
    }

    /// Whether the export stages the file at `key`. A unit generated from
    /// a cron entry is recorded under its `generated:` label, and is
    /// looked up at the destination the stager gives it.
    fn is_carried(&self, key: &str) -> bool {
        if let Some(file) = key.strip_prefix(GENERATED_UNIT_PREFIX) {
            return file
                .rsplit_once('.')
                .and_then(|(name, suffix)| staged_unit_file(name, suffix))
                .is_some_and(|file| self.carried.contains(&format!("{ETC_UNIT_DIR}/{file}")));
        }
        self.carried.contains(key)
            || self
                .unmanaged
                .get(key)
                .is_some_and(|(dest, _)| *dest == UnmanagedDestination::Image)
    }

    /// The advice for the file at `key` holding `findings`, by the first
    /// rule that applies. A machine-state file takes its class line, so a
    /// file in both lists is described once, the way the class describes it.
    fn advice(&self, key: &str, findings: &[&RedactionFinding]) -> SupplyAdvice {
        if let Some(advice) = self.machine_state.get(key) {
            return *advice;
        }
        if let Some(reason) = self.unmanaged_withheld.get(key) {
            return SupplyAdvice::UnmanagedWithheld(*reason);
        }
        if findings.iter().all(|f| f.kind == RedactionKind::Excluded) {
            return SupplyAdvice::NotInImage;
        }
        if findings.iter().all(|f| f.kind == RedactionKind::Flagged) {
            return SupplyAdvice::ReviewOnly {
                carried: self.is_carried(key),
            };
        }
        if key.starts_with(CONTAINER_PREFIX) {
            return SupplyAdvice::ContainerEnvironment;
        }
        match key {
            CMDLINE_PATH => {
                return SupplyAdvice::KernelArgument {
                    owed: self.owed_kargs,
                };
            }
            SHADOW_PATH => return SupplyAdvice::AccountPassword,
            FSTAB_PATH => return SupplyAdvice::MountOption,
            _ => {}
        }
        if findings.iter().any(|f| f.source == "manifest_auth") {
            return SupplyAdvice::PackageRegistry;
        }
        if key
            .strip_prefix(YUM_REPOS_DIR)
            .is_some_and(|name| !name.contains('/') && name.ends_with(".repo"))
        {
            return SupplyAdvice::RepoCredential;
        }
        if let Some((dest, redaction)) = self.unmanaged.get(key) {
            let var_srv = *dest == UnmanagedDestination::VarSrv;
            return if *redaction == PayloadRedaction::Redacted {
                SupplyAdvice::UnmanagedRedacted { var_srv }
            } else {
                SupplyAdvice::UnmanagedAsRead { var_srv }
            };
        }
        if self.is_carried(key) {
            if key.starts_with("/usr/") {
                return SupplyAdvice::CarriedInUsr;
            }
            return SupplyAdvice::Carried;
        }
        SupplyAdvice::Unmatched
    }
}

/// The key a finding is grouped under: its destination in the image, so
/// `etc/x` and `/etc/x` are one file. A label with no path (`container:web`,
/// `quadlet:app`) is kept as written.
fn group_key(path: &str) -> String {
    let is_label = path
        .split_once(':')
        .is_some_and(|(prefix, _)| !prefix.is_empty() && !prefix.contains('/'));
    if is_label {
        return path.to_owned();
    }
    Destination::new(path)
        .map(|d| d.as_str().to_owned())
        .unwrap_or_else(|_| path.to_owned())
}

/// Render the secrets review markdown from a snapshot.
pub fn render_secrets_review(snap: &InspectionSnapshot) -> String {
    let has_redactions = !snap.redactions.is_empty();
    let has_subscription = snap.preserved_subscription;

    if !has_redactions && !has_subscription {
        return "# Secrets Review\n\nNo redactions recorded.\n".to_string();
    }

    let mut lines = Vec::new();
    lines.push("# Secrets Review".into());
    lines.push(String::new());

    let mut by_file: BTreeMap<String, Vec<&RedactionFinding>> = BTreeMap::new();
    for finding in &snap.redactions {
        if finding.source.is_empty() {
            continue;
        }
        by_file
            .entry(group_key(&finding.path))
            .or_default()
            .push(finding);
    }
    for findings in by_file.values_mut() {
        // Stable, so findings on one line keep the order redaction
        // recorded them in; a finding with no line comes last.
        findings.sort_by_key(|f| f.line.unwrap_or(i32::MAX));
    }

    let count = |kind: RedactionKind| {
        by_file
            .values()
            .flatten()
            .filter(|f| f.kind == kind)
            .count()
    };
    let n_excluded = count(RedactionKind::Excluded);
    let n_inline = count(RedactionKind::Inline);
    let n_flagged = count(RedactionKind::Flagged);
    let n_redacted = n_excluded + n_inline;
    let mut parts = Vec::new();
    if n_excluded > 0 {
        parts.push(format!("{n_excluded} excluded"));
    }
    if n_inline > 0 {
        parts.push(format!("{n_inline} inline"));
    }
    let breakdown = if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    };
    let flagged_part = if n_flagged == 0 {
        String::new()
    } else {
        format!(", {n_flagged} flagged for review")
    };
    let files = if by_file.len() == 1 { "file" } else { "files" };
    lines.push(format!(
        "> Detected secrets: {n_redacted} redacted{breakdown}{flagged_part}, in {} {files}",
        by_file.len()
    ));
    lines.push(String::new());

    if !by_file.is_empty() {
        lines.push(
            "Each file below lists what was redacted and where, and says how to supply the real \
             value at deploy time. Secret values were replaced with tokens of the form \
             `REDACTED_<KIND>_<N>`, such as `REDACTED_PASSWORD_1`. A placeholder marks where \
             redacted material was, so search the export for one to find that place; the same \
             token in two files, containers or fleet variants does not prove the credentials \
             match, and one credential can appear under different tokens. A credential in a URL's user information may read `[REDACTED]`, `REDACTED` or \
             `<REDACTED>` instead."
                .into(),
        );
        lines.push(String::new());
    }

    let carriage = Carriage::of(snap);
    for (key, findings) in &by_file {
        lines.push(format!("## {}", md_code(key)));
        lines.push(String::new());
        lines.push(format!(
            "**At deploy time:** {}",
            carriage.advice(key, findings).line(key)
        ));
        lines.push(String::new());
        lines.push("| Line | Finding | Handling | Replaced with | Detection |".into());
        lines.push("|------|---------|----------|---------------|-----------|".into());
        for f in findings {
            let line_str = f.line.map(|l| l.to_string()).unwrap_or_else(|| "--".into());
            let finding = if f.pattern.is_empty() {
                "--".to_string()
            } else {
                md_code_cell(&f.pattern)
            };
            lines.push(format!(
                "| {line_str} | {finding} | {} | {} | {} |",
                handling_cell(f),
                replaced_with(f),
                detection_cell(f)
            ));
        }
        lines.push(String::new());
    }

    // Subscription material
    if snap.preserved_subscription {
        lines.push("## Subscription Material".into());
        lines.push(String::new());
        lines.push(
            "RHEL subscription material was preserved in `subscription/` for build-time mounting."
                .into(),
        );
        lines.push(String::new());
        lines.push(
            "**Action:** Mount these directories at build time (do NOT copy into the image):"
                .into(),
        );
        lines.push(String::new());
        lines.push("```bash".into());
        lines.push("podman build \\".into());
        lines.push("  -v ./subscription/entitlement:/run/secrets/etc-pki-entitlement:z \\".into());
        lines.push("  -v ./subscription/rhsm:/run/secrets/rhsm:z \\".into());
        lines.push("  -v ./subscription/redhat.repo:/run/secrets/redhat.repo:z \\".into());
        lines.push("  -f Containerfile .".into());
        lines.push("```".into());
        lines.push(String::new());
        lines.push("Or use the build helper:".into());
        lines.push(String::new());
        lines.push("```bash".into());
        lines.push("inspectah build <tarball> -t <name:tag>".into());
        lines.push("```".into());
        lines.push(String::new());
        if let Some(ref sub) = snap.subscription
            && !sub.entitlement_certs.is_empty()
        {
            lines.push(format!(
                "**Certificates:** {} entitlement certificate(s)",
                sub.entitlement_certs.len()
            ));
            if let Some(expiry) = sub.earliest_expiry {
                lines.push(format!(
                    "**Earliest expiry:** {}",
                    expiry
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_else(|_| "unknown".into())
                ));
            }
            lines.push(String::new());
        }
    }

    lines.join("\n")
}

/// What happened to the finding: replaced in place, the whole file left
/// out (with the remediation the finding names), or flagged.
fn handling_cell(f: &RedactionFinding) -> String {
    match f.kind {
        RedactionKind::Inline => "redacted inline".into(),
        RedactionKind::Excluded => format!(
            "file excluded: {}",
            md_code_cell(remediation_label(&f.remediation))
        ),
        RedactionKind::Flagged => "flagged for review".into(),
    }
}

/// What stands in the export where the secret was, by the pass that
/// recorded the finding. Only the form: the token's number is per run.
fn replaced_with(f: &RedactionFinding) -> String {
    match f.kind {
        RedactionKind::Excluded => return "whole file left out".into(),
        RedactionKind::Flagged => return "nothing".into(),
        RedactionKind::Inline => {}
    }
    match f.source.as_str() {
        "pattern" if !f.pattern.is_empty() => {
            md_code_cell(&format!("REDACTED_{}_<N>", f.pattern.to_uppercase()))
        }
        "shadow" => "`REDACTED_SHADOWHASH_<N>`".into(),
        "mount_options" => "`REDACTED_MOUNT_PASSWORD_<N>`".into(),
        "proxy_credential" | "token_credential" => "`[REDACTED]`".into(),
        "repo_url_credential" | "manifest_auth" => "`REDACTED` in the URL".into(),
        "compose_env" => "`<REDACTED>`".into(),
        _ => "a `REDACTED` placeholder".into(),
    }
}

/// How the finding was detected, with its confidence where the method
/// alone does not settle it: a heuristic, or a finding flagged for review.
fn detection_cell(f: &RedactionFinding) -> String {
    let method = match f.detection_method {
        DetectionMethod::Pattern => "pattern",
        DetectionMethod::Heuristic => "heuristic",
        DetectionMethod::PathBased => "path-based",
    };
    let show_confidence =
        f.detection_method == DetectionMethod::Heuristic || f.kind == RedactionKind::Flagged;
    match f.confidence {
        Some(c) if show_confidence => {
            format!("{method} ({})", format!("{c:?}").to_lowercase())
        }
        None if f.detection_method == DetectionMethod::Heuristic => format!("{method} (unknown)"),
        _ => method.into(),
    }
}

fn remediation_label(rem: &str) -> &str {
    match rem {
        "regenerate" => "Regenerate on target",
        "provision" => "Provision from secret store",
        "value-removed" => "Value removed inline",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secrets_review_renders() {
        let snap = InspectionSnapshot::new();
        let md = render_secrets_review(&snap);
        assert!(md.contains("# Secrets Review"));
    }

    #[test]
    fn test_secrets_review_no_redactions() {
        let snap = InspectionSnapshot::new();
        let md = render_secrets_review(&snap);
        assert!(md.contains("No redactions recorded"));
    }

    #[test]
    fn test_secrets_review_with_findings() {
        let mut snap = InspectionSnapshot::new();
        snap.redactions = vec![RedactionFinding {
            path: "/etc/shadow".into(),
            source: "file".into(),
            kind: RedactionKind::Excluded,
            pattern: "shadow_hash".into(),
            remediation: "regenerate".into(),
            detection_method: DetectionMethod::Pattern,
            line: None,
            replacement: None,
            confidence: None,
            finding_kind: None,
        }];
        let md = render_secrets_review(&snap);
        assert!(md.contains("# Secrets Review"));
        assert!(md.contains("## `/etc/shadow`"), "{md}");
        assert!(
            md.contains("| -- | `shadow_hash` | file excluded: `Regenerate on target` | whole file left out | pattern |"),
            "{md}"
        );
        assert!(md.contains(&SupplyAdvice::NotInImage.line("")), "{md}");
    }

    /// The Line column is the line the secret is on, also when the key
    /// is indented or follows other text on its line.
    #[test]
    fn test_secrets_review_line_is_the_secret_line() {
        use crate::redaction::{RedactOptions, redact};
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: "/etc/app/app.conf".to_string(),
                content: "[db]\n  password = hunter2\n# note\nAKIAABCDEFGHIJKLMNOP\n".to_string(),
                ..Default::default()
            }],
        });
        redact(&mut snap, &RedactOptions::default());
        let md = render_secrets_review(&snap);
        assert!(md.contains("## `/etc/app/app.conf`"), "{md}");
        assert!(md.contains("| 2 | `password` |"), "{md}");
        assert!(md.contains("| 4 | `awskey` |"), "{md}");
    }

    /// The Inline Redactions note names the token form redaction writes,
    /// `REDACTED_<KIND>_<N>`, so a reader can search the export for it.
    #[test]
    fn test_secrets_review_names_the_token_form() {
        use crate::redaction::{RedactOptions, redact};
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: "/etc/app/app.conf".to_string(),
                content: "password = hunter2\n".to_string(),
                ..Default::default()
            }],
        });
        redact(&mut snap, &RedactOptions::default());
        let content = &snap.config.as_ref().unwrap().files[0].content;
        assert!(content.contains("REDACTED_PASSWORD_1"), "{content}");
        let md = render_secrets_review(&snap);
        assert!(!md.contains("[REDACTED-*]"), "{md}");
        assert!(md.contains("`REDACTED_<KIND>_<N>`"), "{md}");
        assert!(md.contains("`REDACTED_PASSWORD_1`"), "{md}");
        // A token locates redacted material; it is not an identity: two
        // hosts or containers number their secrets separately.
        assert!(!md.contains("same token everywhere"), "{md}");
        assert!(md.contains("does not prove the credentials match"), "{md}");
    }

    #[test]
    fn test_secrets_review_subscription_only() {
        use inspectah_core::types::subscription::SubscriptionSection;
        let mut snap = InspectionSnapshot::new();
        snap.preserved_subscription = true;
        snap.subscription = Some(SubscriptionSection::default());
        snap.redactions = vec![]; // No redactions, only subscription
        let md = render_secrets_review(&snap);
        assert!(
            md.contains("# Secrets Review"),
            "must have secrets review header"
        );
        assert!(
            !md.contains("No redactions recorded"),
            "must not say 'no redactions' when subscription is present"
        );
        assert!(
            md.contains("## Subscription Material"),
            "must have subscription section"
        );
        assert!(
            md.contains("inspectah build"),
            "must reference inspectah build helper"
        );
    }

    fn finding(path: &str, source: &str, kind: RedactionKind) -> RedactionFinding {
        RedactionFinding {
            path: path.into(),
            source: source.into(),
            kind,
            pattern: "password".into(),
            remediation: String::new(),
            line: Some(1),
            replacement: None,
            detection_method: DetectionMethod::Pattern,
            confidence: None,
            finding_kind: None,
        }
    }

    /// Each kind of file gets its own line, by the first rule that
    /// applies: excluded, flagged-only, then by what the path is and
    /// whether the image carries it.
    #[test]
    fn test_supply_advice_by_kind_of_file() {
        use RedactionKind::{Excluded, Flagged, Inline};
        let snap = InspectionSnapshot::new();
        let carriage = Carriage::of(&snap);
        let cases: &[(&str, &str, &[RedactionKind], SupplyAdvice)] = &[
            (
                "/etc/a.conf",
                "pattern",
                &[Excluded],
                SupplyAdvice::NotInImage,
            ),
            (
                "/etc/a.conf",
                "pattern",
                &[Flagged, Flagged],
                SupplyAdvice::ReviewOnly { carried: false },
            ),
            (
                "container:web",
                "pattern",
                &[Inline],
                SupplyAdvice::ContainerEnvironment,
            ),
            (
                "/proc/cmdline",
                "pattern",
                &[Inline],
                SupplyAdvice::KernelArgument { owed: false },
            ),
            (
                "/etc/shadow",
                "shadow",
                &[Inline, Flagged],
                SupplyAdvice::AccountPassword,
            ),
            (
                "/etc/fstab",
                "mount_options",
                &[Inline],
                SupplyAdvice::MountOption,
            ),
            (
                "/opt/app/package.json",
                "manifest_auth",
                &[Inline],
                SupplyAdvice::PackageRegistry,
            ),
            (
                "/etc/yum.repos.d/corp.repo",
                "repo_url_credential",
                &[Inline],
                SupplyAdvice::RepoCredential,
            ),
            (
                "/etc/app/env",
                "pattern",
                &[Inline],
                SupplyAdvice::Unmatched,
            ),
        ];
        for (path, source, kinds, want) in cases {
            let findings: Vec<RedactionFinding> = kinds
                .iter()
                .map(|k| finding(path, source, k.clone()))
                .collect();
            let refs: Vec<&RedactionFinding> = findings.iter().collect();
            assert_eq!(carriage.advice(&group_key(path), &refs), *want, "{path}");
        }
    }

    /// A file the config tree stages is carried; one under `/usr` is
    /// read-only on a deployed machine and says so.
    #[test]
    fn test_supply_advice_for_carried_files() {
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: "/etc/app/app.conf".to_string(),
                content: "password = REDACTED_PASSWORD_1\n".to_string(),
                ..Default::default()
            }],
        });
        let carriage = Carriage::of(&snap);
        let f = finding("etc/app/app.conf", "pattern", RedactionKind::Inline);
        assert_eq!(
            carriage.advice(&group_key(&f.path), &[&f]),
            SupplyAdvice::Carried
        );
        let mut usr = Carriage::of(&InspectionSnapshot::new());
        usr.carried.insert("/usr/lib/app/app.conf".into());
        let f = finding("/usr/lib/app/app.conf", "pattern", RedactionKind::Inline);
        assert_eq!(usr.advice(&f.path, &[&f]), SupplyAdvice::CarriedInUsr);
    }

    /// A unit generated from a cron entry is recorded under its
    /// `generated:` label and staged under `/etc/systemd/system`: when
    /// the unit is included the label resolves to that destination and
    /// reads as carried; when it is excluded nothing stages it, and the
    /// line says so without claiming where the value is.
    #[test]
    fn test_supply_advice_for_a_generated_unit() {
        use inspectah_core::types::FindingKind;
        use inspectah_core::types::scheduled::{GeneratedTimerUnit, ScheduledTaskSection};
        let with_unit = |disposition: FindingKind| {
            let mut snap = InspectionSnapshot::new();
            snap.scheduled_tasks = Some(ScheduledTaskSection {
                generated_timer_units: vec![GeneratedTimerUnit {
                    name: "cron-backup".into(),
                    timer_content: "[Timer]\nOnCalendar=daily\n".into(),
                    service_content: "[Service]\nExecStart=/usr/bin/backup \
                                      --password=REDACTED_PASSWORD_1\n"
                        .into(),
                    disposition,
                    ..Default::default()
                }],
                ..Default::default()
            });
            Carriage::of(&snap)
        };
        let svc = finding(
            "generated:cron-backup.service",
            "pattern",
            RedactionKind::Inline,
        );
        let flagged = finding(
            "generated:cron-backup.timer",
            "pattern",
            RedactionKind::Flagged,
        );
        let included = with_unit(FindingKind::included());
        assert_eq!(
            included.advice(&group_key(&svc.path), &[&svc]),
            SupplyAdvice::Carried
        );
        assert_eq!(
            included.advice(&group_key(&flagged.path), &[&flagged]),
            SupplyAdvice::ReviewOnly { carried: true }
        );
        let excluded = with_unit(FindingKind::excluded());
        assert_eq!(
            excluded.advice(&group_key(&svc.path), &[&svc]),
            SupplyAdvice::Unmatched
        );
        assert!(!SupplyAdvice::Unmatched.line("").contains("Not copied"));
    }

    /// A file with only flagged findings says it ships as the host had it
    /// only when the export stages it: refine can exclude it, and then it
    /// does not ship.
    #[test]
    fn test_supply_advice_for_a_flagged_file_follows_its_route() {
        use inspectah_core::types::FindingKind;
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        let with_file = |disposition: FindingKind| {
            let mut snap = InspectionSnapshot::new();
            snap.config = Some(ConfigSection {
                withheld: Vec::new(),
                files: vec![ConfigFileEntry {
                    path: "/etc/app/app.conf".to_string(),
                    content: "key = maybe\n".to_string(),
                    disposition,
                    ..Default::default()
                }],
            });
            Carriage::of(&snap)
        };
        let f = finding(
            "/etc/app/app.conf",
            "inspector_hint",
            RedactionKind::Flagged,
        );
        let included = with_file(FindingKind::included()).advice(&f.path, &[&f]);
        assert_eq!(included, SupplyAdvice::ReviewOnly { carried: true });
        assert!(included.line("").contains("as the host had it"));
        let excluded = with_file(FindingKind::excluded()).advice(&f.path, &[&f]);
        assert_eq!(excluded, SupplyAdvice::ReviewOnly { carried: false });
        assert!(
            !excluded.line("").contains("as the host had it"),
            "{}",
            excluded.line("")
        );
        assert!(
            !excluded.line("").contains("ships"),
            "{}",
            excluded.line("")
        );
    }

    /// A machine-state file the image does not need reads the way the
    /// provisioning table describes it, not as recreated.
    #[test]
    fn test_supply_advice_for_a_machine_state_file_not_needed() {
        use inspectah_core::types::FindingKind;
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        use inspectah_core::types::machine_state::CarriableMember;
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: "/etc/grub.d/00_tuned".to_string(),
                content: "password = REDACTED_PASSWORD_1\n".to_string(),
                disposition: FindingKind::MachineState {
                    member: CarriableMember::GrubTuned,
                    include: false,
                },
                ..Default::default()
            }],
        });
        let f = finding("/etc/grub.d/00_tuned", "pattern", RedactionKind::Inline);
        let advice = Carriage::of(&snap).advice(&f.path, &[&f]);
        assert_eq!(advice, SupplyAdvice::MachineStateNotNeeded);
        let table = super::super::machine_state::notice_text(ProvisioningNotice::None);
        assert!(
            advice.line("").contains(&table),
            "{} / {table}",
            advice.line("")
        );
        assert!(
            !advice.line("").contains("recreated"),
            "{}",
            advice.line("")
        );
    }

    /// Mount credential advice names `credentials=` for SMB/CIFS only:
    /// other filesystems take their own helper's mechanism.
    #[test]
    fn test_mount_option_advice_is_scoped_to_cifs() {
        let line = SupplyAdvice::MountOption.line("");
        assert!(
            line.contains("For SMB/CIFS, provision a mode-0600 file referenced by `credentials=`"),
            "{line}"
        );
        assert!(
            line.contains(
                "for other filesystems, use the mount helper's supported credential mechanism"
            ),
            "{line}"
        );
    }

    /// A finding recorded with a relative path and one with an absolute
    /// path are one file; a label with no path is kept as written.
    #[test]
    fn test_group_key_normalizes_paths_and_keeps_labels() {
        assert_eq!(group_key("etc/app.conf"), "/etc/app.conf");
        assert_eq!(group_key("/etc//app.conf"), "/etc/app.conf");
        assert_eq!(group_key("container:web"), "container:web");
        assert_eq!(group_key("quadlet:app.container"), "quadlet:app.container");
        assert_eq!(group_key("/etc/a:b"), "/etc/a:b");
    }

    /// The placeholder column names the form each pass writes, and a
    /// finding that replaced nothing says so.
    #[test]
    fn test_replaced_with_names_each_pass_form() {
        let cases: &[(&str, RedactionKind, &str)] = &[
            ("pattern", RedactionKind::Inline, "`REDACTED_PASSWORD_<N>`"),
            ("shadow", RedactionKind::Inline, "`REDACTED_SHADOWHASH_<N>`"),
            ("proxy_credential", RedactionKind::Inline, "`[REDACTED]`"),
            (
                "repo_url_credential",
                RedactionKind::Inline,
                "`REDACTED` in the URL",
            ),
            ("compose_env", RedactionKind::Inline, "`<REDACTED>`"),
            ("pattern", RedactionKind::Flagged, "nothing"),
            ("pattern", RedactionKind::Excluded, "whole file left out"),
        ];
        for (source, kind, want) in cases {
            let f = finding("/etc/x", source, kind.clone());
            assert_eq!(replaced_with(&f), *want, "{source} {kind:?}");
        }
    }

    /// An unmanaged file the export ships: the line names the shipped
    /// copy, says whether placeholders are in it, and never calls it
    /// unmatched. A `/srv` one is left to the `/var` strategy; an excluded
    /// one is not shipped; one the bundle withheld unchecked says it is
    /// left out and why, and names no shipped copy.
    #[test]
    fn test_supply_advice_for_unmanaged_payloads() {
        use inspectah_core::types::FindingKind as Disposition;
        use inspectah_core::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};
        let item = |path: &str, redaction: PayloadRedaction, included: bool| UnmanagedFile {
            path: path.into(),
            file_type: FileType::DataFile,
            disposition: if included {
                Disposition::included()
            } else {
                Disposition::excluded()
            },
            payload_redaction: redaction,
            ..Default::default()
        };
        let mut snap = InspectionSnapshot::new();
        snap.unmanaged_files = Some(UnmanagedFileSection {
            items: vec![
                item("/opt/app/.env", PayloadRedaction::Redacted, true),
                item("/srv/app/.env", PayloadRedaction::Redacted, true),
                item("/opt/app/plain.conf", PayloadRedaction::Unchanged, true),
                item("/opt/app/off.env", PayloadRedaction::Redacted, false),
                item(
                    "/opt/app/huge.log",
                    PayloadRedaction::Withheld(UncheckedPayload::TooLarge),
                    true,
                ),
                item(
                    "/opt/app/latin1.env",
                    PayloadRedaction::Withheld(UncheckedPayload::NotUtf8),
                    true,
                ),
            ],
            ..Default::default()
        });
        let carriage = Carriage::of(&snap);
        let advice = |path: &str, kind: RedactionKind| {
            let f = finding(path, "pattern", kind);
            carriage.advice(&group_key(path), &[&f])
        };
        let image = advice("opt/app/.env", RedactionKind::Inline);
        assert_eq!(image, SupplyAdvice::UnmanagedRedacted { var_srv: false });
        let line = image.line("/opt/app/.env");
        assert!(line.contains("`unmanaged/opt/app/.env`"), "{line}");
        assert!(line.contains("with placeholders in place"), "{line}");
        assert!(line.contains("copies it into the image"), "{line}");
        let srv = advice("/srv/app/.env", RedactionKind::Inline);
        assert_eq!(srv, SupplyAdvice::UnmanagedRedacted { var_srv: true });
        assert!(
            srv.line("/srv/app/.env")
                .contains("`unmanaged/srv/app/.env`")
        );
        assert!(srv.line("/srv/app/.env").contains("does not copy it"));
        assert_eq!(
            advice("/opt/app/plain.conf", RedactionKind::Inline),
            SupplyAdvice::UnmanagedAsRead { var_srv: false }
        );
        assert_eq!(
            advice("/opt/app/off.env", RedactionKind::Inline),
            SupplyAdvice::Unmatched
        );
        for (path, reason) in [
            ("/opt/app/huge.log", UncheckedPayload::TooLarge),
            ("/opt/app/latin1.env", UncheckedPayload::NotUtf8),
        ] {
            let withheld = advice(path, RedactionKind::Excluded);
            assert_eq!(withheld, SupplyAdvice::UnmanagedWithheld(reason));
            let line = withheld.line(path);
            assert!(line.contains("Left out of this export"), "{line}");
            assert!(line.contains(reason.describe()), "{line}");
            assert!(!line.contains("unmanaged/"), "{line}");
        }
    }
}
