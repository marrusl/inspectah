//! The links on the source host that enable a unit, and the install state
//! they give it (§ 7.1 sources 3 and 4, § 7.3 of the units spec).
//!
//! `systemctl list-unit-files` says `enabled` when it finds any link to a
//! unit, and does not say who made it. The install state is read here
//! instead: the links `enable` would make, from the effective `[Install]`
//! section, against the links under `/etc/systemd/system` and the links a
//! package ships under `/usr/lib/systemd/system`.

use super::install::{DROP_IN_DIR_SUFFIX, InstallRead, InstallSection};
use super::resolve::{self, ETC_UNIT_DIR, LinkEnd, ResolveError, VENDOR_UNIT_DIR};
use inspectah_core::traits::executor::Executor;
use inspectah_core::types::units::{
    InstallUnknown, LinkRelation, ManualLink, ManualLinkCause, UndeterminedInstall, UnitName,
    UnitState,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

/// The dependency directories a unit's links sit in, by the suffix of the
/// directory's name after the parent unit's name.
const DEPENDENCY_DIRS: [(&str, LinkRelation); 3] = [
    (".wants", LinkRelation::Wants),
    (".requires", LinkRelation::Requires),
    (".upholds", LinkRelation::Upholds),
];

/// The link that sets the default target. It is not an alias (§ 7.6 of the
/// units spec owns it).
const DEFAULT_TARGET: &str = "default.target";

/// The templates whose instances name a volume or an escaped device path of
/// the source host. A link to one of their instances is never carried,
/// whatever its `[Install]` says (§ 7.5 of the units spec). (Mark Russell,
/// 2026-11-13)
const HOST_DEVICE_TEMPLATES: [&str; 2] = ["systemd-cryptsetup@.service", "systemd-fsck@.service"];

/// One link to a unit: the relation and the unit whose dependency
/// directory holds it, or, for an alias, the alias name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Link {
    pub(crate) relation: LinkRelation,
    pub(crate) parent: UnitName,
}

/// The links to each unit, by the name of the unit they link: the link's
/// own name in a dependency directory, or an alias's target.
pub(crate) type LinksByUnit = BTreeMap<UnitName, Vec<Link>>;

/// The links under both unit directories.
#[derive(Debug, Default)]
pub(crate) struct HostLinks {
    /// Under `/etc/systemd/system` (source 3).
    pub(crate) admin: LinksByUnit,
    /// The units named by an entry of a dependency directory under
    /// `/etc/systemd/system` whose parent does not parse as a
    /// [`UnitName`]. Such a link makes no row of its own (§ 7.3), but it is
    /// still a link to the unit under `/etc`.
    pub(crate) admin_unparsed_parent: BTreeSet<UnitName>,
    /// The paths of the entries of dependency directories under
    /// `/etc/systemd/system` whose parent or own name does not parse as a
    /// [`UnitName`]. Each gets a structured warning and nothing else
    /// (§ 7.3).
    pub(crate) admin_unparsed: Vec<String>,
    /// Whether some read under `/etc/systemd/system` failed, so links there
    /// may be missing from [`admin`](HostLinks::admin).
    pub(crate) admin_unreadable: bool,
    /// The names directly under `/etc/systemd/system`.
    pub(crate) admin_entries: BTreeSet<String>,
    /// Under `/usr/lib/systemd/system`, the links packages ship (source 4).
    /// They complete an install and never start one.
    pub(crate) vendor: LinksByUnit,
}

impl HostLinks {
    /// The links under `/etc/systemd/system` to `unit` whose parent parses.
    pub(crate) fn admin(&self, unit: &UnitName) -> &[Link] {
        self.admin.get(unit).map_or(&[], Vec::as_slice)
    }

    /// Whether any link to `unit` sits under `/etc/systemd/system`, one
    /// under a parent that does not parse included.
    pub(crate) fn any_admin_link(&self, unit: &UnitName) -> bool {
        !self.admin(unit).is_empty() || self.admin_unparsed_parent.contains(unit)
    }

    /// The links a package ships to `unit`.
    pub(crate) fn vendor(&self, unit: &UnitName) -> &[Link] {
        self.vendor.get(unit).map_or(&[], Vec::as_slice)
    }

    /// Whether `unit` has a file or a drop-in directory of its own name
    /// directly under `/etc/systemd/system`, where an install file of it
    /// can sit (§ 10.2).
    pub(crate) fn has_etc_files(&self, unit: &UnitName) -> bool {
        self.admin_entries.contains(unit.as_str())
            || self
                .admin_entries
                .contains(&format!("{}{DROP_IN_DIR_SUFFIX}", unit.as_str()))
    }
}

/// Read the links under both unit directories. A listing of
/// `/etc/systemd/system` that fails is left to the drop-in pass, which
/// lists the same directory and reports it; any other read that fails is
/// returned as `<path>: <error>` in the second value. Any failed read under
/// `/etc` sets [`HostLinks::admin_unreadable`]. A link whose own name does
/// not parse as a [`UnitName`] is skipped; one whose parent does not parse
/// is recorded only in [`HostLinks::admin_unparsed_parent`].
pub(crate) fn read_host_links(exec: &dyn Executor) -> (HostLinks, Vec<String>) {
    let mut failures = Vec::new();
    let admin = read_links(exec, ETC_UNIT_DIR, &mut failures);
    let vendor = read_links(exec, VENDOR_UNIT_DIR, &mut failures);
    let links = HostLinks {
        admin: admin.links,
        admin_unparsed_parent: admin.unparsed_parent,
        admin_unparsed: admin.unparsed,
        admin_unreadable: admin.unreadable,
        admin_entries: admin.entries,
        vendor: vendor.links,
    };
    (links, failures)
}

/// What one unit directory's links are.
#[derive(Default)]
struct RootLinks {
    links: LinksByUnit,
    /// The units linked from a dependency directory whose parent does not
    /// parse.
    unparsed_parent: BTreeSet<UnitName>,
    /// The paths of the dependency-directory entries whose parent or own
    /// name does not parse.
    unparsed: Vec<String>,
    /// Whether a read under the directory failed.
    unreadable: bool,
    /// The names directly under the directory.
    entries: BTreeSet<String>,
}

/// The links in one unit directory: every entry of its `.wants`,
/// `.requires` and `.upholds` directories, and every link directly under it
/// to another unit's name, other than `default.target` and a mask.
fn read_links(exec: &dyn Executor, root: &str, failures: &mut Vec<String>) -> RootLinks {
    let mut read = RootLinks::default();
    let mut fail = |read: &mut RootLinks, failure: String| {
        read.unreadable = true;
        failures.push(failure);
    };
    let entries = match exec.read_dir(Path::new(root)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return read,
        Err(_) if root == ETC_UNIT_DIR => {
            read.unreadable = true;
            return read;
        }
        Err(e) => {
            fail(&mut read, format!("{root}: {e}"));
            return read;
        }
    };
    read.entries = entries.iter().cloned().collect();
    for entry in entries {
        let path = format!("{root}/{entry}");
        let dependency = DEPENDENCY_DIRS.iter().find_map(|(suffix, relation)| {
            entry
                .strip_suffix(suffix)
                .map(|parent| (parent.to_owned(), *relation))
        });
        if let Some((parent, relation)) = dependency {
            let names = match exec.read_dir(Path::new(&path)) {
                Ok(names) => names,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => {
                    fail(&mut read, format!("{path}: {e}"));
                    continue;
                }
            };
            let parent = UnitName::try_from(parent).ok();
            for name in names {
                let link_path = format!("{path}/{name}");
                let Ok(unit) = UnitName::try_from(name) else {
                    read.unparsed.push(link_path);
                    continue;
                };
                match &parent {
                    Some(parent) => read.links.entry(unit).or_default().push(Link {
                        relation,
                        parent: parent.clone(),
                    }),
                    None => {
                        read.unparsed.push(link_path);
                        read.unparsed_parent.insert(unit);
                    }
                }
            }
            continue;
        }
        if entry == DEFAULT_TARGET {
            continue;
        }
        let Ok(alias) = UnitName::try_from(entry.clone()) else {
            continue;
        };
        match exec.read_link(Path::new(&path)) {
            Ok(_) => {}
            // Not a link (`EINVAL`), or gone since the listing.
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::InvalidInput | io::ErrorKind::NotFound
                ) =>
            {
                continue;
            }
            Err(e) => {
                fail(&mut read, format!("{path}: {e}"));
                continue;
            }
        }
        // The unit at the end of the whole chain, as the resolver reads it:
        // a mask, a link to a file outside both unit directories, and a
        // chain that does not resolve are no alias.
        match resolve::link_end(exec, &path) {
            Ok(LinkEnd::UnitFile(unit)) if unit != alias => {
                read.links.entry(unit).or_default().push(Link {
                    relation: LinkRelation::Alias,
                    parent: alias,
                });
            }
            Ok(_) => {}
            Err(ResolveError::Read { path, error }) => fail(&mut read, format!("{path}: {error}")),
            Err(ResolveError::Linked { .. }) => {}
        }
    }
    read
}

/// The links `systemctl enable` makes from `section`: one per `WantedBy=`
/// and `RequiredBy=` value, and one alias link per `Alias=` value.
pub(crate) fn expected_links(section: &InstallSection) -> Vec<Link> {
    let link = |relation| {
        move |parent: &UnitName| Link {
            relation,
            parent: parent.clone(),
        }
    };
    section
        .wanted_by
        .iter()
        .map(link(LinkRelation::Wants))
        .chain(section.required_by.iter().map(link(LinkRelation::Requires)))
        .chain(section.alias.iter().map(link(LinkRelation::Alias)))
        .collect()
}

/// The install state of a unit whose effective section is `read`, with
/// `admin` the links to it under `/etc/systemd/system`, `any_admin_link`
/// whether any link to it sits there (one under a parent that does not
/// parse included), and `vendor` the links a package ships to it (§ 7.3 of
/// the units spec, the table).
/// `None` is no install state: the unit has no enablement of its own, or
/// neither `enable` nor `disable` reproduces its links.
///
/// - A `Known` section that expects links is `Enabled` when every expected
///   link is under `/etc` or shipped and at least one is under `/etc`, and
///   `Disabled` when none is under `/etc`. Vendor links complete an
///   install and never start one.
/// - A section that expects nothing, or no section, has no state.
/// - An `Undetermined` section with no link to the unit under `/etc` is
///   `Disabled`, since `disable` needs no expected set, unless it assigns
///   `Also=`. With a link under `/etc` it has no state.
pub(crate) fn install_state(
    read: &InstallRead,
    admin: &[Link],
    any_admin_link: bool,
    vendor: &[Link],
) -> Option<UnitState> {
    match read {
        InstallRead::Absent => None,
        InstallRead::Known(section) => {
            let expected = expected_links(section);
            if expected.is_empty() {
                return None;
            }
            if !expected.iter().any(|link| admin.contains(link)) {
                return Some(UnitState::Disabled);
            }
            expected
                .iter()
                .all(|link| admin.contains(link) || vendor.contains(link))
                .then_some(UnitState::Enabled)
        }
        InstallRead::Undetermined { assigns_also, .. } => {
            (!any_admin_link && !assigns_also).then_some(UnitState::Disabled)
        }
    }
}

/// What the links say about one unit (§ 7.3 of the units spec): its
/// install state, the links reported and not carried, and the record of a
/// section that cannot be relied on.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct LinkFindings {
    /// The install state; `None` is no install state.
    pub(crate) state: Option<UnitState>,
    pub(crate) manual_links: Vec<ManualLink>,
    pub(crate) undetermined: Option<UndeterminedInstall>,
}

/// Read § 7.3's table for `unit`, whose effective section is `read`, with
/// `etc_install_files` its install files under `/etc/systemd/system` (its
/// unit file or shadow there, and its install drop-ins, § 10.2).
///
/// - An instance of a [`HOST_DEVICE_TEMPLATES`] template has no install
///   state, and each of its links under `/etc` is a `HostDevice` link.
/// - With no section, or a `Known` one that names no link, each link under
///   `/etc` is a `StaticUnit` link.
/// - With a `Known` section that names links, a link outside them is a
///   `NotInInstall` link. When the unit is `Enabled` or `Disabled` that is
///   all; when neither verb reproduces the links, each link under `/etc`
///   the section names is a `PartialInstall` link (Mark Russell,
///   2026-11-15).
/// - With an `Undetermined` section, each link under `/etc` is an
///   `Undetermined` link with its reason. The unit has one
///   [`UndeterminedInstall`] when it assigns `Also=` or has an install file
///   under `/etc`, and then no install state either; otherwise it is
///   `Disabled` with no link under `/etc` (§ 7.3's table, with T40 for the
///   unit with an `/etc` file and no link).
///
/// A link under a parent that does not parse makes no finding; it still
/// counts as a link to the unit under `/etc`.
pub(crate) fn link_findings(
    unit: &UnitName,
    read: &InstallRead,
    links: &HostLinks,
    etc_install_files: &[String],
) -> LinkFindings {
    let admin = links.admin(unit);
    let report = |cause: ManualLinkCause, linked: &mut dyn Iterator<Item = &Link>| {
        linked
            .map(|link| ManualLink {
                relation: link.relation,
                parent: link.parent.clone(),
                unit: unit.clone(),
                cause,
                install_files: Vec::new(),
                aggregate: None,
            })
            .collect::<Vec<_>>()
    };
    let host_device = unit
        .instance()
        .is_some_and(|instance| HOST_DEVICE_TEMPLATES.contains(&instance.template().as_str()));
    if host_device {
        return LinkFindings {
            state: None,
            manual_links: report(ManualLinkCause::HostDevice, &mut admin.iter()),
            undetermined: None,
        };
    }
    let state = install_state(read, admin, links.any_admin_link(unit), links.vendor(unit));
    match read {
        InstallRead::Absent => LinkFindings {
            state,
            manual_links: report(ManualLinkCause::StaticUnit, &mut admin.iter()),
            undetermined: None,
        },
        InstallRead::Known(section) => {
            let expected = expected_links(section);
            let manual_links = match state {
                _ if expected.is_empty() => report(ManualLinkCause::StaticUnit, &mut admin.iter()),
                Some(_) => report(
                    ManualLinkCause::NotInInstall,
                    &mut admin.iter().filter(|link| !expected.contains(link)),
                ),
                None => admin
                    .iter()
                    .flat_map(|link| {
                        let cause = if expected.contains(link) {
                            ManualLinkCause::PartialInstall
                        } else {
                            ManualLinkCause::NotInInstall
                        };
                        report(cause, &mut std::iter::once(link))
                    })
                    .collect(),
            };
            LinkFindings {
                state,
                manual_links,
                undetermined: None,
            }
        }
        InstallRead::Undetermined {
            reason,
            assigns_also,
        } => {
            let recorded = *assigns_also || !etc_install_files.is_empty();
            LinkFindings {
                state: state.filter(|_| !recorded),
                manual_links: report(
                    ManualLinkCause::Undetermined { reason: *reason },
                    &mut admin.iter(),
                ),
                undetermined: recorded.then(|| UndeterminedInstall {
                    unit: unit.clone(),
                    reason: *reason,
                    install_files: etc_install_files.to_vec(),
                    aggregate: None,
                }),
            }
        }
    }
}

/// How a unit's effective `[Install]` section differs from the one its
/// package alone provides (§ 7.4 of the units spec, "A unit at its preset
/// whose `/etc` files change `[Install]`"). Internal to the collector; it
/// is not stored.
#[derive(Debug, PartialEq)]
pub(crate) enum InstallDifference {
    /// The links and the `Also=` names are equal.
    Same,
    /// Every link the package's section makes is an effective link, at
    /// least one effective link is not, and the `Also=` names are equal.
    Additive,
    /// Anything else: the links and the `Also=` names only one side has.
    NotAdditive {
        host_only: Vec<Link>,
        vendor_only: Vec<Link>,
        also_host_only: Vec<UnitName>,
        also_vendor_only: Vec<UnitName>,
    },
    /// The package's section cannot be relied on.
    VendorUndetermined { reason: InstallUnknown },
}

/// Compare `effective`, the unit's effective `[Install]` section (`None`
/// when it has none), with `vendor`, the one its package alone provides
/// (§ 7.4 of the units spec). Two pairs of sets are compared: the links
/// `enable` would make from each (a relation and a parent, or an alias
/// name), and the names each assigns in `Also=`. Order and spelling of the
/// sections do not matter.
///
/// The `Also=` names are compared as given: the caller maps a template to
/// its `DefaultInstance=` on both sides first, as § 7.4 asks.
pub(crate) fn compare_install(
    effective: Option<&InstallSection>,
    vendor: &InstallRead,
) -> InstallDifference {
    let none = InstallSection::default();
    let vendor = match vendor {
        InstallRead::Undetermined { reason, .. } => {
            return InstallDifference::VendorUndetermined { reason: *reason };
        }
        InstallRead::Absent => &none,
        InstallRead::Known(section) => section,
    };
    let effective = effective.unwrap_or(&none);
    let (effective_links, vendor_links) = (expected_links(effective), expected_links(vendor));
    let host_only = only_in(&effective_links, &vendor_links);
    let vendor_only = only_in(&vendor_links, &effective_links);
    let also_host_only = only_in(&effective.also, &vendor.also);
    let also_vendor_only = only_in(&vendor.also, &effective.also);
    if vendor_only.is_empty() && also_host_only.is_empty() && also_vendor_only.is_empty() {
        if host_only.is_empty() {
            InstallDifference::Same
        } else {
            InstallDifference::Additive
        }
    } else {
        InstallDifference::NotAdditive {
            host_only,
            vendor_only,
            also_host_only,
            also_vendor_only,
        }
    }
}

/// The items of `side` that `other` does not hold, in `side`'s order.
fn only_in<T: Clone + PartialEq>(side: &[T], other: &[T]) -> Vec<T> {
    side.iter()
        .filter(|item| !other.contains(item))
        .cloned()
        .collect()
}

/// Apply `difference`, § 7.4's comparison for `unit`, to `found`, the
/// unit's findings from [`link_findings`], with `install_files` its `/etc`
/// install files. Returns whether the unit is an additive state change:
/// `Enabled` whatever its preset says.
///
/// - `NotAdditive`: one `HostInstallOnly` finding per effective link the
///   package's section does not make; one `VendorInstallOnly` finding per
///   link only the package's section makes that is not under `/etc` on the
///   host (one that is, the host and the image both have); and relation
///   `Also`, with the same two causes, for a name only one side assigns.
///   A `StaticUnit` link the package's section makes is dropped: the
///   package install makes it in the image too, so it produces nothing.
/// - `VendorUndetermined`: as for an effective section that cannot be
///   relied on, the unit has no install state and one
///   [`UndeterminedInstall`] with the package section's reason and
///   `install_files`, whether or not it has a link (Mark Russell,
///   2026-11-15), and each of its links under `/etc` is an `Undetermined`
///   finding with that reason, in place of what [`link_findings`] reported
///   for it.
pub(crate) fn apply_install_difference(
    found: &mut LinkFindings,
    unit: &UnitName,
    difference: InstallDifference,
    links: &HostLinks,
    install_files: &[String],
) -> bool {
    let finding =
        |relation, parent: &UnitName, member: &UnitName, cause, files: &[String]| ManualLink {
            relation,
            parent: parent.clone(),
            unit: member.clone(),
            cause,
            install_files: files.to_vec(),
            aggregate: None,
        };
    match difference {
        InstallDifference::Same => false,
        InstallDifference::Additive => true,
        InstallDifference::NotAdditive {
            host_only,
            vendor_only,
            also_host_only,
            also_vendor_only,
        } => {
            found.manual_links.retain(|link| {
                link.cause != ManualLinkCause::StaticUnit
                    || !vendor_only.contains(&Link {
                        relation: link.relation,
                        parent: link.parent.clone(),
                    })
            });
            let admin = links.admin(unit);
            let differences = host_only
                .iter()
                .map(|link| (link, ManualLinkCause::HostInstallOnly))
                .chain(
                    vendor_only
                        .iter()
                        .filter(|link| !admin.contains(link))
                        .map(|link| (link, ManualLinkCause::VendorInstallOnly)),
                )
                .map(|(link, cause)| {
                    finding(link.relation, &link.parent, unit, cause, install_files)
                });
            let also = also_host_only
                .iter()
                .map(|member| (member, ManualLinkCause::HostInstallOnly))
                .chain(
                    also_vendor_only
                        .iter()
                        .map(|member| (member, ManualLinkCause::VendorInstallOnly)),
                )
                .map(|(member, cause)| {
                    finding(LinkRelation::Also, unit, member, cause, install_files)
                });
            found.manual_links.extend(differences.chain(also));
            false
        }
        InstallDifference::VendorUndetermined { reason } => {
            found.state = None;
            found.undetermined = Some(UndeterminedInstall {
                unit: unit.clone(),
                reason,
                install_files: install_files.to_vec(),
                aggregate: None,
            });
            found.manual_links = links
                .admin(unit)
                .iter()
                .map(|link| {
                    finding(
                        link.relation,
                        &link.parent,
                        unit,
                        ManualLinkCause::Undetermined { reason },
                        &[],
                    )
                })
                .collect();
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;

    /// A link to this path masks a unit; it is not an alias.
    const DEV_NULL: &str = "/dev/null";

    fn unit(name: &str) -> UnitName {
        UnitName::try_from(name.to_owned()).expect("a valid unit name")
    }

    fn wants(parent: &str) -> Link {
        Link {
            relation: LinkRelation::Wants,
            parent: unit(parent),
        }
    }

    fn requires(parent: &str) -> Link {
        Link {
            relation: LinkRelation::Requires,
            parent: unit(parent),
        }
    }

    fn alias(name: &str) -> Link {
        Link {
            relation: LinkRelation::Alias,
            parent: unit(name),
        }
    }

    fn section(wanted_by: &[&str], required_by: &[&str], aliases: &[&str]) -> InstallRead {
        let units = |names: &[&str]| names.iter().map(|n| unit(n)).collect();
        InstallRead::Known(InstallSection {
            wanted_by: units(wanted_by),
            required_by: units(required_by),
            alias: units(aliases),
            ..InstallSection::default()
        })
    }

    /// § 7.3's table: each `[Install]` reading against the links under
    /// `/etc` and under `/usr/lib`.
    #[test]
    fn the_install_state_is_read_from_the_links() {
        let undetermined = |assigns_also| InstallRead::Undetermined {
            reason: InstallUnknown::UpheldBy,
            assigns_also,
        };
        let only_also = InstallRead::Known(InstallSection {
            also: vec![unit("helper.service")],
            ..InstallSection::default()
        });
        let multi = &["multi-user.target"][..];
        let two = &["multi-user.target", "graphical.target"][..];
        // (case, section, admin links, vendor links, install state)
        type Case = (
            &'static str,
            InstallRead,
            Vec<Link>,
            Vec<Link>,
            Option<UnitState>,
        );
        let cases: Vec<Case> = vec![
            (
                "the expected link under /etc",
                section(multi, &[], &[]),
                vec![wants("multi-user.target")],
                vec![],
                Some(UnitState::Enabled),
            ),
            (
                "only a hand-made link",
                section(multi, &[], &[]),
                vec![wants("rescue.target")],
                vec![],
                Some(UnitState::Disabled),
            ),
            (
                "the expected link and a hand-made one",
                section(multi, &[], &[]),
                vec![wants("rescue.target"), wants("multi-user.target")],
                vec![],
                Some(UnitState::Enabled),
            ),
            (
                "two expected links, one under /etc",
                section(two, &[], &[]),
                vec![wants("multi-user.target")],
                vec![],
                None,
            ),
            (
                "two expected links, the second shipped",
                section(two, &[], &[]),
                vec![wants("multi-user.target")],
                vec![wants("graphical.target")],
                Some(UnitState::Enabled),
            ),
            (
                "only shipped links",
                section(multi, &[], &[]),
                vec![],
                vec![wants("multi-user.target")],
                Some(UnitState::Disabled),
            ),
            (
                "a RequiredBy= link",
                section(&[], &["a.target"], &[]),
                vec![requires("a.target")],
                vec![],
                Some(UnitState::Enabled),
            ),
            (
                "a wants link is not the RequiredBy= link",
                section(&[], &["a.target"], &[]),
                vec![wants("a.target")],
                vec![],
                Some(UnitState::Disabled),
            ),
            (
                "the WantedBy= link and the alias",
                section(multi, &[], &["x-alias.service"]),
                vec![wants("multi-user.target"), alias("x-alias.service")],
                vec![],
                Some(UnitState::Enabled),
            ),
            (
                "the WantedBy= link without the alias",
                section(multi, &[], &["x-alias.service"]),
                vec![wants("multi-user.target")],
                vec![],
                None,
            ),
            (
                "nothing expected, with a link",
                section(&[], &[], &[]),
                vec![wants("multi-user.target")],
                vec![],
                None,
            ),
            ("only Also=", only_also, vec![], vec![], None),
            (
                "no [Install], with a link",
                InstallRead::Absent,
                vec![wants("multi-user.target")],
                vec![],
                None,
            ),
            (
                "undetermined, no link",
                undetermined(false),
                vec![],
                vec![wants("multi-user.target")],
                Some(UnitState::Disabled),
            ),
            (
                "undetermined, assigning Also=, no link",
                undetermined(true),
                vec![],
                vec![],
                None,
            ),
            (
                "undetermined, a link under /etc",
                undetermined(false),
                vec![wants("multi-user.target")],
                vec![],
                None,
            ),
        ];
        for (case, read, admin, vendor, expected) in cases {
            let any = !admin.is_empty();
            assert_eq!(
                install_state(&read, &admin, any, &vendor),
                expected,
                "{case}"
            );
        }
    }

    /// Source 3 and 4: dependency links by the name they link, aliases by
    /// their target, and nothing for `default.target`, a mask, a unit file
    /// or a name that does not parse.
    #[test]
    fn host_links_are_read_from_both_unit_directories() {
        let exec = MockExecutor::new()
            .with_dir(
                ETC_UNIT_DIR,
                vec![
                    "multi-user.target.wants",
                    "a.target.requires",
                    "b.target.upholds",
                    "x-alias.service",
                    "default.target",
                    "masked.service",
                    "local.service",
                ],
            )
            .with_dir(
                &format!("{ETC_UNIT_DIR}/multi-user.target.wants"),
                vec!["app.service", "getty@tty2.service", "not a unit"],
            )
            .with_dir(
                &format!("{ETC_UNIT_DIR}/a.target.requires"),
                vec!["app.service"],
            )
            .with_dir(
                &format!("{ETC_UNIT_DIR}/b.target.upholds"),
                vec!["up.service"],
            )
            .with_link(
                &format!("{ETC_UNIT_DIR}/x-alias.service"),
                "/usr/lib/systemd/system/app.service",
            )
            .with_link(
                &format!("{ETC_UNIT_DIR}/default.target"),
                "/usr/lib/systemd/system/graphical.target",
            )
            .with_link(&format!("{ETC_UNIT_DIR}/masked.service"), DEV_NULL)
            .with_file(&format!("{ETC_UNIT_DIR}/local.service"), "[Unit]\n")
            .with_dir(
                VENDOR_UNIT_DIR,
                vec!["remote-fs.target.wants", "dbus-org.x.service"],
            )
            .with_dir(
                &format!("{VENDOR_UNIT_DIR}/remote-fs.target.wants"),
                vec!["remote-cryptsetup.target"],
            )
            .with_link(
                &format!("{VENDOR_UNIT_DIR}/dbus-org.x.service"),
                "x.service",
            );

        let (links, failures) = read_host_links(&exec);

        assert!(failures.is_empty(), "{failures:?}");
        let names = |by: &LinksByUnit| by.keys().map(|u| u.as_str().to_owned()).collect::<Vec<_>>();
        assert_eq!(
            names(&links.admin),
            ["app.service", "getty@tty2.service", "up.service"]
        );
        let app = links.admin(&unit("app.service"));
        assert_eq!(app.len(), 3, "{app:?}");
        for link in [
            wants("multi-user.target"),
            requires("a.target"),
            alias("x-alias.service"),
        ] {
            assert!(app.contains(&link), "{link:?} in {app:?}");
        }
        assert_eq!(
            links.admin(&unit("up.service")),
            [Link {
                relation: LinkRelation::Upholds,
                parent: unit("b.target"),
            }]
        );
        assert_eq!(
            links.vendor(&unit("remote-cryptsetup.target")),
            [wants("remote-fs.target")]
        );
        assert_eq!(
            links.vendor(&unit("x.service")),
            [alias("dbus-org.x.service")]
        );
        assert!(links.admin(&unit("graphical.target")).is_empty());
    }

    /// An alias is the unit at the end of its whole chain, as the resolver
    /// reads it, never its first target's name. `public.service` links to
    /// `relay.service`, which links to `real.service`: both are aliases of
    /// `real.service`, and `relay.service` has none. A link that ends at a
    /// file outside both unit directories is no alias, even when that file
    /// is named `real.service`; one that ends at an empty file is a mask.
    /// Control: a direct link to the vendor file is an alias of it.
    #[test]
    fn an_alias_is_the_unit_its_whole_chain_ends_at() {
        let real = format!("{VENDOR_UNIT_DIR}/real.service");
        let exec = MockExecutor::new()
            .with_dir(
                ETC_UNIT_DIR,
                vec![
                    "public.service",
                    "relay.service",
                    "direct.service",
                    "outside.service",
                    "emptied.service",
                ],
            )
            .with_link(&format!("{ETC_UNIT_DIR}/public.service"), "relay.service")
            .with_link(&format!("{ETC_UNIT_DIR}/relay.service"), &real)
            .with_link(&format!("{ETC_UNIT_DIR}/direct.service"), &real)
            .with_link(
                &format!("{ETC_UNIT_DIR}/outside.service"),
                "/opt/app/real.service",
            )
            .with_file("/opt/app/real.service", "[Unit]\nDescription=elsewhere\n")
            .with_link(
                &format!("{ETC_UNIT_DIR}/emptied.service"),
                "/opt/app/empty.service",
            )
            .with_file("/opt/app/empty.service", "")
            .with_dir(VENDOR_UNIT_DIR, vec!["real.service"])
            .with_file(&real, "[Install]\nAlias=public.service\n");

        let (links, failures) = read_host_links(&exec);

        assert!(failures.is_empty(), "{failures:?}");
        let real = links.admin(&unit("real.service"));
        assert!(
            real.contains(&alias("direct.service")),
            "control: a direct link is an alias: {real:?}"
        );
        assert!(
            real.contains(&alias("public.service")),
            "the chain's first link is an alias of its end: {real:?}"
        );
        assert!(
            real.contains(&alias("relay.service")),
            "the chain's second link is an alias of its end: {real:?}"
        );
        assert_eq!(real.len(), 3, "{real:?}");
        assert!(links.admin(&unit("relay.service")).is_empty());
        assert_eq!(
            links.admin.keys().map(UnitName::as_str).collect::<Vec<_>>(),
            ["real.service"]
        );
    }

    /// A link whose chain cannot be read to its end is a failed read under
    /// `/etc`: its links there are incomplete, and the alias is not counted.
    #[test]
    fn an_alias_whose_end_cannot_be_read_marks_etc_unreadable() {
        use std::io::ErrorKind::PermissionDenied;
        let end = format!("{VENDOR_UNIT_DIR}/real.service");
        let exec = MockExecutor::new()
            .with_dir(ETC_UNIT_DIR, vec!["public.service"])
            .with_link(&format!("{ETC_UNIT_DIR}/public.service"), &end)
            .with_file_error(&end, PermissionDenied);
        let (links, failures) = read_host_links(&exec);
        assert!(
            failures.len() == 1 && failures[0].starts_with(&format!("{end}: ")),
            "{failures:?}"
        );
        assert!(links.admin_unreadable);
        assert!(links.admin(&unit("real.service")).is_empty());
    }

    /// A chain that cannot be followed to its end, because a link on the
    /// way cannot be read, is a failed read under `/etc` naming the alias:
    /// its links there are incomplete, and no alias is counted. Control:
    /// the readable chain is an alias of the unit it ends at.
    #[test]
    fn an_alias_whose_chain_cannot_be_followed_marks_etc_unreadable() {
        use std::io::ErrorKind::PermissionDenied;
        let public = format!("{ETC_UNIT_DIR}/public.service");
        let relay = "/opt/units/relay.service";
        let end = format!("{VENDOR_UNIT_DIR}/real.service");
        let chain = || {
            MockExecutor::new()
                .with_dir(ETC_UNIT_DIR, vec!["public.service"])
                .with_link(&public, relay)
                .with_link(relay, &end)
                .with_file(&end, "[Install]\nAlias=public.service\n")
        };

        let (links, failures) = read_host_links(&chain());
        assert!(failures.is_empty(), "control: {failures:?}");
        assert!(!links.admin_unreadable, "control");
        assert_eq!(
            links.admin(&unit("real.service")),
            [alias("public.service")],
            "control"
        );

        let (links, failures) = read_host_links(&chain().with_link_error(relay, PermissionDenied));
        assert!(
            failures.len() == 1
                && failures[0].starts_with(&format!("{public}: "))
                && failures[0].contains(relay),
            "{failures:?}"
        );
        assert!(links.admin_unreadable);
        assert!(links.admin(&unit("real.service")).is_empty());
    }

    /// A link in a dependency directory whose parent does not parse makes
    /// no link of its own, and is still a link to the unit under `/etc`:
    /// an `Undetermined` section with only that link has no state.
    #[test]
    fn a_link_under_an_unparsed_parent_counts_as_a_link_under_etc() {
        let exec = MockExecutor::new()
            .with_dir(ETC_UNIT_DIR, vec!["dev-foo.device.wants"])
            .with_dir(
                &format!("{ETC_UNIT_DIR}/dev-foo.device.wants"),
                vec!["guest.service"],
            );
        let (links, failures) = read_host_links(&exec);
        assert!(failures.is_empty(), "{failures:?}");
        let guest = unit("guest.service");
        assert!(links.admin(&guest).is_empty());
        assert!(links.any_admin_link(&guest));
        assert!(!links.admin_unreadable);
        let undetermined = InstallRead::Undetermined {
            reason: InstallUnknown::ForeignName,
            assigns_also: false,
        };
        assert_eq!(
            install_state(
                &undetermined,
                links.admin(&guest),
                links.any_admin_link(&guest),
                &[]
            ),
            None
        );
        assert_eq!(
            install_state(&undetermined, &[], false, &[]),
            Some(UnitState::Disabled)
        );
    }

    /// Every failure branch of the link read: which reads report a failure,
    /// and which leave `/etc`'s links incomplete. A listing of
    /// `/etc/systemd/system` itself is reported by the drop-in pass, so it
    /// adds no failure here, and the vendor side never marks `/etc`.
    #[test]
    fn failed_link_reads_are_reported_and_mark_etc_unreadable() {
        use std::io::ErrorKind::PermissionDenied;
        let etc_wants = format!("{ETC_UNIT_DIR}/multi-user.target.wants");
        let vendor_wants = format!("{VENDOR_UNIT_DIR}/multi-user.target.wants");
        let alias = format!("{ETC_UNIT_DIR}/x-alias.service");
        let cases: Vec<(&str, MockExecutor, Option<String>, bool)> = vec![
            (
                "the /etc listing",
                MockExecutor::new().with_dir_error(ETC_UNIT_DIR, PermissionDenied),
                None,
                true,
            ),
            (
                "a dependency directory under /etc",
                MockExecutor::new()
                    .with_dir(ETC_UNIT_DIR, vec!["multi-user.target.wants"])
                    .with_dir_error(&etc_wants, PermissionDenied),
                Some(etc_wants.clone()),
                true,
            ),
            (
                "an alias under /etc",
                MockExecutor::new()
                    .with_dir(ETC_UNIT_DIR, vec!["x-alias.service"])
                    .with_link_error(&alias, PermissionDenied),
                Some(alias.clone()),
                true,
            ),
            (
                "the vendor listing",
                MockExecutor::new().with_dir_error(VENDOR_UNIT_DIR, PermissionDenied),
                Some(VENDOR_UNIT_DIR.to_owned()),
                false,
            ),
            (
                "a dependency directory under the vendor directory",
                MockExecutor::new()
                    .with_dir(VENDOR_UNIT_DIR, vec!["multi-user.target.wants"])
                    .with_dir_error(&vendor_wants, PermissionDenied),
                Some(vendor_wants.clone()),
                false,
            ),
        ];
        for (case, exec, failed_path, unreadable) in cases {
            let (links, failures) = read_host_links(&exec);
            match &failed_path {
                Some(path) => assert!(
                    failures.len() == 1 && failures[0].starts_with(&format!("{path}: ")),
                    "{case}: {failures:?}"
                ),
                None => assert!(failures.is_empty(), "{case}: {failures:?}"),
            }
            assert_eq!(links.admin_unreadable, unreadable, "{case}");
        }
    }

    /// T24: a vendor drop-in holding `WantedBy=multi-user.target` in the
    /// type-wide `service.d` or the dash-prefix `a-.service.d` is never
    /// read for `[Install]`, so it makes no expected link; the same file
    /// in the unit's own `.d` does.
    #[test]
    fn a_type_wide_or_dash_prefix_drop_in_makes_no_expected_link() {
        use super::super::install::{DropInRoot, DropInSource, read_install};
        let name = unit("a-b.service");
        let drop_in = "[Install]\nWantedBy=multi-user.target\n";
        for (dir, expected) in [
            ("service.d", vec![]),
            ("a-.service.d", vec![]),
            ("a-b.service.d", vec![wants("multi-user.target")]),
        ] {
            let source = DropInSource {
                root: DropInRoot::Vendor,
                dir: dir.to_owned(),
                file_name: "10-install.conf".to_owned(),
            };
            let links = match read_install(
                &name,
                "[Service]\nExecStart=/bin/true\n",
                &[(source, drop_in)],
            ) {
                InstallRead::Known(section) => expected_links(&section),
                InstallRead::Absent => Vec::new(),
                other => panic!("{dir}: {other:?}"),
            };
            assert_eq!(links, expected, "{dir}");
        }
    }

    /// `HostLinks` with `admin` links to `name` under `/etc`.
    fn host_with(name: &str, admin: Vec<Link>) -> HostLinks {
        HostLinks {
            admin: LinksByUnit::from([(unit(name), admin)]),
            ..HostLinks::default()
        }
    }

    /// The (relation, parent, cause) of each link finding.
    fn found(findings: &LinkFindings) -> Vec<(LinkRelation, &str, ManualLinkCause)> {
        findings
            .manual_links
            .iter()
            .map(|l| (l.relation, l.parent.as_str(), l.cause))
            .collect()
    }

    /// § 7.3's last column and § 7.5's device templates: which links under
    /// `/etc` are reported, with which cause, and when an undetermined
    /// section gets its record.
    #[test]
    fn link_findings_follow_the_table() {
        use ManualLinkCause::{HostDevice, NotInInstall, PartialInstall, StaticUnit};
        let multi = &["multi-user.target"][..];
        let rescue = wants("rescue.target");

        // A systemd-fsck@ instance: whatever its section says, its link is
        // a device link and it has no state. Control: an instance of
        // another template with the same section and link is `Enabled`.
        let fsck = r"systemd-fsck@dev-disk-by\x2duuid-0a1b.service";
        let fsck_found = link_findings(
            &unit(fsck),
            &section(multi, &[], &[]),
            &host_with(fsck, vec![wants("multi-user.target")]),
            &[],
        );
        assert_eq!(fsck_found.state, None);
        assert_eq!(
            found(&fsck_found),
            [(LinkRelation::Wants, "multi-user.target", HostDevice)]
        );
        let other = "worker@site.service";
        let other_found = link_findings(
            &unit(other),
            &section(multi, &[], &[]),
            &host_with(other, vec![wants("multi-user.target")]),
            &[],
        );
        assert_eq!(other_found.state, Some(UnitState::Enabled));
        assert!(other_found.manual_links.is_empty());

        // `Absent`, and `Known` with nothing expected: static links.
        for read in [InstallRead::Absent, section(&[], &[], &[])] {
            let f = link_findings(
                &unit("a.service"),
                &read,
                &host_with("a.service", vec![rescue.clone()]),
                &[],
            );
            assert_eq!(f.state, None);
            assert_eq!(
                found(&f),
                [(LinkRelation::Wants, "rescue.target", StaticUnit)]
            );
        }

        // `Known`: enable-made links report nothing, others are not in the
        // section; two values and one link is a partial install.
        let both = link_findings(
            &unit("a.service"),
            &section(multi, &[], &[]),
            &host_with(
                "a.service",
                vec![wants("multi-user.target"), rescue.clone()],
            ),
            &[],
        );
        assert_eq!(both.state, Some(UnitState::Enabled));
        assert_eq!(
            found(&both),
            [(LinkRelation::Wants, "rescue.target", NotInInstall)]
        );
        let partial = link_findings(
            &unit("a.service"),
            &section(&["multi-user.target", "graphical.target"], &[], &[]),
            &host_with("a.service", vec![wants("multi-user.target")]),
            &[],
        );
        assert_eq!(partial.state, None);
        assert_eq!(
            found(&partial),
            [(LinkRelation::Wants, "multi-user.target", PartialInstall)]
        );
        // A partial unit's link the section does not name, a `.wants` link
        // or an alias, is not in the section (Mark Russell, 2026-11-15).
        let partial_and_more = link_findings(
            &unit("a.service"),
            &section(
                &["multi-user.target", "graphical.target"],
                &[],
                &["a-alias.service", "b-alias.service"],
            ),
            &host_with(
                "a.service",
                vec![
                    wants("multi-user.target"),
                    rescue.clone(),
                    alias("a-alias.service"),
                    alias("c-alias.service"),
                ],
            ),
            &[],
        );
        assert_eq!(partial_and_more.state, None);
        assert_eq!(
            found(&partial_and_more),
            [
                (LinkRelation::Wants, "multi-user.target", PartialInstall),
                (LinkRelation::Wants, "rescue.target", NotInInstall),
                (LinkRelation::Alias, "a-alias.service", PartialInstall),
                (LinkRelation::Alias, "c-alias.service", NotInInstall),
            ]
        );

        // `Undetermined`: each link carries the reason; the record comes
        // with an `/etc` install file or `Also=`, and then no state.
        let reason = InstallUnknown::RequiredByReset;
        let undetermined = |assigns_also| InstallRead::Undetermined {
            reason,
            assigns_also,
        };
        let file = "/etc/systemd/system/a.service.d/10-reset.conf".to_owned();
        let record = |files: Vec<String>| UndeterminedInstall {
            unit: unit("a.service"),
            reason,
            install_files: files,
            aggregate: None,
        };
        // (case, assigns Also=, /etc files, links, state, links found, record)
        type Case = (
            &'static str,
            bool,
            Vec<String>,
            Vec<Link>,
            Option<UnitState>,
            usize,
            Option<UndeterminedInstall>,
        );
        let cases: Vec<Case> = vec![
            (
                "no file, no link",
                false,
                vec![],
                vec![],
                Some(UnitState::Disabled),
                0,
                None,
            ),
            (
                "a file, no link",
                false,
                vec![file.clone()],
                vec![],
                None,
                0,
                Some(record(vec![file.clone()])),
            ),
            (
                "no file, a link",
                false,
                vec![],
                vec![requires("a.target")],
                None,
                1,
                None,
            ),
            (
                "a file and a link",
                false,
                vec![file.clone()],
                vec![requires("a.target")],
                None,
                1,
                Some(record(vec![file.clone()])),
            ),
            (
                "Also=, no link",
                true,
                vec![],
                vec![],
                None,
                0,
                Some(record(vec![])),
            ),
            (
                "Also= and a link",
                true,
                vec![],
                vec![requires("a.target")],
                None,
                1,
                Some(record(vec![])),
            ),
        ];
        for (case, assigns_also, files, admin, state, links, expected) in cases {
            let f = link_findings(
                &unit("a.service"),
                &undetermined(assigns_also),
                &host_with("a.service", admin),
                &files,
            );
            assert_eq!(f.state, state, "{case}");
            assert_eq!(f.manual_links.len(), links, "{case}");
            assert!(
                f.manual_links
                    .iter()
                    .all(|l| l.cause == ManualLinkCause::Undetermined { reason }),
                "{case}"
            );
            assert_eq!(f.undetermined, expected, "{case}");
        }
    }

    /// The entries of `/etc` dependency directories whose parent or own
    /// name does not parse are listed by path for the warning; a vendor
    /// one is not.
    #[test]
    fn unparsed_links_under_etc_are_listed_by_path() {
        let exec = MockExecutor::new()
            .with_dir(
                ETC_UNIT_DIR,
                vec!["dev-foo.device.wants", "multi-user.target.wants"],
            )
            .with_dir(
                &format!("{ETC_UNIT_DIR}/dev-foo.device.wants"),
                vec!["guest.service"],
            )
            .with_dir(
                &format!("{ETC_UNIT_DIR}/multi-user.target.wants"),
                vec!["not a unit", "app.service"],
            )
            .with_dir(VENDOR_UNIT_DIR, vec!["multi-user.target.wants"])
            .with_dir(
                &format!("{VENDOR_UNIT_DIR}/multi-user.target.wants"),
                vec!["also not a unit"],
            );
        let (links, failures) = read_host_links(&exec);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(
            links.admin_unparsed,
            [
                format!("{ETC_UNIT_DIR}/dev-foo.device.wants/guest.service"),
                format!("{ETC_UNIT_DIR}/multi-user.target.wants/not a unit"),
            ]
        );
        assert_eq!(links.admin(&unit("app.service")).len(), 1);
    }

    /// § 7.4's comparison of the effective section with the package's:
    /// reordered values are `Same`; an added `Alias=` is `Additive`; equal
    /// links with a differing `Also=` name are `NotAdditive`; an effective
    /// section that names no link, or none at all, against a package
    /// section with one is `NotAdditive` with that link in `vendor_only`; an
    /// `Undetermined` package section is `VendorUndetermined`.
    #[test]
    fn compare_install_follows_the_four_rows() {
        let known = |wanted_by: &[&str], aliases: &[&str], also: &[&str]| {
            let units = |names: &[&str]| names.iter().map(|n| unit(n)).collect();
            InstallSection {
                wanted_by: units(wanted_by),
                alias: units(aliases),
                also: units(also),
                ..InstallSection::default()
            }
        };
        let vendor = |section: InstallSection| InstallRead::Known(section);
        let not_additive = |host_only, vendor_only, also_host_only, also_vendor_only| {
            InstallDifference::NotAdditive {
                host_only,
                vendor_only,
                also_host_only,
                also_vendor_only,
            }
        };
        let both = known(&["multi-user.target", "graphical.target"], &[], &[]);
        let cases: Vec<(&str, Option<InstallSection>, InstallRead, InstallDifference)> = vec![
            (
                "reordered values",
                Some(known(&["graphical.target", "multi-user.target"], &[], &[])),
                vendor(both.clone()),
                InstallDifference::Same,
            ),
            (
                "an added Alias=",
                Some(known(&["multi-user.target"], &["app-alias.service"], &[])),
                vendor(known(&["multi-user.target"], &[], &[])),
                InstallDifference::Additive,
            ),
            (
                "equal links, a differing Also= name",
                Some(known(&["multi-user.target"], &[], &["a.socket"])),
                vendor(known(&["multi-user.target"], &[], &["b.socket"])),
                not_additive(
                    vec![],
                    vec![],
                    vec![unit("a.socket")],
                    vec![unit("b.socket")],
                ),
            ),
            (
                "an effective section that names no link",
                Some(known(&[], &[], &[])),
                vendor(known(&["multi-user.target"], &[], &[])),
                not_additive(vec![], vec![wants("multi-user.target")], vec![], vec![]),
            ),
            (
                "no effective section",
                None,
                vendor(known(&["multi-user.target"], &[], &[])),
                not_additive(vec![], vec![wants("multi-user.target")], vec![], vec![]),
            ),
            (
                "a replaced link",
                Some(known(&["graphical.target"], &[], &[])),
                vendor(known(&["multi-user.target"], &[], &[])),
                not_additive(
                    vec![wants("graphical.target")],
                    vec![wants("multi-user.target")],
                    vec![],
                    vec![],
                ),
            ),
            (
                "an undetermined package section",
                Some(both.clone()),
                InstallRead::Undetermined {
                    reason: InstallUnknown::Specifier,
                    assigns_also: false,
                },
                InstallDifference::VendorUndetermined {
                    reason: InstallUnknown::Specifier,
                },
            ),
        ];
        for (case, effective, vendor, expected) in cases {
            assert_eq!(
                compare_install(effective.as_ref(), &vendor),
                expected,
                "{case}"
            );
        }
    }
}
