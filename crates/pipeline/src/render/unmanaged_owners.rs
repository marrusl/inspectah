//! Carried unmanaged files whose host owner, group or special mode bits
//! the export does not keep.
//!
//! The export carries every unmanaged payload as `root:root`: `COPY` has
//! no `--chown`, and the tarball's members are owned by uid 0. The
//! bundler also keeps only the permission bits ([`PAYLOAD_MODE_BITS`]),
//! dropping setuid, setgid and sticky. Until ownership is carried, the
//! audit report names each file that loses something by this, with what
//! the host had, so the operator can fix it by hand. The Containerfile's
//! unmanaged block and the scan's output give the count and point at
//! the audit report.
//!
//! Only regular files are named. A symlink's mode is meaningless and the
//! export recreates it with `COPY` or `ln -sfn`; a directory's owner and
//! mode are not recorded by the scan, which records files only.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::nonrpm::{FileType, UnmanagedFile};
use std::collections::{HashMap, HashSet};

use super::configtree::unmanaged_file_withheld;
use super::md_literal::md_code_cell;
use super::unmanaged::{PAYLOAD_MODE_BITS, copied_files};
use crate::export_warning::ExportWarning;

/// Setuid, setgid and sticky: the mode bits [`PAYLOAD_MODE_BITS`] drops.
const SPECIAL_MODE_BITS: u32 = 0o7000;

/// The uid and gid of `root`, which the export gives every payload.
const ROOT_ID: u32 = 0;

/// What happened to the named files and what to do about them, as plain
/// text, so the audit report and any later view say the same thing.
const LEAD_IN: &str = "These files are carried as root:root, without setuid, setgid or \
     sticky, though the host gave them another owner or group, or one of those bits. \
     Fix by hand any that need their original owner or bits, for example with RUN chown \
     or RUN chmod after the COPY, once the account exists.";

/// Why no directory is named, for the audit report after the rows.
const DIRECTORY_NOTE: &str = "Directories are carried root-owned too. The scan does not \
     record their owner or mode, so none is listed.";

/// [`LEAD_IN`]: what the export did to the named files and what to do.
pub fn lead_in_text() -> &'static str {
    LEAD_IN
}

/// One carried unmanaged file whose host owner, group or special bits
/// the export drops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipLoss {
    /// The file's path on the host and in the image.
    pub path: String,
    /// The owner's uid on the host.
    pub uid: u32,
    /// The owner's name, when the snapshot records an account with `uid`.
    pub owner: Option<String>,
    /// The group's gid on the host.
    pub gid: u32,
    /// The group's name, when the snapshot records a group with `gid`.
    pub group: Option<String>,
    /// The host mode, special bits included; `None` when the scan's
    /// record of it cannot be read.
    pub host_mode: Option<u32>,
    /// Whether a `COPY` line of the Containerfile carries the file. One
    /// under `/srv`, or one whose `COPY` the Containerfile refuses, only
    /// the export archive carries.
    pub copied: bool,
}

impl OwnershipLoss {
    /// The mode the export gives the file: the host's permission bits.
    pub fn carried_mode(&self) -> Option<u32> {
        self.host_mode.map(|m| m & PAYLOAD_MODE_BITS)
    }
}

/// The account names the snapshot records, by id. Only local accounts
/// are recorded, so a system account's id has no name here.
struct AccountNames {
    users: HashMap<u64, String>,
    groups: HashMap<u64, String>,
}

impl AccountNames {
    fn of(snap: &InspectionSnapshot) -> Self {
        let by_id = |rows: &[serde_json::Value], id: &str| -> HashMap<u64, String> {
            rows.iter()
                .filter_map(|r| {
                    let id = r.get(id)?.as_u64()?;
                    let name = r.get("name")?.as_str()?;
                    Some((id, name.to_string()))
                })
                .collect()
        };
        snap.users_groups
            .as_ref()
            .map(|ug| Self {
                users: by_id(&ug.users, "uid"),
                groups: by_id(&ug.groups, "gid"),
            })
            .unwrap_or_else(|| Self {
                users: HashMap::new(),
                groups: HashMap::new(),
            })
    }

    fn name(map: &HashMap<u64, String>, id: u32) -> Option<String> {
        if id == ROOT_ID {
            return Some("root".into());
        }
        map.get(&u64::from(id)).cloned()
    }
}

/// The host mode the scan recorded, such as `04750` or `0644`.
fn parse_mode(permissions: &str) -> Option<u32> {
    u32::from_str_radix(permissions, 8).ok()
}

/// Whether `file` is a payload the export carries: included, not
/// withheld, and not a symlink.
fn is_carried_file(file: &UnmanagedFile) -> bool {
    file.disposition.is_included()
        && unmanaged_file_withheld(file).is_none()
        && file.file_type != FileType::Symlink
}

/// Every carried unmanaged file whose host owner or group is not root,
/// or whose host mode has setuid, setgid or sticky, in snapshot order.
pub fn ownership_losses(snap: &InspectionSnapshot) -> Vec<OwnershipLoss> {
    let Some(section) = &snap.unmanaged_files else {
        return Vec::new();
    };
    let names = AccountNames::of(snap);
    let copied: HashSet<&str> = copied_files(snap)
        .into_iter()
        .map(|f| f.path.as_str())
        .collect();
    section
        .items
        .iter()
        .filter(|f| is_carried_file(f))
        .filter_map(|f| {
            let p = &f.provenance;
            let host_mode = parse_mode(&p.permissions);
            let special = host_mode.is_some_and(|m| m & SPECIAL_MODE_BITS != 0);
            (p.uid != ROOT_ID || p.gid != ROOT_ID || special).then(|| OwnershipLoss {
                path: f.path.clone(),
                uid: p.uid,
                owner: AccountNames::name(&names.users, p.uid),
                gid: p.gid,
                group: AccountNames::name(&names.groups, p.gid),
                host_mode,
                copied: copied.contains(f.path.as_str()),
            })
        })
        .collect()
}

/// `name (id)`, or the bare id when the snapshot has no name for it.
fn account_cell(name: Option<&str>, id: u32) -> String {
    match name {
        Some(name) => format!("{} ({id})", md_code_cell(name)),
        None => id.to_string(),
    }
}

/// A mode as four octal digits, in code.
fn mode_cell(mode: Option<u32>) -> String {
    mode.map_or_else(|| "unknown".into(), |m| format!("`{m:04o}`"))
}

/// The audit report subsection naming each file, under Software. Empty
/// when there is none.
pub(crate) fn audit_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let losses = ownership_losses(snap);
    if losses.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![
        format!(
            "### Unmanaged Files That Lose Their Owner or Special Bits ({})",
            losses.len()
        ),
        String::new(),
        LEAD_IN.into(),
        String::new(),
        "| Path | Host owner | Host group | Host mode | Carried as |".into(),
        "|------|------------|------------|-----------|------------|".into(),
    ];
    for loss in &losses {
        let carried = match loss.carried_mode() {
            Some(m) => format!("`root:root` `{m:04o}`"),
            None => "`root:root`".into(),
        };
        lines.push(format!(
            "| {} | {} | {} | {} | {carried} |",
            md_code_cell(&loss.path),
            account_cell(loss.owner.as_deref(), loss.uid),
            account_cell(loss.group.as_deref(), loss.gid),
            mode_cell(loss.host_mode),
        ));
    }
    lines.push(String::new());
    lines.push(DIRECTORY_NOTE.into());
    lines.push(String::new());
    lines
}

/// The Containerfile comment giving the count of files that lose
/// something and that one of its `COPY` lines carries, for the unmanaged
/// block. Empty when there is none. A file under `/srv`, or one whose
/// `COPY` is refused with a FIXME, is not copied, so it is not counted
/// here.
pub(crate) fn containerfile_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let copied = ownership_losses(snap).iter().filter(|l| l.copied).count();
    if copied == 0 {
        return Vec::new();
    }
    vec![
        format!("# {copied} of the files the COPY lines below carry had a non-root owner or"),
        "# group, or setuid, setgid or sticky, on the host. The image gets them as".into(),
        "# root:root without those bits; audit-report.md lists each to fix by hand.".into(),
    ]
}

/// The scan's notice that carried unmanaged files lose their owner or
/// special bits, by count, pointing at `audit-report.md`; `None` when no
/// file does.
pub fn ownership_notice(snap: &InspectionSnapshot) -> Option<ExportWarning> {
    let n = ownership_losses(snap).len();
    (n > 0).then(|| {
        let noun = if n == 1 { "file is" } else { "files are" };
        ExportWarning::notice(format!(
            "{n} unmanaged {noun} carried as root:root without the host's owner, \
             group or setuid, setgid or sticky bits; see audit-report.md."
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::finding::FindingKind;
    use inspectah_core::types::nonrpm::{
        PayloadRedaction, ProvenanceSignals, UncheckedPayload, UnmanagedFileSection,
    };
    use inspectah_core::types::users::UserGroupSection;

    fn file(path: &str, uid: u32, gid: u32, permissions: &str) -> UnmanagedFile {
        UnmanagedFile {
            path: path.into(),
            file_type: FileType::DataFile,
            provenance: ProvenanceSignals {
                uid,
                gid,
                permissions: permissions.into(),
                ..Default::default()
            },
            disposition: FindingKind::included(),
            ..Default::default()
        }
    }

    fn snapshot(items: Vec<UnmanagedFile>) -> InspectionSnapshot {
        InspectionSnapshot {
            unmanaged_files: Some(UnmanagedFileSection {
                items,
                ..Default::default()
            }),
            users_groups: Some(UserGroupSection {
                users: vec![serde_json::json!({"name": "appuser", "uid": 1001, "gid": 1001})],
                groups: vec![serde_json::json!({"name": "appgroup", "gid": 1001})],
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Which files are named, and how each row reads.
    #[test]
    fn each_lost_owner_or_special_bit_is_one_row() {
        let cases: &[(&str, UnmanagedFile, Option<&str>)] = &[
            (
                "non-root owner, name resolves",
                file("/opt/app/run", 1001, 0, "0755"),
                Some(
                    "| `/opt/app/run` | `appuser` (1001) | `root` (0) | `0755` | `root:root` `0755` |",
                ),
            ),
            (
                "non-root group only",
                file("/opt/app/conf", 0, 1001, "0640"),
                Some(
                    "| `/opt/app/conf` | `root` (0) | `appgroup` (1001) | `0640` | `root:root` `0640` |",
                ),
            ),
            (
                "setuid on a root-owned file",
                file("/usr/local/bin/helper", 0, 0, "04755"),
                Some(
                    "| `/usr/local/bin/helper` | `root` (0) | `root` (0) | `4755` | `root:root` `0755` |",
                ),
            ),
            (
                "setgid, uid with no recorded name",
                file("/opt/vendor/bin/mail", 985, 12, "02755"),
                Some("| `/opt/vendor/bin/mail` | 985 | 12 | `2755` | `root:root` `0755` |"),
            ),
            (
                "sticky",
                file("/opt/app/spool", 0, 0, "01777"),
                Some(
                    "| `/opt/app/spool` | `root` (0) | `root` (0) | `1777` | `root:root` `0777` |",
                ),
            ),
            (
                "unreadable mode with a non-root owner",
                file("/opt/app/odd", 1001, 1001, ""),
                Some(
                    "| `/opt/app/odd` | `appuser` (1001) | `appgroup` (1001) | unknown | `root:root` |",
                ),
            ),
            (
                "root:root 0644",
                file("/opt/app/README", 0, 0, "0644"),
                None,
            ),
            (
                "root:root 0755",
                file("/opt/app/bin/tool", 0, 0, "0755"),
                None,
            ),
        ];
        for (name, f, row) in cases {
            let md = audit_lines(&snapshot(vec![f.clone()])).join("\n");
            match row {
                Some(row) => {
                    assert!(md.contains(row), "{name}: {md}");
                    assert!(md.contains(LEAD_IN), "{name}: {md}");
                    assert!(md.contains("(1)"), "{name}: {md}");
                }
                None => assert!(md.is_empty(), "{name}: {md}"),
            }
        }
    }

    /// Files the export does not carry are not named, nor symlinks.
    #[test]
    fn only_carried_regular_files_are_named() {
        let excluded = UnmanagedFile {
            disposition: FindingKind::excluded(),
            ..file("/opt/app/excluded", 1001, 1001, "0755")
        };
        let withheld = UnmanagedFile {
            payload_redaction: PayloadRedaction::Withheld(UncheckedPayload::TooLarge),
            ..file("/opt/app/withheld", 1001, 1001, "0644")
        };
        let link = UnmanagedFile {
            file_type: FileType::Symlink,
            link_target: "run".into(),
            ..file("/opt/app/link", 1001, 1001, "0777")
        };
        let snap = snapshot(vec![excluded, withheld, link]);
        assert!(ownership_losses(&snap).is_empty());
        assert!(audit_lines(&snap).is_empty());
        assert!(containerfile_lines(&snap).is_empty());
        assert!(ownership_notice(&snap).is_none());
    }

    /// The Containerfile counts the files it copies; `/srv` files are
    /// named in the audit report and the scan's notice, but not copied.
    #[test]
    fn the_containerfile_counts_copied_files_only() {
        let snap = snapshot(vec![
            file("/opt/app/run", 1001, 1001, "0755"),
            file("/opt/app/helper", 0, 0, "04755"),
            file("/srv/www/index.html", 1001, 1001, "0644"),
        ]);
        assert_eq!(ownership_losses(&snap).len(), 3);
        assert_eq!(
            containerfile_lines(&snap)[0],
            "# 2 of the files the COPY lines below carry had a non-root owner or"
        );
        assert_eq!(
            ownership_notice(&snap).map(|n| n.to_string()).as_deref(),
            Some(
                "Notice: 3 unmanaged files are carried as root:root without the host's owner, \
                 group or setuid, setgid or sticky bits; see audit-report.md."
            )
        );
        let srv_only = snapshot(vec![file("/srv/www/index.html", 1001, 1001, "0644")]);
        assert!(containerfile_lines(&srv_only).is_empty());
        assert!(
            audit_lines(&srv_only)
                .join("\n")
                .contains("/srv/www/index.html")
        );
    }

    /// The Containerfile counts a file only when one of its `COPY` lines
    /// carries it. A refused `COPY` leaves a FIXME, so its files are not
    /// counted; a directory `COPY` carries a file whose own path would be
    /// refused. The audit report and the scan's notice name every file.
    #[test]
    fn the_containerfile_counts_what_its_copy_lines_carry() {
        let cases: &[(&str, Vec<UnmanagedFile>, usize)] = &[
            (
                "a single-file COPY refused for whitespace",
                vec![file("/opt/app/config file", 1001, 1001, "0644")],
                0,
            ),
            (
                "a directory COPY refused for whitespace",
                vec![
                    file("/opt/my app/run", 1001, 1001, "0755"),
                    file("/opt/my app/conf", 0, 1001, "0640"),
                ],
                0,
            ),
            (
                "a directory COPY carrying a name refused on its own",
                vec![
                    file("/opt/app/config file", 1001, 1001, "0644"),
                    file("/opt/app/README", 0, 0, "0644"),
                ],
                1,
            ),
            (
                "an ancestor directory COPY carrying a nested file refused on its own",
                vec![
                    file("/opt/app/a", 0, 0, "0644"),
                    file("/opt/app/b", 0, 0, "0644"),
                    file("/opt/app/sub/config file", 1001, 1001, "0644"),
                ],
                1,
            ),
            (
                "an ancestor directory COPY carrying a refused nested directory",
                vec![
                    file("/opt/app/a", 0, 0, "0644"),
                    file("/opt/app/b", 0, 0, "0644"),
                    file("/opt/app/my dir/x", 1001, 1001, "0644"),
                    file("/opt/app/my dir/y", 1001, 1001, "0644"),
                ],
                2,
            ),
            (
                "a refused ancestor directory COPY carries nothing nested",
                vec![
                    file("/opt/my app/a", 0, 0, "0644"),
                    file("/opt/my app/b", 0, 0, "0644"),
                    file("/opt/my app/sub/config", 1001, 1001, "0644"),
                ],
                0,
            ),
            (
                "a directory COPY does not carry a sibling sharing its name prefix",
                vec![
                    file("/opt/app/a", 0, 0, "0644"),
                    file("/opt/app/b", 0, 0, "0644"),
                    file("/opt/app2/config file", 1001, 1001, "0644"),
                ],
                0,
            ),
        ];
        let mut wrong = Vec::new();
        for (name, items, copied) in cases {
            let snap = snapshot(items.clone());
            let named = items
                .iter()
                .filter(|f| f.provenance.uid != 0 || f.provenance.gid != 0)
                .count();
            let lines = containerfile_lines(&snap);
            let want = (*copied > 0).then(|| {
                format!(
                    "# {copied} of the files the COPY lines below carry had a non-root owner or"
                )
            });
            if lines.first() != want.as_ref() {
                wrong.push(format!("{name}: {:?}", lines.first()));
            }
            assert_eq!(ownership_losses(&snap).len(), named, "{name}");
            assert!(
                ownership_notice(&snap)
                    .is_some_and(|n| n.to_string().starts_with(&format!("Notice: {named} "))),
                "{name}"
            );
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// The audit report and the Containerfile gain the notice and nothing
    /// else: the same snapshot with the file root-owned and without
    /// special bits renders exactly as the report did before the notice.
    #[test]
    fn the_notice_is_the_only_difference_in_both_outputs() {
        // The root-owned `/srv` file gives both reports a Software section.
        let srv = || file("/srv/www/index.html", 0, 0, "0644");
        let lost = snapshot(vec![file("/opt/app/run", 1001, 1001, "04755"), srv()]);
        let kept = snapshot(vec![file("/opt/app/run", 0, 0, "0755"), srv()]);

        let audit = crate::render::audit::render_audit(&lost);
        let section = audit_lines(&lost).join("\n");
        assert!(audit.contains("## Software"), "{audit}");
        assert!(audit.contains(&section), "{audit}");
        assert!(
            audit.contains(
                "| `/opt/app/run` | `appuser` (1001) | `appgroup` (1001) | `4755` | `root:root` `0755` |"
            ),
            "{audit}"
        );
        assert_eq!(
            audit.replacen(&format!("\n{section}"), "", 1),
            crate::render::audit::render_audit(&kept)
        );

        let mut copied = crate::render::unmanaged::unmanaged_file_lines(&lost);
        let notice = containerfile_lines(&lost);
        assert_eq!(notice.len(), 3);
        let at = copied
            .iter()
            .position(|l| *l == notice[0])
            .expect("the count line is in the unmanaged block");
        copied.drain(at..at + notice.len());
        assert_eq!(
            copied,
            crate::render::unmanaged::unmanaged_file_lines(&kept)
        );
    }

    /// A host path is code in its cell, so table syntax in it stays text.
    #[test]
    fn a_path_with_a_pipe_stays_in_its_cell() {
        let md = audit_lines(&snapshot(vec![file("/opt/a|b", 1001, 0, "0644")])).join("\n");
        assert!(md.contains("| `/opt/a\\|b` |"), "{md}");
    }
}
