//! The units `Also=` reaches from a unit, and what the links say about
//! each (§ 7.3 of the units spec, "`Also=` members").
//!
//! `enable` and `disable` act on every unit `Also=` names, and on every
//! unit those name in turn. The walk follows the names through each
//! member's effective unit until no new name appears, the way the two verbs
//! follow them at build, and records each with its [`MemberLinks`].

use super::install::{InstallRead, InstallSection};
use super::install_evidence;
use super::links::{self, HostLinks};
use super::resolve::{self, ResolveError, Resolved};
use inspectah_core::traits::executor::Executor;
use inspectah_core::types::units::{AlsoMember, MemberLinks, UnitName, UnitState};
use std::collections::{BTreeSet, VecDeque};

/// The name `Also=` stands for (§ 7.3): a template stands for its
/// `DefaultInstance=`, read from the template's effective `[Install]`.
/// `Err` holds the template's name when it has none, or when its section
/// could not be read for one.
pub(crate) fn also_name(
    exec: &dyn Executor,
    name: &UnitName,
    read_failures: &mut Vec<String>,
) -> Result<UnitName, UnitName> {
    if !name.is_template() {
        return Ok(name.clone());
    }
    let section = match effective_read(exec, name, read_failures) {
        Some(InstallRead::Known(section)) => section,
        _ => return Err(name.clone()),
    };
    let Some(instance) = section.default_instance else {
        return Err(name.clone());
    };
    // `prefix@.suffix` becomes `prefix@instance.suffix`; an instance that
    // does not make a unit name is no instance of it.
    let (prefix, suffix) = name
        .as_str()
        .split_once("@.")
        .expect("a template name holds `@.`");
    UnitName::try_from(format!("{prefix}@{instance}.{suffix}")).map_err(|_| name.clone())
}

/// The effective `[Install]` of `name`, or `None` when it has no unit file
/// this scan reads, or a file it applies could not be read as text. A read
/// that fails goes to `read_failures`.
fn effective_read(
    exec: &dyn Executor,
    name: &UnitName,
    read_failures: &mut Vec<String>,
) -> Option<InstallRead> {
    match resolve::resolve(exec, name) {
        Ok(Resolved::File { fragment }) => match install_evidence(exec, name, &fragment) {
            Ok(evidence) => evidence.read,
            Err(failure) => {
                read_failures.push(failure);
                None
            }
        },
        Err(ResolveError::Read { path, error }) => {
            read_failures.push(format!("{path}: {error}"));
            None
        }
        _ => None,
    }
}

/// What one member is: its links, its vendor drop-in names, and the names
/// its own `Also=` gives, which the walk follows. A member whose section
/// could not be read, or cannot be relied on, gives none.
struct Member {
    links: MemberLinks,
    vendor_drop_ins: Vec<String>,
    also: Vec<UnitName>,
}

/// The [`Member`] `name` is on the source host (§ 6.3, § 7.3). `Unlinked`
/// and `Inert` count every link to it under `/etc/systemd/system`, not
/// only the ones its `[Install]` names; vendor links complete an install
/// and never start one, as in § 7.3's table.
fn member(
    exec: &dyn Executor,
    name: &UnitName,
    host_links: &HostLinks,
    read_failures: &mut Vec<String>,
) -> Member {
    let other = Member {
        links: MemberLinks::Other,
        vendor_drop_ins: Vec::new(),
        also: Vec::new(),
    };
    // A read under `/etc` failed, so a link to the member may be missing.
    if host_links.admin_unreadable {
        return other;
    }
    let linked = host_links.any_admin_link(name);
    let nothing_to_make = if linked {
        MemberLinks::Other
    } else {
        MemberLinks::Inert
    };
    let fragment = match resolve::resolve(exec, name) {
        Ok(Resolved::File { fragment }) => fragment,
        Ok(Resolved::NotFound) => {
            return Member {
                links: nothing_to_make,
                ..other
            };
        }
        Err(ResolveError::Read { path, error }) => {
            read_failures.push(format!("{path}: {error}"));
            return other;
        }
        // A mask, an alias, a unit the search path refuses, a link
        // elsewhere: neither verb reproduces it.
        Ok(_) | Err(ResolveError::Linked { .. }) => return other,
    };
    let evidence = match install_evidence(exec, name, &fragment) {
        Ok(evidence) => evidence,
        Err(failure) => {
            read_failures.push(failure);
            return other;
        }
    };
    let vendor_drop_ins = evidence.vendor_drop_ins;
    let (links, also) = match evidence.read {
        Some(InstallRead::Absent) => (nothing_to_make, Vec::new()),
        Some(InstallRead::Known(section)) => (
            known_links(name, &section, host_links, nothing_to_make),
            section.also,
        ),
        // The section cannot be relied on, or a file it applies is not
        // text: the walk does not continue through it.
        Some(InstallRead::Undetermined { .. }) | None => (MemberLinks::Other, Vec::new()),
    };
    Member {
        links,
        vendor_drop_ins,
        also,
    }
}

/// The links of a member whose section is `Known`: `nothing_to_make` when
/// it names no link; `Installed` when § 7.3's table reads `Enabled`;
/// `Unlinked` when no link to it is under `/etc`; `Other` otherwise.
fn known_links(
    name: &UnitName,
    section: &InstallSection,
    host_links: &HostLinks,
    nothing_to_make: MemberLinks,
) -> MemberLinks {
    if links::expected_links(section).is_empty() {
        return nothing_to_make;
    }
    let read = InstallRead::Known(section.clone());
    let state = links::install_state(
        &read,
        host_links.admin(name),
        host_links.any_admin_link(name),
        host_links.vendor(name),
    );
    match state {
        Some(UnitState::Enabled) => MemberLinks::Installed,
        _ if !host_links.any_admin_link(name) => MemberLinks::Unlinked,
        _ => MemberLinks::Other,
    }
}

/// Every unit `Also=` reaches from `owner`, whose effective section names
/// `also`, in the order the walk reaches them, `owner` left out (§ 7.3). A
/// template stands for its `DefaultInstance=`; with none, it is recorded
/// under its own name as `Other` and the walk does not go through it.
pub(crate) fn also_members(
    exec: &dyn Executor,
    owner: &UnitName,
    also: &[UnitName],
    host_links: &HostLinks,
    read_failures: &mut Vec<String>,
) -> Vec<AlsoMember> {
    let mut seen = BTreeSet::from([owner.clone()]);
    let mut pending: VecDeque<UnitName> = also.iter().cloned().collect();
    let mut members = Vec::new();
    while let Some(named) = pending.pop_front() {
        let name = match also_name(exec, &named, read_failures) {
            Ok(name) => name,
            Err(template) => {
                if seen.insert(template.clone()) {
                    members.push(AlsoMember {
                        unit: template,
                        links: MemberLinks::Other,
                        vendor_drop_ins: Vec::new(),
                    });
                }
                continue;
            }
        };
        if !seen.insert(name.clone()) {
            continue;
        }
        let found = member(exec, &name, host_links, read_failures);
        pending.extend(found.also);
        members.push(AlsoMember {
            unit: name,
            links: found.links,
            vendor_drop_ins: found.vendor_drop_ins,
        });
    }
    members
}

/// Whether a member's links make an owner at its preset a state change
/// (§ 7.4): `Unlinked` and `Other` do, `Installed` and `Inert` do not.
pub(crate) fn is_off(member: &AlsoMember) -> bool {
    matches!(member.links, MemberLinks::Unlinked | MemberLinks::Other)
}
