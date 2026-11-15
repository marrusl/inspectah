//! The links on the source host that enable a unit, and the install state
//! they give it (§ 7.1 sources 3 and 4, § 7.3 of the units spec).
//!
//! `systemctl list-unit-files` says `enabled` when it finds any link to a
//! unit, and does not say who made it. The install state is read here
//! instead: the links `enable` would make, from the effective `[Install]`
//! section, against the links under `/etc/systemd/system` and the links a
//! package ships under `/usr/lib/systemd/system`.

use super::install::{InstallRead, InstallSection};
use super::resolve::{ETC_UNIT_DIR, VENDOR_UNIT_DIR};
use inspectah_core::traits::executor::Executor;
use inspectah_core::types::units::{LinkRelation, UnitName, UnitState};
use std::collections::BTreeMap;
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

/// A link to this path masks a unit; it is not an alias.
const DEV_NULL: &str = "/dev/null";

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
    /// Under `/usr/lib/systemd/system`, the links packages ship (source 4).
    /// They complete an install and never start one.
    pub(crate) vendor: LinksByUnit,
}

impl HostLinks {
    /// The links under `/etc/systemd/system` to `unit`.
    pub(crate) fn admin(&self, unit: &UnitName) -> &[Link] {
        self.admin.get(unit).map_or(&[], Vec::as_slice)
    }

    /// The links a package ships to `unit`.
    pub(crate) fn vendor(&self, unit: &UnitName) -> &[Link] {
        self.vendor.get(unit).map_or(&[], Vec::as_slice)
    }
}

/// Read the links under both unit directories. A listing of
/// `/etc/systemd/system` that fails is left to the drop-in pass, which
/// lists the same directory and reports it; any other read that fails is
/// returned as `<path>: <error>` in the second value. A link whose own name
/// or parent does not parse as a [`UnitName`] is skipped.
pub(crate) fn read_host_links(exec: &dyn Executor) -> (HostLinks, Vec<String>) {
    let mut failures = Vec::new();
    let links = HostLinks {
        admin: read_links(exec, ETC_UNIT_DIR, &mut failures),
        vendor: read_links(exec, VENDOR_UNIT_DIR, &mut failures),
    };
    (links, failures)
}

/// The links in one unit directory: every entry of its `.wants`,
/// `.requires` and `.upholds` directories, and every link directly under it
/// to another unit's name, other than `default.target` and a mask.
fn read_links(exec: &dyn Executor, root: &str, failures: &mut Vec<String>) -> LinksByUnit {
    let mut links = LinksByUnit::new();
    let entries = match exec.read_dir(Path::new(root)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound || root == ETC_UNIT_DIR => {
            return links;
        }
        Err(e) => {
            failures.push(format!("{root}: {e}"));
            return links;
        }
    };
    for entry in entries {
        let path = format!("{root}/{entry}");
        let dependency = DEPENDENCY_DIRS.iter().find_map(|(suffix, relation)| {
            entry
                .strip_suffix(suffix)
                .map(|parent| (parent.to_owned(), *relation))
        });
        if let Some((parent, relation)) = dependency {
            let Ok(parent) = UnitName::try_from(parent) else {
                continue;
            };
            let names = match exec.read_dir(Path::new(&path)) {
                Ok(names) => names,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => {
                    failures.push(format!("{path}: {e}"));
                    continue;
                }
            };
            for name in names {
                if let Ok(unit) = UnitName::try_from(name) {
                    links.entry(unit).or_default().push(Link {
                        relation,
                        parent: parent.clone(),
                    });
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
        let target = match exec.read_link(Path::new(&path)) {
            Ok(target) => target,
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
                failures.push(format!("{path}: {e}"));
                continue;
            }
        };
        if target == DEV_NULL {
            continue;
        }
        let unit = Path::new(&target)
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| UnitName::try_from(name.to_owned()).ok());
        match unit {
            Some(unit) if unit != alias => links.entry(unit).or_default().push(Link {
                relation: LinkRelation::Alias,
                parent: alias,
            }),
            _ => {}
        }
    }
    links
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
/// `admin` the links to it under `/etc/systemd/system` and `vendor` the
/// links a package ships to it (§ 7.3 of the units spec, the table).
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
            (admin.is_empty() && !assigns_also).then_some(UnitState::Disabled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::types::units::InstallUnknown;

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
            assert_eq!(install_state(&read, &admin, &vendor), expected, "{case}");
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
}
