//! What every export says about the machine-state class: the provisioning
//! steps a deployed machine owes, the "Machine state" table and the export
//! notices. The same surfaces carry the one owed step that is not a
//! machine-state file: the kernel arguments the kargs file leaves out
//! because redaction replaced their values.
//!
//! Every surface is built here from the snapshot, so the audit report, the
//! Containerfile, the README, `x-export-warnings` and the scan's stderr
//! list the same steps in the same order. Notices are derived from each
//! row's member, never stored. All text is enum-owned, a path or a kernel
//! argument with its redacted value shown as `<value>`; no file content
//! reaches any surface.

use std::collections::BTreeMap;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::machine_state::{
    CarriableMember, MachineStateMember, OverrideLevel, ProvisioningNotice, ProvisioningStep,
};

use super::comment::escape_comment_text;
use super::configtree::owed_kernel_arguments;
use super::md_literal::{md_code, md_code_cell};
use crate::export_warning::ExportWarning;

/// The most bytes of a path the export header carries in a `warn`
/// override notice.
const NOTICE_PATH_MAX: usize = 200;

/// How a deployed machine gets a kernel argument the image leaves out.
const KARGS_INSTALL_STEP: &str = "bootc install to-disk --karg";

/// The audit report's notice for the NVMe host id files. Their member's
/// [`ProvisioningNotice`] says first boot recreates them, which is false on
/// a RHEL 9 bootc base: it bakes one identity into the image and nothing
/// regenerates it. This text says what holds on both RHEL 9 and RHEL 10
/// until the member's notice itself is corrected.
const NVME_HOST_ID_NOTICE: &str = "the RHEL 9 bootc base image ships its own \
     /etc/nvme/hostnqn and hostid, so every machine built from it shares one NVMe host \
     identity until you delete both files in the Containerfile or write new ones at \
     provisioning. RHEL 10 derives it from each machine's firmware UUID";

/// One machine-state row, from any of the snapshot's three class lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassRow<'a> {
    pub path: &'a str,
    pub member: MachineStateMember,
    /// The snapshot holds the file's content: a carriable member entry.
    pub content_captured: bool,
    /// An operator override carries it into the image.
    pub included: bool,
    pub mode: Option<u32>,
    pub size: Option<u64>,
}

/// Every class row: each `MachineState` config entry, each
/// `config.withheld` row and each `unmanaged_files.withheld` row, sorted
/// by path.
pub fn class_rows(snap: &InspectionSnapshot) -> Vec<ClassRow<'_>> {
    let mut rows = Vec::new();
    if let Some(config) = &snap.config {
        for entry in &config.files {
            if let FindingKind::MachineState { member, include } = &entry.disposition {
                rows.push(ClassRow {
                    path: &entry.path,
                    member: MachineStateMember::Carriable(*member),
                    content_captured: true,
                    included: *include,
                    mode: None,
                    size: None,
                });
            }
        }
        rows.extend(config.withheld.iter().map(|w| ClassRow {
            path: &w.path,
            member: w.member,
            content_captured: false,
            included: false,
            mode: w.mode,
            size: w.size,
        }));
    }
    if let Some(unmanaged) = &snap.unmanaged_files {
        rows.extend(unmanaged.withheld.iter().map(|w| ClassRow {
            path: &w.path,
            member: w.member,
            content_captured: false,
            included: false,
            mode: w.mode,
            size: w.size,
        }));
    }
    rows.sort_by(|a, b| a.path.cmp(b.path));
    rows
}

/// The owed provisioning steps of the rows not overridden, deduplicated
/// by step, each with its paths, in [`ProvisioningStep`] declaration
/// order. Automatic and `None` notices are not steps, and an overridden
/// row owes nothing.
pub fn provisioning_steps(snap: &InspectionSnapshot) -> Vec<(ProvisioningStep, Vec<&str>)> {
    let mut steps: BTreeMap<ProvisioningStep, Vec<&str>> = BTreeMap::new();
    for row in class_rows(snap) {
        if row.included {
            continue;
        }
        if let ProvisioningNotice::Owed { step } = row.member.notice() {
            steps.entry(step).or_default().push(row.path);
        }
    }
    steps.into_iter().collect()
}

/// How many class rows left out of the image owe no step: recreated
/// without operator action, or needed by nothing.
pub fn automatic_count(snap: &InspectionSnapshot) -> usize {
    class_rows(snap)
        .iter()
        .filter(|r| !r.included && !matches!(r.member.notice(), ProvisioningNotice::Owed { .. }))
        .count()
}

/// How many class rows the export leaves out.
pub fn left_out_count(snap: &InspectionSnapshot) -> usize {
    class_rows(snap).iter().filter(|r| !r.included).count()
}

/// The `notice` the export header and the scan's stderr carry when a
/// provisioning step is owed: a count of the class rows left out and the
/// steps, enum text only. None when nothing is owed: files recreated
/// without operator action need no notice.
pub fn left_out_notice(snap: &InspectionSnapshot) -> Option<ExportWarning> {
    let steps: Vec<&str> = provisioning_steps(snap)
        .iter()
        .map(|(step, _)| leading_command(*step))
        .collect();
    if steps.is_empty() {
        return None;
    }
    let left_out = left_out_count(snap);
    let files = if left_out == 1 { "file" } else { "files" };
    Some(ExportWarning::notice(format!(
        "{left_out} machine-state {files} left out. At provisioning: {}.",
        steps.join(", ")
    )))
}

/// A step's leading command alone, for the one-line notice.
fn leading_command(step: ProvisioningStep) -> &'static str {
    let short = step.short();
    short.split(" (").next().unwrap_or(short)
}

/// One `notice` per `warn` member an operator included: every machine
/// built from the image shares that identity.
pub fn warn_override_notices(snap: &InspectionSnapshot) -> Vec<ExportWarning> {
    class_rows(snap)
        .iter()
        .filter(|r| r.included && r.member.override_level() == OverrideLevel::Warn)
        .map(|r| {
            ExportWarning::notice(format!(
                "Host identity included in the image: {}",
                header_path(r.path)
            ))
        })
        .collect()
}

/// A path bound for the warnings header: control characters (a newline
/// would be header injection) become `?`, and it is cut to
/// [`NOTICE_PATH_MAX`] bytes on a character boundary, ending in `...`.
fn header_path(path: &str) -> String {
    const ELLIPSIS: &str = "...";
    let clean: String = path
        .chars()
        .map(|c| {
            if c.is_control() || !c.is_ascii() {
                '?'
            } else {
                c
            }
        })
        .collect();
    if clean.len() <= NOTICE_PATH_MAX {
        return clean;
    }
    format!("{}{ELLIPSIS}", &clean[..NOTICE_PATH_MAX - ELLIPSIS.len()])
}

/// The `notice` the export header and the scan's stderr carry when the
/// kargs file leaves out kernel arguments whose values were redacted: a
/// count and the install-time step, fixed text only. None when nothing
/// is owed.
pub fn owed_kernel_arguments_notice(snap: &InspectionSnapshot) -> Option<ExportWarning> {
    let owed = owed_kernel_arguments(snap).len();
    if owed == 0 {
        return None;
    }
    let args = if owed == 1 { "argument" } else { "arguments" };
    Some(ExportWarning::notice(format!(
        "{owed} kernel {args} left out: the value was redacted. At install time: \
         {KARGS_INSTALL_STEP}."
    )))
}

/// Every class notice the export header carries, in header order: the
/// left-out notice, the owed kernel arguments, then one per `warn`
/// override.
pub fn export_notices(snap: &InspectionSnapshot) -> Vec<ExportWarning> {
    left_out_notice(snap)
        .into_iter()
        .chain(owed_kernel_arguments_notice(snap))
        .chain(warn_override_notices(snap))
        .collect()
}

/// The Containerfile's `Provisioning` section body: a note a person would
/// leave, with the steps and no paths (the audit report has the paths),
/// then each kernel argument owed at install time. Empty when nothing is
/// owed.
pub fn containerfile_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let steps = provisioning_steps(snap);
    let mut lines = Vec::new();
    if !steps.is_empty() {
        lines.push("# Not in the image: this host's identity. At provisioning:".to_string());
        lines.extend(
            steps
                .iter()
                .map(|(step, _)| format!("#   {}", escape_comment_text(step.short()))),
        );
    }
    let kargs = owed_kernel_arguments(snap);
    if !kargs.is_empty() {
        lines.push(
            "# Not in the image: kernel arguments whose values were redacted. Pass each at \
             install time, for example:"
                .to_string(),
        );
        lines.extend(kargs.iter().map(|karg| {
            format!(
                "#   {}",
                escape_comment_text(&format!("{KARGS_INSTALL_STEP} {karg}"))
            )
        }));
    }
    lines
}

/// The `## Provisioning steps` section the audit report and the README
/// carry: one bullet per step with the paths it covers, then each kernel
/// argument owed at install time, then the count of files recreated
/// without a step. Empty when no step is owed.
pub fn provisioning_markdown(snap: &InspectionSnapshot) -> Vec<String> {
    let steps = provisioning_steps(snap);
    let kargs = owed_kernel_arguments(snap);
    if steps.is_empty() && kargs.is_empty() {
        return Vec::new();
    }
    let mut lines = vec!["## Provisioning steps".to_string(), String::new()];
    if !steps.is_empty() {
        lines.push(
            "These files belong to this host, so the image leaves them out. Run each step \
             when you deploy a machine from the image:"
                .to_string(),
        );
        lines.push(String::new());
        for (step, paths) in &steps {
            let paths: Vec<String> = paths.iter().map(|p| md_code(p)).collect();
            lines.push(format!(
                "- {}: {}",
                md_code(step.command()),
                paths.join(", ")
            ));
        }
        lines.push(String::new());
    }
    if !kargs.is_empty() {
        lines.push(format!(
            "These kernel arguments had their values redacted, so the image's kernel arguments \
             file leaves them out. Pass each with its real value at install time, for example \
             with {}:",
            md_code(KARGS_INSTALL_STEP)
        ));
        lines.push(String::new());
        lines.extend(kargs.iter().map(|karg| format!("- {}", md_code(karg))));
        lines.push(String::new());
    }
    let automatic = automatic_count(snap);
    if !steps.is_empty() && automatic > 0 {
        let files = if automatic == 1 {
            "file is"
        } else {
            "files are"
        };
        lines.push(format!(
            "{automatic} other machine-state {files} recreated without operator action."
        ));
        lines.push(String::new());
    }
    lines
}

/// The audit report's "Machine state" table under Configuration: every
/// class row with its reason and notice, and for a row whose content the
/// snapshot does not hold, its mode and size.
pub fn audit_table(snap: &InspectionSnapshot) -> Vec<String> {
    let rows = class_rows(snap);
    if rows.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![
        format!("#### Machine state ({})", rows.len()),
        String::new(),
        "| Path | Reason | Notice | File |".to_string(),
        "|------|--------|--------|------|".to_string(),
    ];
    for row in rows {
        let reason = match row.member.reason() {
            inspectah_core::types::machine_state::MachineStateReason::HostIdentity => {
                "host identity"
            }
            inspectah_core::types::machine_state::MachineStateReason::HostCredential => {
                "host credential"
            }
            inspectah_core::types::machine_state::MachineStateReason::Generated => "generated",
        };
        let notice = if row.included {
            "included in the image by override".to_string()
        } else if let Some(text) = member_notice(row.member) {
            text.to_string()
        } else {
            notice_text(row.member.notice())
        };
        let file = if row.content_captured {
            "contents captured".to_string()
        } else {
            let mode = row
                .mode
                .map_or("mode unknown".to_string(), |m| format!("mode {m:04o}"));
            let size = row
                .size
                .map_or("size unknown".to_string(), |s| format!("{s} bytes"));
            format!("{mode}, {size}, contents not captured")
        };
        lines.push(format!(
            "| {} | {reason} | {notice} | {file} |",
            md_code_cell(row.path)
        ));
    }
    lines.push(String::new());
    lines
}

/// A notice as one line of prose.
/// The member's own notice, in place of its [`ProvisioningNotice`]'s
/// generic line, when that line is wrong for it. The audit report prints
/// it and refine's view sends it, so the two say the same.
pub fn member_notice(member: MachineStateMember) -> Option<&'static str> {
    (member == MachineStateMember::Carriable(CarriableMember::NvmeHostId))
        .then_some(NVME_HOST_ID_NOTICE)
}

pub fn notice_text(notice: ProvisioningNotice) -> String {
    use inspectah_core::types::machine_state::RecreatedBy;
    match notice {
        ProvisioningNotice::Owed { step } => format!("at provisioning: {}", step.command()),
        ProvisioningNotice::Automatic {
            by: RecreatedBy::FirstBoot,
        } => "recreated at first boot".to_string(),
        ProvisioningNotice::Automatic {
            by: RecreatedBy::Provisioning,
        } => "set at provisioning by the installer, cloud-init or DHCP".to_string(),
        ProvisioningNotice::Automatic {
            by: RecreatedBy::CarriedSetting,
        } => "rewritten from a setting the image carries".to_string(),
        ProvisioningNotice::None => "not needed in the image".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
    use inspectah_core::types::machine_state::{CarriableMember, WithheldConfigEntry};

    fn withheld(path: &str, member: MachineStateMember) -> WithheldConfigEntry {
        WithheldConfigEntry {
            path: path.into(),
            kind: ConfigFileKind::Unowned,
            package: None,
            member,
            mode: Some(0o600),
            size: Some(82),
            aggregate: None,
        }
    }

    fn snap() -> InspectionSnapshot {
        InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![ConfigFileEntry {
                    path: "/etc/iscsi/initiatorname.iscsi".into(),
                    disposition: FindingKind::MachineState {
                        member: CarriableMember::IscsiInitiatorName,
                        include: false,
                    },
                    ..Default::default()
                }],
                withheld: vec![
                    withheld("/etc/krb5.keytab", MachineStateMember::KerberosKeytab),
                    withheld("/etc/krb5.keytab.ad", MachineStateMember::KerberosKeytab),
                    withheld(
                        "/etc/insights-client/machine-id",
                        MachineStateMember::InsightsState,
                    ),
                    withheld("/etc/machine-id", MachineStateMember::MachineId),
                ],
            }),
            ..InspectionSnapshot::new()
        }
    }

    #[test]
    fn steps_are_deduplicated_in_declaration_order() {
        let snap = snap();
        let steps = provisioning_steps(&snap);
        assert_eq!(
            steps,
            vec![
                (
                    ProvisioningStep::RealmJoin,
                    vec!["/etc/krb5.keytab", "/etc/krb5.keytab.ad"]
                ),
                (
                    ProvisioningStep::InsightsRegister,
                    vec!["/etc/insights-client/machine-id"]
                ),
            ]
        );
        assert_eq!(automatic_count(&snap), 2);
        assert_eq!(left_out_count(&snap), 5);
        assert_eq!(
            left_out_notice(&snap).unwrap().text(),
            "5 machine-state files left out. At provisioning: realm join, insights-client \
             --register."
        );
        assert_eq!(
            containerfile_lines(&snap),
            vec![
                "# Not in the image: this host's identity. At provisioning:",
                "#   realm join (or ipa-client-install, adcli join)",
                "#   insights-client --register (or rhc connect)",
            ]
        );
    }

    #[test]
    fn an_overridden_warn_row_owes_nothing_and_says_so() {
        let mut snap = snap();
        let entry = &mut snap.config.as_mut().unwrap().files[0];
        entry.disposition = entry.disposition.with_include(true);
        assert_eq!(left_out_count(&snap), 4);
        let notices = warn_override_notices(&snap);
        assert_eq!(notices.len(), 1);
        assert_eq!(
            notices[0].text(),
            "Host identity included in the image: /etc/iscsi/initiatorname.iscsi"
        );
    }

    /// A kernel argument whose value was redacted is owed at install time
    /// on every surface the machine-state steps reach, alone or beside
    /// them, by its name with the value shown as `<value>`.
    #[test]
    fn a_redacted_kernel_argument_is_an_owed_step_everywhere() {
        use inspectah_core::types::kernelboot::KernelBootSection;
        let with_cmdline = |mut snap: InspectionSnapshot| {
            snap.kernel_boot = Some(KernelBootSection {
                cmdline: "ro nosmt=force rd.iscsi.password=REDACTED_PASSWORD_1".into(),
                ..Default::default()
            });
            snap
        };
        let owed_line = "#   bootc install to-disk --karg rd.iscsi.password=<value>";
        let notice = "1 kernel argument left out: the value was redacted. At install time: \
                      bootc install to-disk --karg.";

        // Alone: the section exists for the kernel argument only.
        let alone = with_cmdline(InspectionSnapshot::new());
        let md = provisioning_markdown(&alone).join("\n");
        assert!(md.starts_with("## Provisioning steps"), "{md}");
        assert!(md.contains("- `rd.iscsi.password=<value>`"), "{md}");
        assert!(!md.contains("These files belong to this host"), "{md}");
        assert!(!md.contains("REDACTED"), "{md}");
        let cf = containerfile_lines(&alone);
        assert_eq!(cf.len(), 2, "{cf:?}");
        assert_eq!(cf[1], owed_line);
        let notices: Vec<String> = export_notices(&alone)
            .iter()
            .map(|n| n.text().to_owned())
            .collect();
        assert_eq!(notices, vec![notice.to_owned()]);

        // Beside the machine-state steps: both, machine state first.
        let both = with_cmdline(snap());
        let md = provisioning_markdown(&both).join("\n");
        let realm = md.find("realm join").expect("the realm step");
        let karg = md.find("rd.iscsi.password=<value>").expect("the karg");
        assert!(realm < karg, "{md}");
        assert!(md.contains("recreated without operator action"), "{md}");
        let cf = containerfile_lines(&both);
        assert_eq!(cf.last().map(String::as_str), Some(owed_line), "{cf:?}");
        let notices: Vec<String> = export_notices(&both)
            .iter()
            .map(|n| n.text().to_owned())
            .collect();
        assert_eq!(notices.len(), 2, "{notices:?}");
        assert_eq!(notices[1], notice);
    }

    /// Nothing recreates the NVMe host id on a RHEL 9 bootc host, so its
    /// row says what the base image does instead; other first-boot rows
    /// keep their text.
    #[test]
    fn the_nvme_host_id_row_does_not_claim_first_boot_recreates_it() {
        let mut snap = snap();
        snap.config.as_mut().unwrap().files.push(ConfigFileEntry {
            path: "/etc/nvme/hostnqn".into(),
            disposition: FindingKind::MachineState {
                member: CarriableMember::NvmeHostId,
                include: false,
            },
            ..Default::default()
        });
        let table = audit_table(&snap);
        let row = |path: &str| {
            table
                .iter()
                .find(|l| l.contains(path))
                .unwrap_or_else(|| panic!("{path}: {table:?}"))
                .clone()
        };
        let nvme = row("/etc/nvme/hostnqn");
        assert!(nvme.contains(NVME_HOST_ID_NOTICE), "{nvme}");
        assert!(!nvme.contains("first boot"), "{nvme}");
        assert!(!nvme.contains('\u{2014}'), "{nvme}");
        assert!(row("/etc/machine-id").contains("recreated at first boot"));
    }

    #[test]
    fn nothing_owed_and_nothing_left_out_writes_nothing() {
        let snap = InspectionSnapshot::new();
        assert!(provisioning_markdown(&snap).is_empty());
        assert!(containerfile_lines(&snap).is_empty());
        assert!(export_notices(&snap).is_empty());
        assert!(audit_table(&snap).is_empty());
    }
}
