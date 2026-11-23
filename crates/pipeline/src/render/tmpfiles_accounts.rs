//! Carried tmpfiles.d lines that name an account the image will not have.
//!
//! `systemd-tmpfiles` skips a line whose user or group it cannot resolve,
//! and `systemd-tmpfiles-setup.service` accepts its exit status as
//! success, so whatever the line does (create a path, set ownership on
//! one) silently does not happen on a deployed host. The export does not
//! change such a line; it names it, in the Containerfile next to the
//! config `COPY` and in both audit reports, and refine carries the same
//! rows, so the operator can create the account or provision it.
//!
//! An account is named only when the export is sure the image lacks it:
//! the scan recorded it as a local (non-system) account on the host, the
//! users block does not create it, and no carried or base-image package
//! is known to create it. A name the scan did not record is a system
//! account the base or a package may provide, which the export cannot
//! check, so it is not named. A numeric id and `-` are never named:
//! tmpfiles resolves neither.
//!
//! Lines are read through the shared [`TmpfilesEntry`] parser, which
//! unquotes and unescapes fields as systemd does, so a quoted path holding
//! a space is one field and the owner is the field systemd resolves.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::storage::{ETC_TMPFILES_PREFIX, TmpfilesEntry, USR_TMPFILES_PREFIX};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::configtree::staged_writes;
use super::containerfile::rpm_provided_accounts;
use super::language_packages::sanitize_for_comment;
use super::md_literal::md_code;
use super::users::ContainerfileUserPlan;

/// The tmpfiles.d directories a carried file can land in and systemd
/// reads at boot.
const CARRIED_TMPFILES_DIRS: [&str; 2] = [ETC_TMPFILES_PREFIX, USR_TMPFILES_PREFIX];

/// The only filename suffix systemd reads in a tmpfiles.d directory.
const TMPFILES_SUFFIX: &str = ".conf";

/// What the operator does about a named line, one Containerfile comment
/// line each. The audit reports join them into one paragraph.
const REMEDY: [&str; 3] = [
    "Create each missing user or group (useradd in refine) or provision it, so",
    "its line can run. Otherwise systemd-tmpfiles skips the line, and",
    "systemd-tmpfiles-setup.service still reports success.",
];

/// [`REMEDY`] as one paragraph, for the audit reports.
pub fn remedy_text() -> String {
    REMEDY.join(" ")
}

/// One carried tmpfiles.d line whose user or group the image lacks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingOwner {
    /// Where the fragment lands in the image.
    pub file: String,
    /// The path the line names.
    pub path: String,
    /// The user field, when the image lacks that user.
    pub user: Option<String>,
    /// The group field, when the image lacks that group.
    pub group: Option<String>,
}

impl MissingOwner {
    /// `user appuser, group appgroup`, or whichever of the two is missing,
    /// each name written by `name`.
    fn accounts(&self, name: impl Fn(&str) -> String) -> String {
        let user = self.user.as_deref().map(|u| format!("user {}", name(u)));
        let group = self.group.as_deref().map(|g| format!("group {}", name(g)));
        user.into_iter().chain(group).collect::<Vec<_>>().join(", ")
    }
}

/// The accounts the export knows the image will have, and the local host
/// accounts it knows the image will not have unless something creates
/// them.
struct ImageAccounts {
    plan: ContainerfileUserPlan,
    package_accounts: HashMap<String, String>,
    replicated_packages: HashSet<String>,
    host_users: HashSet<String>,
    host_groups: HashSet<String>,
}

impl ImageAccounts {
    fn of(snap: &InspectionSnapshot) -> Self {
        // The carried packages and the base image's own, which create
        // their accounts in the image whether or not they are carried.
        let packages = snap
            .rpm
            .as_ref()
            .map(|rpm| {
                let carried = rpm
                    .packages_added
                    .iter()
                    .filter(|p| p.disposition.is_included())
                    .map(|p| p.name.clone());
                let base = rpm.baseline_package_names.iter().flatten().cloned();
                carried.chain(base).collect()
            })
            .unwrap_or_default();
        let (package_accounts, replicated_packages) = rpm_provided_accounts(packages);
        let names = |rows: &[serde_json::Value]| -> HashSet<String> {
            rows.iter()
                .filter_map(|r| r.get("name").and_then(|n| n.as_str()))
                .map(str::to_string)
                .collect()
        };
        let (host_users, host_groups) = snap
            .users_groups
            .as_ref()
            .map(|ug| (names(&ug.users), names(&ug.groups)))
            .unwrap_or_default();
        Self {
            plan: ContainerfileUserPlan::from_snapshot(snap),
            package_accounts,
            replicated_packages,
            host_users,
            host_groups,
        }
    }

    /// A carried or base-image package is known to create the account
    /// `name`.
    fn package_creates(&self, name: &str) -> bool {
        self.package_accounts.contains_key(name) || self.replicated_packages.contains(name)
    }

    /// `field` is a user the image lacks.
    fn missing_user<'a>(&self, field: Option<&'a str>) -> Option<&'a str> {
        let name = named_account(field)?;
        (self.host_users.contains(name)
            && !self.plan.creates_user(name)
            && !self.package_creates(name))
        .then_some(name)
    }

    /// `field` is a group the image lacks.
    fn missing_group<'a>(&self, field: Option<&'a str>) -> Option<&'a str> {
        let name = named_account(field)?;
        (self.host_groups.contains(name)
            && self.plan.guaranteed_gid(name).is_none()
            && !self.package_creates(name))
        .then_some(name)
    }
}

/// The account a user or group field names, or `None` for an absent
/// field, `-`, a numeric id, and a `%` specifier, none of which is a name
/// the image has to hold.
fn named_account(field: Option<&str>) -> Option<&str> {
    let field = field?;
    let is_numeric = field.chars().all(|c| c.is_ascii_digit());
    (field != "-" && !is_numeric && !field.contains('%')).then_some(field)
}

/// Whether the image's systemd reads `dest` as a tmpfiles.d fragment.
fn is_tmpfiles_fragment(dest: &str) -> bool {
    CARRIED_TMPFILES_DIRS.iter().any(|dir| {
        dest.strip_prefix(dir)
            .is_some_and(|name| name.ends_with(TMPFILES_SUFFIX) && !name.contains('/'))
    })
}

/// Every carried tmpfiles.d line whose user or group the image lacks, in
/// file order then line order.
pub fn missing_owners(snap: &InspectionSnapshot) -> Vec<MissingOwner> {
    let staged = staged_writes(snap);
    let fragments: Vec<_> = staged
        .writes
        .iter()
        .filter(|(dest, _)| is_tmpfiles_fragment(dest.as_str()))
        .collect();
    if fragments.is_empty() {
        return Vec::new();
    }
    let accounts = ImageAccounts::of(snap);
    let mut out = Vec::new();
    for (dest, write) in fragments {
        for line in write.content.lines() {
            let Some(entry) = TmpfilesEntry::parse(line) else {
                continue;
            };
            let user = accounts.missing_user(entry.user.as_deref());
            let group = accounts.missing_group(entry.group.as_deref());
            if user.is_none() && group.is_none() {
                continue;
            }
            out.push(MissingOwner {
                file: dest.as_str().to_string(),
                path: entry.path,
                user: user.map(str::to_string),
                group: group.map(str::to_string),
            });
        }
    }
    out
}

/// The Containerfile comment that names each line, for the Configuration
/// Files section. Empty when there is none.
pub(crate) fn containerfile_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let missing = missing_owners(snap);
    if missing.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![
        String::new(),
        "# NOTE: these carried tmpfiles.d lines name an account this image does not".into(),
        "# create.".into(),
    ];
    for m in &missing {
        lines.push(format!(
            "#   {}: {} needs {}",
            sanitize_for_comment(&m.file),
            sanitize_for_comment(&m.path),
            m.accounts(sanitize_for_comment)
        ));
    }
    lines.extend(REMEDY.iter().map(|l| format!("# {l}")));
    lines
}

/// The audit report subsection that names each line, under
/// Configuration Files. Empty when there is none.
pub(crate) fn audit_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let missing = missing_owners(snap);
    if missing.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![
        format!(
            "#### tmpfiles.d Lines Naming Accounts the Image Lacks ({})",
            missing.len()
        ),
        String::new(),
        "Each line below names an account the image does not create.".into(),
        String::new(),
    ];
    for m in &missing {
        lines.push(format!(
            "- {}: {} needs {}",
            md_code(&m.file),
            md_code(&m.path),
            m.accounts(md_code)
        ));
    }
    lines.push(String::new());
    lines.push(remedy_text());
    lines.push(String::new());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_name_is_an_account() {
        for (field, named) in [
            (None, None),
            (Some("-"), None),
            (Some("0"), None),
            (Some("1001"), None),
            (Some("%U"), None),
            (Some("appuser"), Some("appuser")),
            (Some("app1"), Some("app1")),
        ] {
            assert_eq!(named_account(field), named, "{field:?}");
        }
    }

    #[test]
    fn only_a_top_level_conf_is_a_fragment() {
        for (dest, fragment) in [
            ("/etc/tmpfiles.d/appone.conf", true),
            ("/usr/lib/tmpfiles.d/appone.conf", true),
            ("/etc/tmpfiles.d/appone.conf.rpmsave", false),
            ("/etc/tmpfiles.d/sub/appone.conf", false),
            ("/run/tmpfiles.d/appone.conf", false),
            ("/etc/appone.conf", false),
        ] {
            assert_eq!(is_tmpfiles_fragment(dest), fragment, "{dest}");
        }
    }
}
