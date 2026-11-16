use inspectah_core::baseline::{
    STOCK_STATE, apply_runtime_updater_default, target_needs_unit, target_needs_unit_text,
};
use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::FindingKind;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::finding::ShadowType;
use inspectah_core::types::redaction::{Confidence, RedactionHint};
use inspectah_core::types::units::{
    AdminStorageFragment, AdvisoryUnit, AlsoMember, Classified, DefaultTarget,
    INSTANCE_READS_UNKNOWN, InstallUnknown, ManualLink, MountUnitFinding, PresetDefault,
    StorageKind, SystemdDropIn, TargetUnit, UNIT_FILE_WHITESPACE, UNIT_TYPE_SUFFIXES,
    UndeterminedInstall, UnitFragment, UnitInventoryEntry, UnitKind, UnitName, UnitSection,
    UnitState, UnitStateChange, UnitSubject, holds_install_section, logical_lines,
};
use inspectah_core::types::warnings::{Warning, WarningSeverity};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

mod also;
pub(crate) mod install;
mod instance;
pub(crate) mod links;
mod resolve;
pub(crate) mod specifiers;

use install::{
    DropInRoot, DropInSource, InstallRead, InstallSection, drop_in_dirs, read_install,
    read_vendor_install,
};
use instance::NeedsUnknown;
use links::{HostLinks, InstallDifference};
use resolve::{ETC_UNIT_DIR, Entry, ResolveError, Resolved, VENDOR_UNIT_DIR};

/// Secret-like environment variable name fragments that trigger redaction hints.
const SECRET_PATTERNS: &[&str] = &["PASSWORD", "SECRET", "TOKEN", "KEY", "CREDENTIAL"];

/// Why a full shadow matters, recorded on its drop-in row.
const FULL_SHADOW_RATIONALE: &str = "Full shadow: this unit file in /etc/systemd/system/ overrides \
     the vendor unit in /usr/lib/systemd/system/. Base image updates to this unit will be \
     silently ignored.";

/// The prefix of a D-Bus activation alias (`dbus-org.freedesktop.*`): a
/// link D-Bus activation manages, not a unit an administrator switched.
const DBUS_ACTIVATION_ALIAS_PREFIX: &str = "dbus-org.";

/// The type suffixes of the unit kinds § 5 of the units spec excludes
/// outright: no unit file of these kinds exists, and the scan records
/// nothing of their state.
const EXCLUDED_KIND_SUFFIXES: &[&str] = &["device", "scope"];

/// The `list-unit-files` states, dropped without a warning, that a unit
/// is still read under when a link under `/etc/systemd/system` names it
/// (§ 7.3 of the units spec), or when it has a file or drop-in directory
/// of its own name there (§ 7.4): systemctl's word is not what decides its
/// install state.
const LINK_READ_STATES: &[&str] = &["static", "indirect"];

/// The name gate (§ 7.4 of the units spec): the name of a listed unit the
/// section reads, with the listing's state word, or `None` for one it does
/// not. A unit of a kind the scan excludes ([`EXCLUDED_KIND_SUFFIXES`]) is
/// dropped first, with no warning whatever its state, so an open login
/// session's transient scope is not reported on every scan. A state that is
/// not durable is then dropped or warned about by [`parse_unit_state`],
/// except that a `static` or `indirect` unit ([`LINK_READ_STATES`]) is kept
/// with no state word when a link under `/etc` names it, or when it has a
/// file or drop-in directory of its own name there: a unit whose `/etc`
/// files leave it no link reads `static`, and § 7.4 compares it with its
/// package's. The name must parse
/// as a [`UnitName`] and not be a D-Bus activation alias. A template is
/// recorded only when masked, since its mask masks every instance;
/// enabling a template means nothing without an instance. A listed
/// instance (a concrete instance file, or its mask) is recorded like any
/// unit; an instance backed only by its template has no row, and the links
/// find it ([`linked_instances`]).
///
/// The words `enabled` and `disabled` are kept only for a unit whose
/// `[Install]` cannot be read as text; every other unit's install state is
/// read from the links ([`links::install_state`]).
fn gate(
    unit: &UnitFileEntry,
    host_links: &HostLinks,
    warnings: &mut Vec<Warning>,
) -> Option<(UnitName, Option<UnitState>)> {
    if unit
        .unit
        .rsplit_once('.')
        .is_some_and(|(_, suffix)| EXCLUDED_KIND_SUFFIXES.contains(&suffix))
    {
        return None;
    }
    let (name, state) = match parse_unit_state(&unit.unit, &unit.state) {
        ParsedUnitState::Durable(state) => (
            proved_unit_name(&unit.unit, &unit.state, warnings)?,
            Some(state),
        ),
        ParsedUnitState::SilentDrop if LINK_READ_STATES.contains(&unit.state.as_str()) => {
            let name = UnitName::try_from(unit.unit.clone()).ok()?;
            if !host_links.admin.contains_key(&name) && !host_links.has_etc_files(&name) {
                return None;
            }
            (name, None)
        }
        ParsedUnitState::SilentDrop => return None,
        ParsedUnitState::Warn(warning) => {
            warnings.push(warning);
            return None;
        }
    };
    if name.as_str().starts_with(DBUS_ACTIVATION_ALIAS_PREFIX) {
        return None;
    }
    if name.is_template() && state != Some(UnitState::Masked) {
        return None;
    }
    Some((name, state))
}

/// The state word recorded, for warnings only, for an instance found by a
/// link under `/etc/systemd/system`: `list-unit-files` has no row for it,
/// and systemd reads any unit with such a link as `enabled`.
const LINKED_INSTANCE_STATE: &str = "enabled";

/// The instances a link under `/etc/systemd/system` names that the listing
/// does not (§ 7.1, § 7.3 and § 7.5 of the units spec). `listing` holds
/// every name `list-unit-files` gave, before the name gate: a listed
/// instance the gate refused (`bad`, `transient`, a runtime state) stays
/// refused, and one it kept is read once. `list-unit-files` shows an
/// instance backed only by its template as the template reading
/// `indirect`, so its links are what find it. Its install state is read
/// from the links like any unit's, so it has no listing word
/// ([`Listed::listed`] is `None`). A link under `/run` is not read, and an
/// instance with no link under `/etc` is not found.
fn linked_instances(host_links: &HostLinks, listing: &HashSet<UnitName>) -> Vec<Listed> {
    host_links
        .admin
        .keys()
        .filter(|name| name.instance().is_some() && !listing.contains(*name))
        .filter(|name| !name.as_str().starts_with(DBUS_ACTIVATION_ALIAS_PREFIX))
        .map(|name| Listed {
            name: name.clone(),
            raw_state: LINKED_INSTANCE_STATE.to_owned(),
            listed: None,
        })
        .collect()
}

/// The state word recorded, for warnings only, for an instance derived from
/// the presets or from `Also=` ([`preset_instances`], [`derivable`]): it
/// has no listing row and no link under `/etc/systemd/system`, and systemd
/// reads such an instance as `disabled`.
const DERIVED_INSTANCE_STATE: &str = "disabled";

/// Whether `name` is a candidate instance § 7.5 of the units spec derives:
/// an instance not yet found (`known` holds every name the listing gave
/// and every instance its links found), with no link under
/// `/etc/systemd/system`, of a template not tied to a host's devices, and
/// not a D-Bus activation alias. An instance with a link of its own is
/// found by it ([`linked_instances`]) or reported where its link is read.
fn derivable(name: &UnitName, known: &HashSet<UnitName>, host_links: &HostLinks) -> bool {
    name.instance()
        .is_some_and(|parts| !links::HOST_DEVICE_TEMPLATES.contains(&parts.template().as_str()))
        && !known.contains(name)
        && !host_links.any_admin_link(name)
        && !name.as_str().starts_with(DBUS_ACTIVATION_ALIAS_PREFIX)
}

/// The instances the image's package install would link from the presets
/// (§ 7.1 source 6 and § 7.5 of the units spec, "A disabled instance is
/// found from the presets"), for the templates the listing gives. For each
/// template whose first matching rule is a bare `enable`, the candidate is
/// its `DefaultInstance=`, read from the template's effective `[Install]`;
/// for the list form, each listed instance. A `disable` rule, a rule this
/// scan does not read, or no rule derives none, and neither does a masked
/// template, whose mask covers every instance. A candidate the listing
/// gives (an instance with its own unit file) or that has a link under
/// `/etc` is not derived again ([`derivable`]). Nothing new is read from
/// the host beyond the template's own files.
fn preset_instances(
    exec: &dyn Executor,
    units: &[UnitFileEntry],
    rules: &[PresetRule],
    known: &HashSet<UnitName>,
    host_links: &HostLinks,
    read_failures: &mut Vec<String>,
) -> Vec<Listed> {
    let mut derived: Vec<UnitName> = Vec::new();
    for unit in units.iter().filter(|unit| unit.state != "masked") {
        let Ok(template) = UnitName::try_from(unit.unit.clone()) else {
            continue;
        };
        if !template.is_template() {
            continue;
        }
        let candidates: Vec<UnitName> = match first_rule(template.as_str(), rules) {
            Some(PresetRule::Plain {
                verb: PresetDefault::Enable,
                ..
            }) => also::also_name(exec, &template, read_failures)
                .ok()
                .into_iter()
                .collect(),
            Some(PresetRule::EnableInstances { instances, .. }) => instances
                .iter()
                .filter_map(|instance| also::instance_name(&template, instance))
                .collect(),
            _ => Vec::new(),
        };
        for name in candidates {
            if derivable(&name, known, host_links) && !derived.contains(&name) {
                derived.push(name);
            }
        }
    }
    derived
        .into_iter()
        .map(|name| Listed {
            name,
            raw_state: DERIVED_INSTANCE_STATE.to_owned(),
            listed: None,
        })
        .collect()
}

/// Result of parsing a raw systemd unit state string into the typed contract.
enum ParsedUnitState {
    /// Durable administrator intent: produces a `UnitStateChange`.
    Durable(UnitState),
    /// Known-inert packaging artifact: silently dropped, no warning.
    SilentDrop,
    /// Runtime/linked/bad/unknown: emits a structured warning and drops.
    Warn(Warning),
}

/// Classify a raw systemd unit-file state into the typed contract.
fn parse_unit_state(unit: &str, raw_state: &str) -> ParsedUnitState {
    match raw_state {
        "enabled" => ParsedUnitState::Durable(UnitState::Enabled),
        "disabled" => ParsedUnitState::Durable(UnitState::Disabled),
        "masked" => ParsedUnitState::Durable(UnitState::Masked),
        // Packaging/dependency artifacts: no migration relevance.
        "static" | "alias" | "indirect" | "generated" => ParsedUnitState::SilentDrop,
        // Runtime/volatile states: lost on reboot, cannot be migrated.
        "enabled-runtime" | "masked-runtime" | "transient" | "linked-runtime" => {
            ParsedUnitState::Warn(service_warning(
                unit,
                raw_state,
                "transient state is not migrated",
            ))
        }
        // Linked units require manual handling.
        "linked" => ParsedUnitState::Warn(service_warning(
            unit,
            raw_state,
            "linked unit requires manual handling",
        )),
        // Bad unit files are invalid/unreadable.
        "bad" => ParsedUnitState::Warn(service_warning(
            unit,
            raw_state,
            "unit file is invalid or unreadable",
        )),
        // Future-proof catch-all for unrecognized states.
        other => ParsedUnitState::Warn(service_warning(unit, other, "unrecognized systemd state")),
    }
}

/// Build a structured warning for a non-durable service state.
fn service_warning(unit: &str, raw_state: &str, detail: &str) -> Warning {
    Warning {
        inspector: "units".into(),
        message: format!("unit {unit} has state '{raw_state}' - {detail}"),
        severity: Some(WarningSeverity::Warning),
        extra: std::collections::HashMap::from([
            ("unit".into(), serde_json::json!(unit)),
            ("raw_state".into(), serde_json::json!(raw_state)),
        ]),
    }
}

/// Parse the name a state change will carry. A name that does not parse
/// is reported through the inspector's warnings and yields no state change.
fn proved_unit_name(unit: &str, raw_state: &str, warnings: &mut Vec<Warning>) -> Option<UnitName> {
    match UnitName::try_from(unit.to_owned()) {
        Ok(name) => Some(name),
        Err(e) => {
            warnings.push(service_warning(unit, raw_state, &e.to_string()));
            None
        }
    }
}

/// What the host's presets give one unit (§ 7.4 of the units spec).
#[derive(Debug, Clone, PartialEq, Eq)]
enum PresetLookup {
    Rule(PresetDefault),
    /// The first matching line has a verb this scan does not read. The
    /// lookup stops there, with no preset.
    Unknown {
        file: String,
        line: usize,
    },
    NoRule,
}

/// The preset of `unit` from `rules`, in the order systemd reads them
/// (§ 7.4 of the units spec). A unit that is not an instance, and an
/// instance with its own unit file (`own_file`), take the first rule that
/// matches their own name: a rule naming it, a glob matching it, or a
/// list-form rule listing the instance. A bare or list-form template rule
/// that does not list such an instance does not match it, and later rules
/// decide. An instance backed only by its template takes the template's
/// first matching rule: a plain `enable` gives `enable` to
/// `default_instance` only, the list form to the listed instances only,
/// and `disable` reaches every instance. Rules that name an instance are
/// not read for it.
fn preset_for(
    unit: &UnitName,
    own_file: bool,
    default_instance: Option<&str>,
    rules: &[PresetRule],
) -> PresetLookup {
    let lookup = |rule: &PresetRule, verb| match rule {
        PresetRule::Unknown { file, line, .. } => PresetLookup::Unknown {
            file: file.clone(),
            line: *line,
        },
        _ => PresetLookup::Rule(verb),
    };
    match unit.instance() {
        Some(parts) if !own_file => {
            let template = parts.template();
            let instance = parts.instance();
            match first_rule(template.as_str(), rules) {
                None => PresetLookup::NoRule,
                Some(rule @ PresetRule::Unknown { .. }) => lookup(rule, PresetDefault::Disable),
                Some(PresetRule::Plain {
                    verb: PresetDefault::Enable,
                    ..
                }) if default_instance == Some(instance) => {
                    PresetLookup::Rule(PresetDefault::Enable)
                }
                Some(PresetRule::EnableInstances { instances, .. })
                    if instances.iter().any(|listed| listed == instance) =>
                {
                    PresetLookup::Rule(PresetDefault::Enable)
                }
                Some(_) => PresetLookup::Rule(PresetDefault::Disable),
            }
        }
        _ => {
            let found = rules.iter().find(|rule| match (rule, unit.instance()) {
                (
                    PresetRule::EnableInstances {
                        template,
                        instances,
                    },
                    Some(parts),
                ) => {
                    parts.template().as_str() == template
                        && instances.iter().any(|listed| listed == parts.instance())
                }
                _ => rule.matches(unit.as_str()),
            });
            match found {
                None => PresetLookup::NoRule,
                Some(rule @ PresetRule::Plain { verb, .. }) => lookup(rule, *verb),
                Some(rule) => lookup(rule, PresetDefault::Enable),
            }
        }
    }
}

/// The first rule that matches `name` as a name of its own.
fn first_rule<'r>(name: &str, rules: &'r [PresetRule]) -> Option<&'r PresetRule> {
    rules.iter().find(|rule| rule.matches(name))
}

const OWNING_PACKAGE_QUERY: &str = "%{NAME}\n";

/// The `owning_package` of each `Vendor` and `Shadowed` fragment, which
/// holds the vendor file's name: `Local` and `Absent` have no vendor file.
fn vendor_owner_slot(fragment: &mut UnitFragment) -> Option<(&UnitName, &mut Option<String>)> {
    match fragment {
        UnitFragment::Vendor {
            file,
            owning_package,
        }
        | UnitFragment::Shadowed {
            file,
            owning_package,
        } => Some((file, owning_package)),
        UnitFragment::Local { .. } | UnitFragment::Absent => None,
    }
}

/// Fill in the owning package of every `Vendor` and `Shadowed` fragment
/// via a batch `rpm -qf` lookup on the vendor file the fragment names.
/// Falls back to per-file queries when the batch fails or line count
/// mismatches. An empty or unrecognized answer leaves the owner unknown.
///
/// Runs before any unit is classified, so a vendor mount unit whose query
/// fails is advisory, not carried (§ 7.8 of the units spec).
fn populate_owning_packages(exec: &dyn Executor, fragments: &mut [UnitFragment]) {
    let mut slots: Vec<(&UnitName, &mut Option<String>)> =
        fragments.iter_mut().filter_map(vendor_owner_slot).collect();
    if slots.is_empty() {
        return;
    }

    let batch_paths: Vec<String> = slots
        .iter()
        .map(|(file, _)| format!("{VENDOR_UNIT_DIR}/{}", file.as_str()))
        .collect();

    let batch_args: Vec<&str> = std::iter::once("-qf")
        .chain(std::iter::once("--queryformat"))
        .chain(std::iter::once(OWNING_PACKAGE_QUERY))
        .chain(batch_paths.iter().map(|s| s.as_str()))
        .collect();

    let result = exec.run("rpm", &batch_args);
    let batch_lines: Vec<&str> = result.stdout.lines().collect();

    if result.success() && batch_lines.len() == slots.len() {
        for ((_, owner), line) in slots.iter_mut().zip(batch_lines) {
            **owner = package_name(line);
        }
        return;
    }

    // Batch failed or line count mismatch: fall back to per-file queries.
    for ((_, owner), path) in slots.iter_mut().zip(&batch_paths) {
        **owner = query_owning_package(exec, path);
    }
}

/// A package name from one line of `rpm -qf` output: `None` for an empty
/// line or a message such as "file ... is not owned by any package".
fn package_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    (!trimmed.is_empty() && !trimmed.contains(' ')).then(|| trimmed.to_string())
}

/// Query the owning RPM package for one vendor unit file.
fn query_owning_package(exec: &dyn Executor, path: &str) -> Option<String> {
    let result = exec.run("rpm", &["-qf", "--queryformat", OWNING_PACKAGE_QUERY, path]);
    if !result.success() {
        return None;
    }
    result.stdout.lines().next().and_then(package_name)
}

/// A unit past the name gate: its name, the state word `systemctl`
/// printed, and that word as a state when it is a durable one. An instance
/// found by its links ([`linked_instances`]) has no listing row: its word is
/// [`LINKED_INSTANCE_STATE`], for warnings only, and it has no state.
struct Listed {
    name: UnitName,
    raw_state: String,
    listed: Option<UnitState>,
}

/// A unit with an install state or a mask, and its preset.
struct Candidate {
    name: UnitName,
    /// The state word `systemctl` printed, for warnings.
    raw_state: String,
    state: UnitState,
    default_state: Option<PresetDefault>,
}

/// Whether a candidate whose unit file resolved to `fragment` is a state
/// change (§ 7.4 of the units spec). A mask always is. A local unit is when
/// enabled, whatever its preset: the image has the unit only if the export
/// carries it, and a copied unit file is not enabled until something
/// enables it. Any other unit is when its state differs from its preset.
///
/// A unit in the state a stock install leaves it in ([`STOCK_STATE`]) is
/// not a state change: nobody chose it.
fn is_state_change(candidate: &Candidate, fragment: &UnitFragment) -> bool {
    if STOCK_STATE
        .iter()
        .any(|stock| stock.unit == candidate.name.as_str() && stock.state == candidate.state)
    {
        return false;
    }
    if candidate.state == UnitState::Masked {
        return true;
    }
    if matches!(fragment, UnitFragment::Local { .. }) {
        return candidate.state == UnitState::Enabled;
    }
    matches!(
        (candidate.state, candidate.default_state),
        (UnitState::Enabled, Some(PresetDefault::Disable))
            | (UnitState::Disabled, Some(PresetDefault::Enable))
    )
}

/// Whether a candidate is an enabled storage unit. An administrator's one
/// is a mount finding whatever its preset (§ 6.5 of the units spec), so it
/// is kept until its owner is known, even when its state is no change.
fn is_enabled_storage(candidate: &Candidate) -> bool {
    matches!(candidate.name.kind(), UnitKind::Storage(_)) && candidate.state == UnitState::Enabled
}

/// What [`build_state_changes`] finds for the listed units.
#[derive(Debug, Default)]
struct UnitFindings {
    inventory: Vec<UnitInventoryEntry>,
    state_changes: Vec<UnitStateChange>,
    mount_findings: Vec<MountUnitFinding>,
    /// The links reported and not carried (§ 6.6 of the units spec).
    manual_links: Vec<ManualLink>,
    /// The units whose `[Install]` cannot be relied on and whose `/etc`
    /// files change it, or that assign `Also=` (§ 6.6).
    undetermined_installs: Vec<UndeterminedInstall>,
    /// The masked units and the units the search path refuses, whose links
    /// under `/etc` are therefore not read (§ 7.3), for the warnings.
    unreported: Vec<(UnitName, UnreportedLinks)>,
}

/// Resolve each listed unit's file, read its install state from the links
/// (§ 7.3 of the units spec) and its preset, and record it in the
/// inventory; keep the state changes and the enabled storage units, look
/// up the owners, then classify: a carried unit is a state change, an
/// administrator's storage unit a mount finding. The links reported and
/// not carried, and the undetermined install records, come from the same
/// read of the links, for every unit whose section was read, with an
/// install state or not. A unit with no install state has no inventory
/// entry. A unit that cannot be resolved or classified is reported through
/// `warnings`; a unit file that could not be read goes to `read_failures`. The names refused by the search path,
/// unreadable, or with an undetermined `[Install]` section go to
/// `reported`, so the storage pass does not report them again.
///
/// Once every unit in `listed` is read, the instances an enabled unit's
/// `Also=` reached are read the same way, each one [`derivable`] against
/// `known` (every name already found) and the links (§ 7.5).
#[allow(clippy::too_many_arguments)] // the scan's state, read once by the caller
fn build_state_changes(
    exec: &dyn Executor,
    listed: Vec<Listed>,
    host_links: &HostLinks,
    rules: &[PresetRule],
    known: &HashSet<UnitName>,
    warnings: &mut Vec<Warning>,
    read_failures: &mut Vec<String>,
    reported: &mut HashSet<UnitName>,
) -> UnitFindings {
    let mut inventory = Vec::new();
    let mut manual_links = Vec::new();
    let mut undetermined_installs = Vec::new();
    let mut unreported = Vec::new();
    let mut unknown_lines: BTreeMap<(String, usize), Vec<UnitName>> = BTreeMap::new();
    let mut found = Vec::new();
    let mut fragments = Vec::new();
    // § 7.5: the instances an enabled unit's `Also=` reaches, collected as
    // the units are read, then read themselves once every listed unit is.
    let mut known = known.clone();
    let mut also_reached: Vec<UnitName> = Vec::new();
    let mut also_read = false;
    let mut queue: std::collections::VecDeque<Listed> = listed.into();
    loop {
        let Some(subject) = queue.pop_front() else {
            if also_read {
                break;
            }
            also_read = true;
            for name in std::mem::take(&mut also_reached) {
                if derivable(&name, &known, host_links) {
                    known.insert(name.clone());
                    queue.push_back(Listed {
                        name,
                        raw_state: DERIVED_INSTANCE_STATE.to_owned(),
                        listed: None,
                    });
                }
            }
            continue;
        };
        let warn =
            |detail: &str| service_warning(subject.name.as_str(), &subject.raw_state, detail);
        let (fragment, state, evidence, additive) = match resolve::resolve(exec, &subject.name) {
            Ok(Resolved::File { fragment }) if subject.listed == Some(UnitState::Masked) => {
                unreported.push((subject.name.clone(), UnreportedLinks::Masked));
                (
                    fragment,
                    UnitState::Masked,
                    InstallEvidence::default(),
                    false,
                )
            }
            // A read under `/etc/systemd/system` failed, so links there may
            // be missing, and a state read from the rest could carry a
            // command the host never had. The scan degrades, and no unit
            // that is not masked gets an install state.
            Ok(Resolved::File { .. }) if host_links.admin_unreadable => continue,
            Ok(Resolved::File { fragment }) => {
                let evidence = match install_evidence(exec, &subject.name, &fragment) {
                    Ok(evidence) => evidence,
                    Err(failure) => {
                        read_failures.push(failure);
                        reported.insert(subject.name.clone());
                        continue;
                    }
                };
                if !evidence.unreadable.is_empty() {
                    warnings.push(unreadable_install_warning(warn, &evidence.unreadable));
                }
                // An undetermined section, an unreadable one included, leaves
                // the state unknown, so the storage pass must not record the
                // unit as one that is not enabled. Its record says why.
                if matches!(evidence.read, Some(InstallRead::Undetermined { .. })) {
                    reported.insert(subject.name.clone());
                }
                let mut additive = false;
                let state = match &evidence.read {
                    Some(read) => {
                        let install_files = etc_install_files(&fragment, &evidence);
                        let mut found =
                            links::link_findings(&subject.name, read, host_links, &install_files);
                        let difference = install_difference(
                            exec,
                            &subject.name,
                            &fragment,
                            &evidence,
                            found.state,
                            &install_files,
                            rules,
                            read_failures,
                        );
                        if let Some(difference) = difference {
                            additive = links::apply_install_difference(
                                &mut found,
                                &subject.name,
                                difference,
                                host_links,
                                &install_files,
                            );
                        }
                        // A package section that cannot be relied on leaves
                        // the state unknown too.
                        if found.undetermined.is_some() {
                            reported.insert(subject.name.clone());
                        }
                        manual_links.extend(found.manual_links);
                        undetermined_installs.extend(found.undetermined);
                        found.state
                    }
                    // A unit file always resolves to a fragment with a file,
                    // so its section is always read; none is no state.
                    None => None,
                };
                // No install state: the unit has no enablement of its own,
                // or neither verb reproduces its links (§ 7.3).
                let Some(state) = state else {
                    continue;
                };
                (fragment, state, evidence, additive)
            }
            // A mask reads no `[Install]`, so it records no install files.
            Ok(Resolved::Masked { hidden }) => {
                unreported.push((subject.name.clone(), UnreportedLinks::Masked));
                (hidden, UnitState::Masked, InstallEvidence::default(), false)
            }
            Ok(Resolved::AliasOf(target)) => {
                warnings.push(warn(&format!(
                    "unit is an alias of {}; its state is not carried on its own",
                    target.as_str()
                )));
                continue;
            }
            Ok(Resolved::NotFound) => {
                warnings.push(warn(
                    "no unit file under /etc/systemd/system or /usr/lib/systemd/system",
                ));
                continue;
            }
            Ok(Resolved::Unsupported { path }) => {
                unreported.push((
                    subject.name.clone(),
                    UnreportedLinks::Refused { path: path.clone() },
                ));
                warnings.push(search_path_warning(warn, path, STATE_NOT_CARRIED));
                reported.insert(subject.name.clone());
                continue;
            }
            // Today's warning for a `linked` unit.
            Err(ResolveError::Linked { .. }) => {
                warnings.push(service_warning(
                    subject.name.as_str(),
                    "linked",
                    "linked unit requires manual handling",
                ));
                continue;
            }
            Err(ResolveError::Read { path, error }) => {
                read_failures.push(format!("{path}: {error}"));
                reported.insert(subject.name.clone());
                continue;
            }
        };

        let default_instance = match &evidence.read {
            Some(InstallRead::Known(section)) => section.default_instance.as_deref(),
            _ => None,
        };
        let own_file = fragment_file(&fragment) == Some(&subject.name);
        let preset = match preset_for(&subject.name, own_file, default_instance, rules) {
            PresetLookup::Rule(preset) => Some(preset),
            PresetLookup::NoRule => None,
            // The line decides only a unit whose result depends on its
            // preset: a `Vendor` or `Shadowed` one that is not masked. A
            // mask and an enabled local unit are state changes whatever the
            // presets say, and a local unit is never compared with one.
            PresetLookup::Unknown { file, line } => {
                let preset_decides = matches!(
                    fragment,
                    UnitFragment::Vendor { .. } | UnitFragment::Shadowed { .. }
                );
                if state != UnitState::Masked && preset_decides {
                    unknown_lines
                        .entry((file, line))
                        .or_default()
                        .push(subject.name.clone());
                }
                None
            }
        };
        inventory.push(UnitInventoryEntry {
            unit: subject.name.clone(),
            state,
            preset,
        });
        let candidate = Candidate {
            name: subject.name,
            raw_state: subject.raw_state,
            state,
            default_state: preset,
        };
        // An additive unit is an `Enabled` state change whatever its
        // preset says (§ 7.4).
        let state_change = additive || is_state_change(&candidate, &fragment);
        // § 7.4, "An owner at its preset whose member is off": a packaged
        // unit enabled under an `enable` preset has no row of its own, and
        // the image's package install enables every member with it. This
        // holds where the comparison differs too: the unit's findings name
        // the changed values, and the row names the member that is off.
        let at_preset = !state_change
            && candidate.state == UnitState::Enabled
            && candidate.default_state == Some(PresetDefault::Enable)
            && matches!(
                fragment,
                UnitFragment::Vendor { .. } | UnitFragment::Shadowed { .. }
            );
        // Every enabled unit's `Also=` is walked too, for the instances it
        // reaches (§ 7.5, "Instances named by an enabled unit's `Also=`").
        let enabled = candidate.state == UnitState::Enabled;
        let named = match &evidence.read {
            Some(InstallRead::Known(section)) if state_change || at_preset || enabled => {
                section.also.as_slice()
            }
            _ => &[],
        };
        let members = also::also_members(exec, &candidate.name, named, host_links, read_failures);
        if enabled {
            also_reached.extend(
                members
                    .iter()
                    .filter(|member| member.unit.instance().is_some())
                    .map(|member| member.unit.clone()),
            );
        }
        let member_off = at_preset && members.iter().any(also::is_off);
        let state_change = state_change || member_off;
        if state_change || is_enabled_storage(&candidate) {
            fragments.push(fragment);
            found.push(Found {
                candidate,
                state_change,
                evidence,
                additive,
                members,
                member_off,
            });
        }
    }
    for ((file, line), units) in unknown_lines {
        warnings.push(unknown_preset_warning(&file, line, &units));
    }

    populate_owning_packages(exec, &mut fragments);

    let mut state_changes = Vec::new();
    let mut mount_findings = Vec::new();
    for (
        Found {
            candidate,
            state_change,
            evidence,
            additive,
            members,
            member_off,
        },
        fragment,
    ) in found.into_iter().zip(fragments)
    {
        let warn =
            |detail: &str| service_warning(candidate.name.as_str(), &candidate.raw_state, detail);
        let subject = match UnitSubject::new(candidate.name.clone(), fragment) {
            Ok(subject) => subject,
            Err(e) => {
                warnings.push(warn(&e.to_string()));
                continue;
            }
        };
        let unit = match subject.classify() {
            Classified::Carried(unit) if state_change => unit,
            // An enabled storage unit at its preset, carried: the image
            // already has its state.
            Classified::Carried(_) => continue,
            // An administrator's storage unit is a mount finding, never a
            // state change (§ 6.5). The finding records the state when the
            // unit is enabled or masked. A disabled one here is not a
            // finding; a disabled one with a file under `/etc` is recorded
            // by [`etc_storage_findings`].
            Classified::Advisory(unit) => {
                if candidate.state != UnitState::Disabled {
                    mount_findings.push(MountUnitFinding {
                        unit,
                        state_change: Some((candidate.state, candidate.default_state)),
                        mount_point: None,
                        nofail: None,
                        aggregate: None,
                    });
                }
                continue;
            }
        };
        match UnitStateChange::new(unit, candidate.state, candidate.default_state) {
            Ok(mut change) => {
                // § 7.4: the `/etc` files add links the package's unit
                // does not make. The notes below take its place when they
                // apply.
                if additive {
                    change.attention_reason = Some(ADDS_LINKS_REASON.to_owned());
                }
                // § 7.4: a unit it enables through `Also=` is off on the
                // host.
                if member_off {
                    change.attention_reason = Some(MEMBER_OFF_REASON.to_owned());
                }
                change.also = members;
                // § 10.3: the plan refuses this command, and refine shows
                // why in the plan's own words.
                let name = change.unit().name().as_str();
                let action = change.implied_action();
                if target_needs_unit(name, action) {
                    change.attention_reason = Some(target_needs_unit_text(name, action));
                }
                // § 10.4: a runtime package updater starts excluded.
                apply_runtime_updater_default(&mut change);
                change.install_drop_ins = evidence.row_install_drop_ins();
                change.vendor_drop_ins = evidence.vendor_drop_ins;
                // § 7.5: what an enabled instance reads and keeps. Its
                // enable is the command that needs them (§ 10.8).
                if change.current_state() == UnitState::Enabled {
                    let needs = match instance::instance_needs(
                        exec,
                        change.unit().name(),
                        evidence
                            .fragment
                            .as_ref()
                            .map(|(path, text)| (path.as_str(), text.as_str())),
                    ) {
                        Ok(needs) => needs,
                        // What it reads is unknown, so rule 6 cannot
                        // check its enable: the row records why, and the
                        // plan omits the enable saying so. A read failure
                        // degrades the scan too, as any other does.
                        Err(unknown) => {
                            if let NeedsUnknown::Unreadable { path, error } = &unknown {
                                read_failures.push(format!("{path}: {error}"));
                            }
                            warnings.push(needs_unknown_warning(warn, &unknown));
                            change.needs_unknown = Some(unknown.record());
                            state_changes.push(change);
                            continue;
                        }
                    };
                    change.required_config = needs.required_config;
                    change.data_paths = needs.data_paths;
                    // § 10.2 rule 5: a runtime drop-in that removed or
                    // changed a path the lists would hold is evidence the
                    // export stages, as a drop-in hiding an install file
                    // is: without it the image reads that path unchecked.
                    if !needs.overriding_drop_ins.is_empty() {
                        let paths: BTreeSet<String> = change
                            .install_drop_ins
                            .drain(..)
                            .chain(needs.overriding_drop_ins)
                            .collect();
                        change.install_drop_ins = paths.into_iter().collect();
                    }
                    // § 7.5: an instance of a template on no list that
                    // names nothing it reads gets a note saying so; the
                    // plan adds that it is carried on its unit file alone
                    // once its enable is planned.
                    let unknown = change
                        .unit()
                        .name()
                        .instance()
                        .is_some_and(|parts| !instance::on_a_list(&parts.template()));
                    if unknown && change.required_config.is_empty() && change.data_paths.is_empty()
                    {
                        add_note(&mut change.attention_reason, INSTANCE_READS_UNKNOWN);
                    }
                }
                state_changes.push(change);
            }
            Err(e) => warnings.push(warn(&e.to_string())),
        }
    }
    UnitFindings {
        inventory,
        state_changes,
        mount_findings,
        manual_links,
        undetermined_installs,
        unreported,
    }
}

/// Add `note` to a row's attention reason, after any note it has.
fn add_note(reason: &mut Option<String>, note: &str) {
    *reason = Some(match reason.take() {
        Some(earlier) => format!("{earlier} {note}"),
        None => note.to_owned(),
    });
}

/// The attention note of an additive row (§ 7.4 of the units spec): the
/// unit's `/etc` files add links the package's unit does not make.
const ADDS_LINKS_REASON: &str = "Its /etc files add links the package's unit does not make.";

/// The attention note of an owner at its preset that is a row because a
/// unit its `Also=` reaches is `Unlinked` or `Other` (§ 7.4 of the units
/// spec, "An owner at its preset whose member is off"). It names no host,
/// so it reads true on a fleet aggregate's merged row too.
const MEMBER_OFF_REASON: &str = "A unit it enables through Also= is not enabled alongside it.";

/// A unit kept for a state change or as an enabled storage unit, with what
/// [`build_state_changes`] read for it.
struct Found {
    candidate: Candidate,
    state_change: bool,
    evidence: InstallEvidence,
    /// An additive row (§ 7.4).
    additive: bool,
    /// Every unit `Also=` reaches from it (§ 7.3), for a state change or an
    /// owner at its preset.
    members: Vec<AlsoMember>,
    /// An owner at its preset made a row by a member that is off (§ 7.4).
    member_off: bool,
}

/// § 7.4's comparison of `name`'s effective `[Install]` with its
/// package's, when the test applies: a `Vendor` or `Shadowed` fragment, an
/// install file under `/etc` (`install_files`), a preset of `enable`, and
/// install state `Enabled`, or no install state because the effective
/// section names no link (a `WantedBy=` reset with nothing added, or a
/// shadow with no `[Install]`). `None` when it does not apply, or when the
/// package's section could not be read as text.
#[allow(clippy::too_many_arguments)] // the scan's state, read once by the caller
fn install_difference(
    exec: &dyn Executor,
    name: &UnitName,
    fragment: &UnitFragment,
    evidence: &InstallEvidence,
    state: Option<UnitState>,
    install_files: &[String],
    rules: &[PresetRule],
    read_failures: &mut Vec<String>,
) -> Option<InstallDifference> {
    let packaged = matches!(
        fragment,
        UnitFragment::Vendor { .. } | UnitFragment::Shadowed { .. }
    );
    if !packaged || install_files.is_empty() {
        return None;
    }
    let effective = match evidence.read.as_ref()? {
        InstallRead::Known(section) => Some(section),
        InstallRead::Absent => None,
        InstallRead::Undetermined { .. } => return None,
    };
    let names_no_link = effective.is_none_or(|section| links::expected_links(section).is_empty());
    match state {
        Some(UnitState::Enabled) => {}
        None if names_no_link => {}
        _ => return None,
    }
    let own_file = fragment_file(fragment) == Some(name);
    let default_instance = effective.and_then(|section| section.default_instance.as_deref());
    let preset = preset_for(name, own_file, default_instance, rules);
    if !matches!(preset, PresetLookup::Rule(PresetDefault::Enable)) {
        return None;
    }
    let vendor = evidence.vendor_read.as_ref()?;
    // § 7.4 compares the `Also=` names with a template standing for its
    // `DefaultInstance=` (§ 7.3), on both sides.
    let mut as_named = |section: &InstallSection| InstallSection {
        also: section
            .also
            .iter()
            .map(|named| also::also_name(exec, named, read_failures).unwrap_or_else(|t| t))
            .collect(),
        ..section.clone()
    };
    let effective = effective.map(&mut as_named);
    let vendor = match vendor {
        InstallRead::Known(section) => InstallRead::Known(as_named(section)),
        other => other.clone(),
    };
    Some(links::compare_install(effective.as_ref(), &vendor))
}

/// The install files of a unit under `/etc/systemd/system` (§ 10.2 of the
/// units spec): its unit file or shadow there, then its install drop-ins.
fn etc_install_files(fragment: &UnitFragment, evidence: &InstallEvidence) -> Vec<String> {
    let unit_file = match fragment {
        UnitFragment::Local { file } | UnitFragment::Shadowed { file, .. } => {
            Some(format!("{ETC_UNIT_DIR}/{}", file.as_str()))
        }
        UnitFragment::Vendor { .. } | UnitFragment::Absent => None,
    };
    unit_file
        .into_iter()
        .chain(evidence.install_drop_ins.iter().cloned())
        .collect()
}

/// The warning for an entry of a dependency directory under
/// `/etc/systemd/system` whose parent or own name does not parse as a unit
/// name (§ 7.3 of the units spec): it is reported and nothing else.
fn unparsed_link_warning(path: &str) -> Warning {
    Warning {
        inspector: "units".into(),
        message: format!(
            "link {path} - its parent or its own name is not a unit name this scan reads, \
             so the link is not reported or carried"
        ),
        severity: Some(WarningSeverity::Warning),
        extra: std::collections::HashMap::from([("path".into(), serde_json::json!(path))]),
    }
}

/// Why the scan does not report the links under `/etc/systemd/system` to a
/// unit (§ 7.3 of the units spec), for one that no `ManualLink`, install
/// state or other warning accounts for. Each cause is one warning listing
/// every such link ([`unreported_link_warnings`]). Warnings only: nothing
/// here is a read failure, a row or a command.
#[derive(Debug, Clone, PartialEq, Eq)]
enum UnreportedLinks {
    /// The listing does not print the unit as one the scan reads, so it is
    /// not read. `listed` is the listing's state word when it prints the
    /// name at all.
    NotRead { listed: Option<String> },
    /// The unit is masked: its links have no effect.
    Masked,
    /// The unit is a template, not masked, linked under a plain parent that
    /// loads from a unit file of its own name: systemd fills in the
    /// parent's name without its type as the instance and adds `starts`, a
    /// dependency of the parent (`resolve_template` in `src/core/unit.c`,
    /// 252 and 257; measured on RHEL 9 and 10, 2026-11-16). Only
    /// [`Self::for_link`] gives this cause.
    Template { starts: UnitName },
    /// The unit is a template, not masked, under any other parent: an
    /// instance or a template, whose instance name systemd fills in, or a
    /// plain parent that is an alias (systemd fills in the name of the unit
    /// at its end), is masked, has no unit file or is refused, where the
    /// parent's own name does not tell what the link starts. The scan does
    /// not read such a link, and the warning says nothing of its effect.
    /// [`unexamined_links`] gives every such template this cause, and
    /// [`Self::for_link`] makes a link [`Self::Template`] where the
    /// instance is known, and an alias [`Self::TemplateAlias`].
    TemplateFilled,
    /// The unit is a template, not masked, and the link is an alias of it:
    /// a name directly under `/etc/systemd/system` that links to the
    /// template's file, with no parent whose instance systemd fills in. The
    /// scan does not read such a link, and the warning says nothing of its
    /// effect. Only [`Self::for_link`] gives this cause.
    TemplateAlias,
    /// A file at `path`, on the search path outside the two unit
    /// directories the scan reads, changes the unit (§ 7.1).
    Refused { path: String },
    /// The link's name is an alias of `of`; a unit's links are read under
    /// its own name only.
    Alias { of: UnitName },
    /// No unit file of the name is in either unit directory.
    Dangling,
}

impl UnreportedLinks {
    /// The cause's name in the warning's `cause` field, and its place in
    /// the order the warnings are given.
    fn cause(&self) -> (u8, &'static str) {
        match self {
            Self::NotRead { .. } => (0, "not_read"),
            Self::Masked => (1, "masked"),
            Self::Template { .. } => (2, "template"),
            Self::TemplateFilled => (3, "instance_filled"),
            Self::TemplateAlias => (4, "template_alias"),
            Self::Refused { .. } => (5, "refused"),
            Self::Alias { .. } => (6, "alias"),
            Self::Dangling => (7, "dangling"),
        }
    }

    /// Why the links are not reported, written about one link.
    fn reason(&self) -> &'static str {
        match self {
            Self::NotRead { .. } => {
                "systemctl list-unit-files does not list its unit as one this scan reads, so \
                 the scan does not read the unit or its links"
            }
            Self::Masked => "its unit is masked, so the link has no effect",
            Self::Template { .. } => {
                "its unit is a template, and systemd fills in the parent's name without its type \
                 as the instance and starts that instance with the parent"
            }
            Self::TemplateFilled => {
                "its unit is a template, and the scan does not read links that systemd fills in \
                 with an instance name"
            }
            Self::TemplateAlias => {
                "its unit is a template and the link is an alias of it, which the scan does not \
                 read"
            }
            Self::Refused { .. } => {
                "a file on the systemd search path outside the two unit directories this scan \
                 reads changes its unit, so the scan does not read the unit's links"
            }
            Self::Alias { .. } => {
                "it names its unit by an alias, and the scan reads a unit's links under the \
                 unit's own name only"
            }
            Self::Dangling => {
                "there is no unit file of its name under /etc/systemd/system or \
                 /usr/lib/systemd/system, so the link points at nothing"
            }
        }
    }

    /// What the warning adds after a link's unit, in its text and as a
    /// field of the link: the listing's word, the instance the link starts,
    /// the refusing file or the aliased unit.
    fn detail(&self) -> Option<(&'static str, String, String)> {
        match self {
            Self::NotRead {
                listed: Some(state),
            } => Some(("listed_state", format!("listed '{state}'"), state.clone())),
            Self::Template { starts } => Some((
                "starts",
                format!("starts {}", starts.as_str()),
                starts.as_str().to_owned(),
            )),
            Self::Refused { path } => {
                Some(("search_path", format!("changed by {path}"), path.clone()))
            }
            Self::Alias { of } => Some((
                "alias_of",
                format!("an alias of {}", of.as_str()),
                of.as_str().to_owned(),
            )),
            Self::NotRead { listed: None }
            | Self::Masked
            | Self::TemplateFilled
            | Self::TemplateAlias
            | Self::Dangling => None,
        }
    }

    /// The cause for one `link` to `unit`: a template's alias is
    /// [`Self::TemplateAlias`], whose `parent` is the alias's own name and
    /// no parent systemd fills an instance in from; a template's link
    /// under a plain parent that loads from a unit file of its own name is
    /// [`Self::Template`], naming the instance systemd starts
    /// ([`started_instance`]). Every other cause is the unit's.
    fn for_link(&self, exec: &dyn Executor, unit: &UnitName, link: &links::Link) -> Cow<'_, Self> {
        match self {
            Self::TemplateFilled
                if link.relation == inspectah_core::types::units::LinkRelation::Alias =>
            {
                Cow::Owned(Self::TemplateAlias)
            }
            Self::TemplateFilled => match started_instance(exec, unit, &link.parent) {
                Some(starts) => Cow::Owned(Self::Template { starts }),
                None => Cow::Borrowed(self),
            },
            other => Cow::Borrowed(other),
        }
    }
}

/// The instance systemd starts from a link to the template `template`
/// under the plain `parent`, when the parent's name tells it:
/// `<template prefix>@<parent prefix>.<type>`. systemd's `resolve_template`
/// (`src/core/unit.c`, 252 and 257) fills a template dependency in with the
/// parent unit's own instance or, for a plain unit, the prefix of the
/// unit's id (`unit_name_to_prefix`, `src/basic/unit-name.c`). The id is
/// the parent's name only when that name loads from a unit file of its own
/// ([`Resolved::File`]): an alias's id is the unit at its end, and a
/// masked parent, one with no unit file and one the search path refuses
/// start nothing the scan can name. `None` there, for an instance or a
/// template parent, and when the filled-in name is not a unit name (over
/// systemd's length limit, which systemd refuses as well).
fn started_instance(
    exec: &dyn Executor,
    template: &UnitName,
    parent: &UnitName,
) -> Option<UnitName> {
    if parent.is_template() || parent.instance().is_some() {
        return None;
    }
    if !matches!(resolve::resolve(exec, parent), Ok(Resolved::File { .. })) {
        return None;
    }
    let (prefix, type_suffix) = template.as_str().split_once('@')?;
    let (parent_prefix, _) = parent.as_str().rsplit_once('.')?;
    UnitName::try_from(format!("{prefix}@{parent_prefix}{type_suffix}")).ok()
}

/// How many links a grouped unreported-link warning names in its text; the
/// rest are counted, and `links` holds them all.
const UNREPORTED_LINKS_SHOWN: usize = 5;

/// One warning per cause for the links under `/etc/systemd/system` the
/// scan does not report (§ 7.3 of the units spec), in [`UnreportedLinks`]
/// order, each naming the first [`UNREPORTED_LINKS_SHOWN`] links' paths and
/// units and counting the rest, with `cause` and `links` (each `path`,
/// `unit` and the cause's detail, every link) among its fields. A unit
/// with no such link gives nothing.
fn unreported_link_warnings(
    exec: &dyn Executor,
    host_links: &HostLinks,
    unreported: &[(UnitName, UnreportedLinks)],
) -> Vec<Warning> {
    // Per cause (its order, name and reason): each link's text and fields.
    type Listed = (Vec<String>, Vec<serde_json::Value>);
    let mut by_cause: BTreeMap<(u8, &str, &str), Listed> = BTreeMap::new();
    for (unit, why) in unreported {
        for (link, path) in host_links.admin_paths(unit) {
            let why = why.for_link(exec, unit, link);
            let (order, cause) = why.cause();
            let (texts, fields) = by_cause.entry((order, cause, why.reason())).or_default();
            let mut field = serde_json::json!({ "path": path, "unit": unit.as_str() });
            let text = match why.detail() {
                Some((key, text, value)) => {
                    field[key] = serde_json::json!(value);
                    format!("{path} ({}, {text})", unit.as_str())
                }
                None => format!("{path} ({})", unit.as_str()),
            };
            texts.push(text);
            fields.push(field);
        }
    }
    by_cause
        .into_iter()
        .map(|((_, cause, reason), (texts, fields))| {
            let message = match texts.as_slice() {
                [one] => format!("link {one} is not reported or carried: {reason}"),
                many => {
                    let shown = &many[..many.len().min(UNREPORTED_LINKS_SHOWN)];
                    let rest = many.len() - shown.len();
                    let more = if rest > 0 {
                        format!(" and {rest} more")
                    } else {
                        String::new()
                    };
                    format!(
                        "{} links are not reported or carried: for each, {reason}. The links: \
                         {}{more}",
                        many.len(),
                        shown.join("; ")
                    )
                }
            };
            Warning {
                inspector: "units".into(),
                message,
                severity: Some(WarningSeverity::Warning),
                extra: std::collections::HashMap::from([
                    ("cause".into(), serde_json::json!(cause)),
                    ("links".into(), serde_json::json!(fields)),
                ]),
            }
        })
        .collect()
}

/// Why the links under `/etc/systemd/system` to each unit that never
/// reached [`build_state_changes`] (not in `examined`) are not reported,
/// from what loads for its name ([`resolve::resolve`]). A unit the name
/// gate already warned about (`listing` gives its state word) has that
/// warning and is skipped. A template that is not masked is
/// [`UnreportedLinks::TemplateFilled`] whatever loads for it, read again
/// per link by [`UnreportedLinks::for_link`]. A name whose
/// resolution fails, or that is
/// `linked`, is [`UnreportedLinks::NotRead`]: the scan does not read it
/// either way, and a failure here only hides the more specific cause, so it
/// does not degrade the scan.
fn unexamined_links(
    exec: &dyn Executor,
    host_links: &HostLinks,
    examined: &HashSet<UnitName>,
    listing: &BTreeMap<UnitName, String>,
) -> Vec<(UnitName, UnreportedLinks)> {
    let mut unreported = Vec::new();
    for unit in host_links
        .admin
        .keys()
        .filter(|unit| !examined.contains(*unit))
    {
        let listed = listing.get(unit);
        if listed.is_some_and(|state| {
            matches!(
                parse_unit_state(unit.as_str(), state),
                ParsedUnitState::Warn(_)
            )
        }) {
            continue;
        }
        let why = match resolve::resolve(exec, unit) {
            Ok(Resolved::Masked { .. }) => UnreportedLinks::Masked,
            _ if unit.is_template() => UnreportedLinks::TemplateFilled,
            Ok(Resolved::Unsupported { path }) => UnreportedLinks::Refused { path },
            Ok(Resolved::AliasOf(of)) => UnreportedLinks::Alias { of },
            Ok(Resolved::NotFound) => UnreportedLinks::Dangling,
            Ok(Resolved::File { .. }) | Err(_) => UnreportedLinks::NotRead {
                listed: listed.cloned(),
            },
        };
        unreported.push((unit.clone(), why));
    }
    unreported
}

/// The unit file a fragment names, if any.
fn fragment_file(fragment: &UnitFragment) -> Option<&UnitName> {
    match fragment {
        UnitFragment::Local { file }
        | UnitFragment::Shadowed { file, .. }
        | UnitFragment::Vendor { file, .. } => Some(file),
        UnitFragment::Absent => None,
    }
}

/// The warning for one preset line whose verb this scan does not read and
/// that decided `units` (§ 7.4 of the units spec): it names the line, its
/// file and the units, and says their state was not compared with a
/// preset.
fn unknown_preset_warning(file: &str, line: usize, units: &[UnitName]) -> Warning {
    let names: Vec<&str> = units.iter().map(UnitName::as_str).collect();
    Warning {
        inspector: "units".into(),
        message: format!(
            "preset line {line} of {file} starts with a verb this scan does not read and is \
             the first rule matching {}; their state was not compared with a preset",
            names.join(", ")
        ),
        severity: Some(WarningSeverity::Warning),
        extra: std::collections::HashMap::from([
            ("file".into(), serde_json::json!(file)),
            ("line".into(), serde_json::json!(line)),
            ("units".into(), serde_json::json!(names)),
        ]),
    }
}

/// What a state change records about the files `enable` reads `[Install]`
/// from (§ 7.2 of the units spec, step 4 and the install evidence).
#[derive(Debug, Default)]
struct InstallEvidence {
    /// The effective section; `None` only for a mask, which reads none. A
    /// unit file or drop-in that applies and cannot be read as text makes
    /// it `Undetermined(Unreadable)` with `Also=` taken as assigned.
    read: Option<InstallRead>,
    /// The unit file and the drop-ins that apply that could not be read as
    /// text, for the warning.
    unreadable: Vec<String>,
    /// The section the package alone provides (§ 7.2, "The vendor-side
    /// read"), for a `Vendor` or `Shadowed` unit with an install file under
    /// `/etc`; `None` otherwise, or when a file it reads is not text.
    vendor_read: Option<InstallRead>,
    /// Paths of the drop-ins under `/etc/systemd/system` that are install
    /// files, sorted.
    install_drop_ins: Vec<String>,
    /// Paths of the drop-ins in a directory ranked above `/etc` that hide
    /// one of `install_drop_ins`, sorted: the image reads the hidden file
    /// whenever the hiding one is not there, so the row's command needs it
    /// staged too (§ 10.2 rule 5). Kept apart from `install_drop_ins`
    /// because neither is under `/etc/systemd/system`'s own tree, which the
    /// § 7.4 comparison and the findings' install files are about.
    hiding_drop_ins: Vec<String>,
    /// File names of the vendor drop-ins that apply, sorted.
    vendor_drop_ins: Vec<String>,
    /// The unit file's path and text, for an instance whose unit file is
    /// text: its needs are read from it and from its runtime drop-ins
    /// (§ 7.5, `instance::instance_needs`), and a warning names the path.
    /// `None` for any other unit.
    fragment: Option<(String, String)>,
}

impl InstallEvidence {
    /// The row's `install_drop_ins`: the `/etc` install drop-ins and the
    /// drop-ins ranked above `/etc` that hide one of them, sorted, so rule
    /// 5 checks that the export stages every one.
    fn row_install_drop_ins(&self) -> Vec<String> {
        let mut paths: Vec<String> = self
            .install_drop_ins
            .iter()
            .chain(&self.hiding_drop_ins)
            .cloned()
            .collect();
        paths.sort();
        paths
    }
}

/// Read the install evidence of `name`, whose unit file is `fragment`:
/// the `.conf` files in each of [`drop_in_dirs`] under both unit
/// directories and the directories ranked above `/etc`
/// ([`resolve::above_etc_dirs`], which only hide a drop-in of their file
/// name), the effective section over them, the vendor drop-ins' file
/// names, and which `/etc` drop-ins are install files. An `/etc` drop-in is
/// one when it holds an `[Install]` section, or a vendor drop-in of its
/// file name exists in one of those directories (§ 10.2, "Install files").
/// One that cannot be read as text counts, since its `[Install]` cannot be
/// ruled out. A drop-in ranked above `/etc` that hides one of them is
/// recorded in `hiding_drop_ins`. When the unit file or a drop-in that applies cannot be read
/// as text, the section is `Undetermined(Unreadable)` with `Also=` taken as
/// assigned, and the state is read from the links alone. An `Err` names a
/// file or directory that could not be read.
fn install_evidence(
    exec: &dyn Executor,
    name: &UnitName,
    fragment: &UnitFragment,
) -> Result<InstallEvidence, String> {
    let failure = |e: ResolveError| match e {
        ResolveError::Read { path, error } => format!("{path}: {error}"),
        ResolveError::Linked { path, target } => format!("{path}: link to {target}"),
    };
    let dirs = drop_in_dirs(name);
    let mut vendor_names = BTreeSet::new();
    for dir in &dirs {
        vendor_names.extend(
            resolve::conf_files(exec, &format!("{VENDOR_UNIT_DIR}/{dir}")).map_err(failure)?,
        );
    }

    let mut drop_ins = Vec::new();
    let mut unreadable = Vec::new();
    let mut install_drop_ins = Vec::new();
    // Per file name, the drop-in ranked above `/etc` systemd reads, with
    // the rank of its directory in `dirs`: the lowest rank wins, and of
    // equal ranks the first in search-path order, the order `roots` gives.
    let mut above_etc: BTreeMap<String, (usize, String)> = BTreeMap::new();
    // The `/etc` install drop-ins by directory rank and file name.
    let mut etc_install = Vec::new();
    let roots = resolve::above_etc_dirs()
        .map(|base| (DropInRoot::AboveEtc, base))
        .chain([
            (DropInRoot::Etc, ETC_UNIT_DIR),
            (DropInRoot::Vendor, VENDOR_UNIT_DIR),
        ]);
    for (root, base) in roots {
        for (rank, dir) in dirs.iter().enumerate() {
            for file_name in resolve::conf_files(exec, &format!("{base}/{dir}")).map_err(failure)? {
                let path = format!("{base}/{dir}/{file_name}");
                let text = unit_text(exec, &path)?;
                if root == DropInRoot::Etc
                    && (vendor_names.contains(&file_name)
                        || text.as_deref().is_none_or(holds_install_section))
                {
                    install_drop_ins.push(path.clone());
                    etc_install.push((rank, file_name.clone()));
                }
                if root == DropInRoot::AboveEtc {
                    above_etc
                        .entry(file_name.clone())
                        .and_modify(|read| {
                            if rank < read.0 {
                                *read = (rank, path.clone());
                            }
                        })
                        .or_insert_with(|| (rank, path.clone()));
                }
                match text {
                    Some(text) => drop_ins.push((
                        DropInSource {
                            root,
                            dir: dir.clone(),
                            file_name,
                        },
                        text,
                    )),
                    None => unreadable.push(path),
                }
            }
        }
    }

    let fragment_path = match fragment {
        UnitFragment::Local { file } | UnitFragment::Shadowed { file, .. } => {
            Some(format!("{ETC_UNIT_DIR}/{}", file.as_str()))
        }
        UnitFragment::Vendor { file, .. } => Some(format!("{VENDOR_UNIT_DIR}/{}", file.as_str())),
        UnitFragment::Absent => None,
    };
    let fragment_text = match &fragment_path {
        Some(path) => {
            let text = unit_text(exec, path)?;
            if text.is_none() {
                unreadable.insert(0, path.clone());
            }
            text
        }
        None => None,
    };
    let drop_ins: Vec<(DropInSource, &str)> = drop_ins
        .iter()
        .map(|(source, text)| (source.clone(), text.as_str()))
        .collect();
    // Only an instance's needs are read from its unit file here, with its
    // runtime drop-ins, which `instance_needs` reads itself (§ 7.5).
    let instance_fragment = name
        .instance()
        .and(fragment_path.clone().zip(fragment_text.clone()));
    // The vendor-side read (§ 7.2): for a packaged unit file with an
    // install file under `/etc`, a shadow or an install drop-in.
    let vendor_text = match fragment {
        _ if !unreadable.is_empty() => None,
        UnitFragment::Vendor { .. } if !install_drop_ins.is_empty() => fragment_text.clone(),
        UnitFragment::Shadowed { file, .. } => {
            unit_text(exec, &format!("{VENDOR_UNIT_DIR}/{}", file.as_str()))?
        }
        _ => None,
    };
    let vendor_read = vendor_text.map(|text| read_vendor_install(name, &text, &drop_ins));
    let read = match fragment_text {
        _ if !unreadable.is_empty() => Some(InstallRead::Undetermined {
            reason: InstallUnknown::Unreadable,
            assigns_also: true,
        }),
        Some(text) => Some(read_install(name, &text, &drop_ins)),
        None => None,
    };
    install_drop_ins.sort();
    // A drop-in ranked above `/etc` hides an `/etc` drop-in of its file
    // name in a directory of its rank or after it (instance `.d` before
    // template `.d`, `read_install`'s order).
    let hiding_drop_ins: BTreeSet<String> = etc_install
        .iter()
        .filter_map(|(rank, file_name)| {
            above_etc
                .get(file_name)
                .filter(|(above_rank, _)| above_rank <= rank)
                .map(|(_, path)| path.clone())
        })
        .collect();
    Ok(InstallEvidence {
        read,
        unreadable,
        vendor_read,
        install_drop_ins,
        hiding_drop_ins: hiding_drop_ins.into_iter().collect(),
        vendor_drop_ins: vendor_names.into_iter().collect(),
        fragment: instance_fragment,
    })
}

/// The text of the unit file or drop-in at `path`, or `None` when it is
/// not text the executor returns (not UTF-8, or over its size cap).
fn unit_text(exec: &dyn Executor, path: &str) -> Result<Option<String>, String> {
    match exec.read_file(Path::new(path)) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => Ok(None),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

/// The mount findings for the storage files under `/etc/systemd/system`
/// that the candidates did not make a finding of (§ 6.5 of the units
/// spec): every regular, non-empty `.mount`, `.automount` or `.swap` file,
/// local or a full copy over a vendor file. Such a unit is not enabled (an
/// enabled one was a candidate) and not masked (its file is no mask), so
/// its finding records no state. `recorded` holds the names that already
/// have a finding, or were already reported as refused, unreadable or
/// undetermined.
///
/// A listing error is left to [`collect_drop_ins`], which reads the same
/// directory and reports it. A unit file that could not be read goes to
/// `read_failures`.
fn etc_storage_findings(
    exec: &dyn Executor,
    recorded: &HashSet<UnitName>,
    warnings: &mut Vec<Warning>,
    read_failures: &mut Vec<String>,
) -> Vec<MountUnitFinding> {
    let Ok(entries) = exec.read_dir_strict(Path::new(ETC_UNIT_DIR)) else {
        return Vec::new();
    };
    let mut fragments = Vec::new();
    for entry in entries {
        let Ok(name) = UnitName::try_from(entry) else {
            continue;
        };
        if !matches!(name.kind(), UnitKind::Storage(_)) || recorded.contains(&name) {
            continue;
        }
        match resolve::resolve(exec, &name) {
            Ok(Resolved::File {
                fragment: fragment @ (UnitFragment::Local { .. } | UnitFragment::Shadowed { .. }),
            }) => fragments.push((name, fragment)),
            Ok(Resolved::Unsupported { path }) => warnings.push(search_path_warning(
                |detail| etc_unit_file_warning(&name, detail),
                path,
                FINDING_NOT_RECORDED,
            )),
            // A mask, an alias or a link elsewhere: not a unit file here.
            Ok(_) | Err(ResolveError::Linked { .. }) => {}
            Err(ResolveError::Read { path, error }) => {
                read_failures.push(format!("{path}: {error}"));
            }
        }
    }

    let (names, mut fragments): (Vec<UnitName>, Vec<UnitFragment>) = fragments.into_iter().unzip();
    populate_owning_packages(exec, &mut fragments);

    let mut findings = Vec::new();
    for (name, fragment) in names.into_iter().zip(fragments) {
        // The name is the file's and the owner is never empty, so neither
        // check below refuses in practice; a refusal is still reported.
        let warn = |detail: &str| etc_unit_file_warning(&name, detail);
        let unit = UnitSubject::new(name.clone(), fragment)
            .map_err(|e| e.to_string())
            .and_then(|subject| AdvisoryUnit::try_from(subject).map_err(|e| e.to_string()));
        match unit {
            Ok(unit) => findings.push(MountUnitFinding {
                unit,
                state_change: None,
                mount_point: None,
                nofail: None,
                aggregate: None,
            }),
            Err(detail) => warnings.push(warn(&detail)),
        }
    }
    findings
}

/// A structured warning about the unit file `name` under
/// `/etc/systemd/system`.
fn etc_unit_file_warning(name: &UnitName, detail: &str) -> Warning {
    Warning {
        inspector: "units".into(),
        message: format!("unit file {ETC_UNIT_DIR}/{} - {detail}", name.as_str()),
        severity: Some(WarningSeverity::Warning),
        extra: std::collections::HashMap::from([("unit".into(), serde_json::json!(name))]),
    }
}

/// The warning for a unit whose unit file or `[Install]` drop-in
/// `paths` cannot be read as text, built by `warn`, with `paths` among its
/// fields: its section is undetermined and its state is read from its
/// links alone.
fn unreadable_install_warning(warn: impl FnOnce(&str) -> Warning, paths: &[String]) -> Warning {
    let mut warning = warn(&format!(
        "{} cannot be read as text, so its [Install] section is undetermined \
         and its state is read from its links alone",
        paths.join(", ")
    ));
    warning
        .extra
        .insert("paths".into(), serde_json::json!(paths));
    warning
}

/// The warning for an enabled instance whose needs are unknown (§ 7.5),
/// built by `warn`: it names the setting and the file it is in, with its
/// line, or the file that cannot be read, and says the enable is not
/// carried. The key, path and line are among its fields. It never holds a
/// setting's value: a command line can carry a credential, and the
/// redaction pass does not read warnings.
fn needs_unknown_warning(warn: impl FnOnce(&str) -> Warning, unknown: &NeedsUnknown) -> Warning {
    match unknown {
        NeedsUnknown::Uninterpretable { key, path, line } => {
            let mut warning = warn(&format!(
                "its {key}= setting in {path} (line {line}) names a path for the instance that \
                 inspectah cannot read as systemd does, so what the instance reads is unknown \
                 and its enable is not carried"
            ));
            warning.extra.insert("key".into(), serde_json::json!(key));
            warning.extra.insert("path".into(), serde_json::json!(path));
            warning.extra.insert("line".into(), serde_json::json!(line));
            warning
        }
        NeedsUnknown::Unreadable { path, error } => {
            let mut warning = warn(&format!(
                "{path}, a drop-in directory or drop-in systemd reads for the instance, cannot \
                 be read ({error}), so what the instance reads is unknown and its enable is not \
                 carried"
            ));
            warning.extra.insert("path".into(), serde_json::json!(path));
            warning
        }
    }
}

/// The closing clause of the search-path warning from the state-change
/// pass.
const STATE_NOT_CARRIED: &str = "the unit's state is not carried";

/// The closing clause of the search-path warning from the storage pass.
const FINDING_NOT_RECORDED: &str = "its mount finding is not recorded";

/// The warning for a unit the search path refuses (§ 7.1 of the units
/// spec), built by `warn` from the detail ending in `consequence`, with
/// `path` among its fields.
fn search_path_warning(
    warn: impl FnOnce(&str) -> Warning,
    path: String,
    consequence: &str,
) -> Warning {
    let mut warning = warn(&format!(
        "{path} is on the systemd search path, outside the two unit \
         directories this scan reads, and changes what loads or what \
         enable does; {consequence}"
    ));
    warning.extra.insert("path".into(), serde_json::json!(path));
    warning
}

/// Inspects systemd unit state for every unit kind: enabled, disabled or
/// masked against what the image would have, where each unit's file lives
/// and which package owns it. Also collects drop-ins and full shadows of
/// every kind and flags environment variables that may contain secrets.
pub struct UnitsInspector;

impl UnitsInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for UnitsInspector {
    fn default() -> Self {
        Self::new()
    }
}

/// A parsed unit entry from `systemctl list-unit-files` output.
#[derive(Debug)]
struct UnitFileEntry {
    unit: String,
    state: String,
    /// Parsed from systemctl output but not used: preset comparison
    /// uses the actual preset files for first-match-wins semantics.
    _preset: String,
}

/// One line of the host's `.preset` files, in the order systemd reads
/// them (§ 7.1 source 2 of the units spec).
#[derive(Debug, Clone, PartialEq, Eq)]
enum PresetRule {
    /// `enable` or `disable` and one unit name or glob. A template name
    /// here is the bare form.
    Plain {
        verb: PresetDefault,
        pattern: String,
    },
    /// `enable`, a template name and the instances that follow it.
    EnableInstances {
        template: String,
        instances: Vec<String>,
    },
    /// A first word that is neither verb. Kept with where it came from,
    /// for the warning.
    Unknown {
        pattern: String,
        file: String,
        line: usize,
    },
}

impl PresetRule {
    /// Read one preset line: `None` for a line of fewer than two words. An
    /// `enable` line is the list form when its second word is a template
    /// name and more words follow; every other line is read by its first
    /// two words. `file` and `line` (from 1) say where it came from.
    fn parse(text: &str, file: &str, line: usize) -> Option<Self> {
        let mut words = text.split_whitespace();
        let (verb, pattern) = (words.next()?, words.next()?.to_owned());
        let rest: Vec<String> = words.map(str::to_owned).collect();
        let verb = match verb {
            "enable" => PresetDefault::Enable,
            "disable" => PresetDefault::Disable,
            _ => {
                return Some(Self::Unknown {
                    pattern,
                    file: file.to_owned(),
                    line,
                });
            }
        };
        let template = UnitName::try_from(pattern.clone()).is_ok_and(|name| name.is_template());
        if verb == PresetDefault::Enable && template && !rest.is_empty() {
            return Some(Self::EnableInstances {
                template: pattern,
                instances: rest,
            });
        }
        Some(Self::Plain { verb, pattern })
    }

    /// Whether the rule matches the unit `name` by its pattern: a glob for
    /// `Plain` and `Unknown`, the template's own name for the list form.
    fn matches(&self, name: &str) -> bool {
        match self {
            Self::Plain { pattern, .. } | Self::Unknown { pattern, .. } => {
                glob_match(pattern, name)
            }
            Self::EnableInstances { template, .. } => template == name,
        }
    }
}

impl Inspector for UnitsInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Units
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        let exec = ctx.executor;

        // 1. Run systemctl list-unit-files
        let systemctl_result = exec.run("systemctl", &["list-unit-files", "--no-pager"]);

        if systemctl_result.exit_code == 127 {
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::Units(UnitSection::default()),
                    warnings: Vec::new(),
                    redaction_hints: Vec::new(),
                }),
                reason: "systemctl not found".into(),
            });
        }

        if !systemctl_result.success() {
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::Units(UnitSection::default()),
                    warnings: Vec::new(),
                    redaction_hints: Vec::new(),
                }),
                reason: format!(
                    "systemctl failed with exit code {}",
                    systemctl_result.exit_code
                ),
            });
        }

        // 2. Parse systemctl output
        let units = parse_unit_files(&systemctl_result.stdout);
        if units.is_empty() {
            return Ok(InspectorOutput {
                section: SectionData::Units(UnitSection::default()),
                warnings: Vec::new(),
                redaction_hints: Vec::new(),
            });
        }

        // 3. Read preset files: try both dirs; if neither readable, degrade
        let (preset_rules, preset_read_failures) = match read_preset_rules(exec) {
            Ok(pair) => pair,
            Err(reason) => {
                // Degrade: include systemctl data as partial
                let mut warnings = Vec::new();
                let inventory = partial_inventory(&units, &mut warnings);
                return Err(InspectorError::Degraded {
                    partial: Box::new(InspectorOutput {
                        section: SectionData::Units(UnitSection {
                            state_changes: Vec::new(),
                            inventory,
                            drop_ins: Vec::new(),
                            mount_findings: Vec::new(),
                            manual_links: Vec::new(),
                            undetermined_installs: Vec::new(),
                            default_target: None,
                        }),
                        warnings,
                        redaction_hints: Vec::new(),
                    }),
                    reason,
                });
            }
        };

        // 4. The links under both unit directories (§ 7.1 sources 3 and 4),
        //    then every unit past the name gate once, in listing order.
        let (host_links, mut unit_file_read_failures) = links::read_host_links(exec);
        let mut warnings: Vec<Warning> = host_links
            .admin_unparsed
            .iter()
            .map(|path| unparsed_link_warning(path))
            .collect();
        let mut listed = Vec::new();
        let mut seen = HashSet::new();
        // Every name the listing gave, kept by the gate or not.
        let listing: HashSet<UnitName> = units
            .iter()
            .filter_map(|unit| UnitName::try_from(unit.unit.clone()).ok())
            .collect();
        for unit in &units {
            let Some((name, state)) = gate(unit, &host_links, &mut warnings) else {
                continue;
            };
            // A name systemctl lists again keeps its first entry.
            if seen.insert(name.clone()) {
                listed.push(Listed {
                    name,
                    raw_state: unit.state.clone(),
                    listed: state,
                });
            }
        }
        // 4a. The enabled instances the listing has no row for, found by
        //     their links, then the instances the presets would link (§ 7.5).
        listed.extend(linked_instances(&host_links, &listing));
        let mut known = listing.clone();
        known.extend(listed.iter().map(|subject| subject.name.clone()));
        // Every name read below whose links can be under `/etc` (§ 7.3): a
        // derived instance has none.
        let examined: HashSet<UnitName> =
            listed.iter().map(|subject| subject.name.clone()).collect();
        let derived = preset_instances(
            exec,
            &units,
            &preset_rules,
            &known,
            &host_links,
            &mut unit_file_read_failures,
        );
        known.extend(derived.iter().map(|subject| subject.name.clone()));
        listed.extend(derived);

        // 4b. Each unit's file, install state and preset: the inventory,
        //     the state changes and the enabled storage units.
        let mut reported = HashSet::new();
        let UnitFindings {
            inventory,
            state_changes,
            mut mount_findings,
            manual_links,
            undetermined_installs,
            mut unreported,
        } = build_state_changes(
            exec,
            listed,
            &host_links,
            &preset_rules,
            &known,
            &mut warnings,
            &mut unit_file_read_failures,
            &mut reported,
        );
        // 4b'. The links under `/etc` none of that read or warned about:
        //      warnings only (§ 7.3).
        let mut listing_states = BTreeMap::new();
        for unit in &units {
            if let Ok(name) = UnitName::try_from(unit.unit.clone()) {
                listing_states
                    .entry(name)
                    .or_insert_with(|| unit.state.clone());
            }
        }
        unreported.extend(unexamined_links(
            exec,
            &host_links,
            &examined,
            &listing_states,
        ));
        warnings.extend(unreported_link_warnings(exec, &host_links, &unreported));

        // 4c. The administrator's other storage files, then what each
        //     finding mounts, read from its unit file (§ 6.5).
        let mut recorded = reported;
        recorded.extend(
            mount_findings
                .iter()
                .map(|f| f.unit.name().as_unit_name().clone()),
        );
        mount_findings.extend(etc_storage_findings(
            exec,
            &recorded,
            &mut warnings,
            &mut unit_file_read_failures,
        ));
        for finding in &mut mount_findings {
            read_mount_settings(exec, finding, &mut unit_file_read_failures);
        }

        // Emit metric for progress rendering
        progress.emit(inspectah_core::types::progress::ProgressEvent::Metric {
            inspector: InspectorId::Units,
            kind: inspectah_core::types::progress::MetricKind::UnitsFound,
            value: state_changes.len(),
        });

        // 5. Drop-ins and full shadows of every kind: one owner per path
        //    under /etc/systemd/system (§ 7.7 of the units spec).
        let (drop_ins, redaction_hints, dropin_read_failures) = collect_drop_ins(exec);

        // 6. The default target (§ 7.6 of the units spec).
        let default_target = default_target(exec, &mut warnings);

        // 7. Build result
        let section = UnitSection {
            state_changes,
            inventory,
            drop_ins,
            mount_findings,
            manual_links,
            undetermined_installs,
            default_target,
        };

        // Preset file read failures are correctness-bearing: a missing
        // preset file means we cannot determine the default state for
        // services whose rules were in that file.
        if !preset_read_failures.is_empty() {
            let reason = format!(
                "preset file read failures: {}",
                preset_read_failures.join("; ")
            );
            // The rules read are a subset, so every state change and every
            // preset in the inventory may be wrong, either way. As when no
            // preset can be read: no state change, and an inventory without
            // presets. The gate already warned about any unparsed name.
            let section = UnitSection {
                state_changes: Vec::new(),
                inventory: partial_inventory(&units, &mut Vec::new()),
                ..section
            };
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::Units(section),
                    warnings,
                    redaction_hints,
                }),
                reason,
            });
        }

        // A unit file that could not be read hides where the unit came from
        // and who owns it, so its state change is missing.
        if !unit_file_read_failures.is_empty() {
            let reason = format!(
                "unit file read failures: {}",
                unit_file_read_failures.join("; ")
            );
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::Units(section),
                    warnings,
                    redaction_hints,
                }),
                reason,
            });
        }

        // Drop-in read failures are correctness-bearing: a missed override
        // makes the snapshot look like defaults when there are customizations.
        if !dropin_read_failures.is_empty() {
            let reason = format!(
                "drop-in conf read failures (possible size cap): {}",
                dropin_read_failures.join("; ")
            );
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::Units(section),
                    warnings,
                    redaction_hints,
                }),
                reason,
            });
        }

        Ok(InspectorOutput {
            section: SectionData::Units(section),
            warnings,
            redaction_hints,
        })
    }
}

/// The link `systemctl set-default` writes (§ 7.6 of the units spec).
const DEFAULT_TARGET_LINK: &str = "/etc/systemd/system/default.target";

/// The default target the host sets (§ 7.6 of the units spec): the target
/// [`DEFAULT_TARGET_LINK`] links to, named by the link's last path
/// component, with the unit file that loads for it and that file's owner.
/// Recorded as included. `None` when there is no link; a link that names
/// no target, or a target with no unit file this scan reads, gives a
/// warning and no record.
fn default_target(exec: &dyn Executor, warnings: &mut Vec<Warning>) -> Option<DefaultTarget> {
    let link = match exec.read_link(Path::new(DEFAULT_TARGET_LINK)) {
        Ok(link) => link,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warnings.push(default_target_warning(
                None,
                &format!("could not be read as a link ({e})"),
            ));
            return None;
        }
    };
    let file_name = link.rsplit('/').next().unwrap_or(&link);
    let target = UnitName::try_from(file_name.to_owned())
        .map_err(|e| e.to_string())
        .and_then(|name| match resolve::resolve(exec, &name) {
            Ok(Resolved::File { fragment }) => {
                let mut fragments = [fragment];
                populate_owning_packages(exec, &mut fragments);
                let [fragment] = fragments;
                UnitSubject::new(name, fragment)
                    .and_then(TargetUnit::try_from)
                    .map_err(|e| e.to_string())
            }
            _ => Err(format!(
                "{} has no unit file this scan reads",
                name.as_str()
            )),
        });
    match target {
        Ok(target) => Some(DefaultTarget {
            target,
            disposition: FindingKind::included(),
            locked: false,
            aggregate: None,
        }),
        Err(detail) => {
            warnings.push(default_target_warning(Some(&link), &detail));
            None
        }
    }
}

/// The warning for a [`DEFAULT_TARGET_LINK`] that records no default
/// target, with the link's target among its fields when it was read.
fn default_target_warning(link: Option<&str>, detail: &str) -> Warning {
    let subject = match link {
        Some(link) => format!("{DEFAULT_TARGET_LINK} links to {link}"),
        None => DEFAULT_TARGET_LINK.to_owned(),
    };
    let mut extra =
        std::collections::HashMap::from([("path".into(), serde_json::json!(DEFAULT_TARGET_LINK))]);
    if let Some(link) = link {
        extra.insert("target".into(), serde_json::json!(link));
    }
    Warning {
        inspector: "units".into(),
        message: format!("{subject} - {detail}; the default target is not carried"),
        severity: Some(WarningSeverity::Warning),
        extra,
    }
}

/// Parse `systemctl list-unit-files` output.
/// Expects header line, data lines with UNIT/STATE/PRESET columns,
/// and a trailing summary line ("N unit files listed.").
fn parse_unit_files(stdout: &str) -> Vec<UnitFileEntry> {
    let mut entries = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        // Skip header, empty lines, and summary line
        if line.is_empty() || line.starts_with("UNIT FILE") || line.ends_with("unit files listed.")
        {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            entries.push(UnitFileEntry {
                unit: parts[0].to_string(),
                state: parts[1].to_string(),
                _preset: parts[2].to_string(),
            });
        } else if parts.len() == 2 {
            // Some outputs may omit preset column
            entries.push(UnitFileEntry {
                unit: parts[0].to_string(),
                state: parts[1].to_string(),
                _preset: String::new(),
            });
        }
    }
    entries
}

/// Read and merge preset rules from standard preset directories.
/// Returns `(rules, read_failures)`. Returns Err if neither directory is readable.
/// Individual file-level read failures are tracked but do not prevent other
/// files from being read: the caller decides whether failures are fatal.
fn read_preset_rules(exec: &dyn Executor) -> Result<(Vec<PresetRule>, Vec<String>), String> {
    let usr_dir = Path::new("/usr/lib/systemd/system-preset");
    let etc_dir = Path::new("/etc/systemd/system-preset");

    let mut preset_files: Vec<(String, String, String)> = Vec::new(); // (filename, path, content)
    let mut read_failures: Vec<String> = Vec::new();

    // Strict listings: a listing that fails part way may leave out the file
    // holding a unit's rule, and a state change derived from the rest
    // could point either way.
    let usr_ok = match exec.read_dir_strict(usr_dir) {
        Ok(entries) => {
            for entry in &entries {
                if entry.ends_with(".preset") {
                    let path = usr_dir.join(entry);
                    match exec.read_file(&path) {
                        Ok(content) => preset_files.push((
                            entry.clone(),
                            path.to_string_lossy().into_owned(),
                            content,
                        )),
                        Err(e) => {
                            read_failures.push(format!("{}: {e}", path.to_string_lossy()));
                        }
                    }
                }
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Directory doesn't exist: normal on systems without vendor presets.
            false
        }
        Err(e) => {
            // Directory exists but is unreadable (PermissionDenied, etc.):
            // this is a trust gap: vendor presets may be hidden.
            read_failures.push(format!("{}: {e}", usr_dir.display()));
            false
        }
    };

    let etc_ok = match exec.read_dir_strict(etc_dir) {
        Ok(entries) => {
            for entry in &entries {
                if entry.ends_with(".preset") {
                    let path = etc_dir.join(entry);
                    match exec.read_file(&path) {
                        Ok(content) => preset_files.push((
                            entry.clone(),
                            path.to_string_lossy().into_owned(),
                            content,
                        )),
                        Err(e) => {
                            read_failures.push(format!("{}: {e}", path.to_string_lossy()));
                        }
                    }
                }
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Directory doesn't exist: normal on systems without admin presets.
            false
        }
        Err(e) => {
            // Directory exists but is unreadable (PermissionDenied, etc.):
            // admin preset overrides may be hidden.
            read_failures.push(format!("{}: {e}", etc_dir.display()));
            false
        }
    };

    if !usr_ok && !etc_ok && read_failures.is_empty() {
        return Err(
            "neither /usr/lib/systemd/system-preset nor /etc/systemd/system-preset readable".into(),
        );
    }

    // Sort by filename: numeric prefix ordering (e.g., 85-xxx before 90-xxx)
    preset_files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut rules = Vec::new();
    for (_filename, path, content) in &preset_files {
        for (index, line) in content.lines().enumerate() {
            let line = line.trim();
            // `#` and `;` both start a comment line; the second is read from
            // systemd's preset reader, not measured on RHEL 9 or 10.
            if line.is_empty() || line.starts_with(['#', ';']) {
                continue;
            }
            rules.extend(PresetRule::parse(line, path, index + 1));
        }
    }

    Ok((rules, read_failures))
}

/// Simple glob matching supporting `*` (any chars) and `?` (single char).
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    glob_match_inner(&pattern, &text)
}

fn glob_match_inner(pattern: &[char], text: &[char]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some('*'), _) => {
            // '*' matches zero or more characters
            // Try matching rest of pattern against current text position
            // or skip one character of text
            glob_match_inner(&pattern[1..], text)
                || (!text.is_empty() && glob_match_inner(pattern, &text[1..]))
        }
        (Some('?'), Some(_)) => glob_match_inner(&pattern[1..], &text[1..]),
        (Some(p), Some(t)) if *p == *t => glob_match_inner(&pattern[1..], &text[1..]),
        _ => false,
    }
}

/// The inventory of enabled, disabled and masked units when no preset rule
/// could be read: every entry has no preset, and its state is the
/// listing's word, since the degraded scan reads no links. A name that
/// does not parse is reported through `warnings` and has no entry.
fn partial_inventory(
    units: &[UnitFileEntry],
    warnings: &mut Vec<Warning>,
) -> Vec<UnitInventoryEntry> {
    let mut inventory = Vec::new();
    let mut inventoried = HashSet::new();
    let no_links = HostLinks::default();
    for u in units {
        let Some((name, Some(state))) = gate(u, &no_links, warnings) else {
            continue;
        };
        if inventoried.insert(name.clone()) {
            inventory.push(UnitInventoryEntry {
                unit: name,
                state,
                preset: None,
            });
        }
    }
    inventory
}

/// Detect whether a drop-in's unit has a full shadow or a drop-in override.
///
/// The parent path's form is read with [`resolve::entry`], the test the
/// resolver and [`full_shadow`] apply, so the three agree:
///
/// - FullShadow: `/etc/systemd/system/{unit}` is a regular, non-empty file
///   and `/usr/lib/systemd/system/{unit}` exists;
/// - None: `/etc/systemd/system/{unit}` is a regular, non-empty file and
///   no vendor file exists, a local unit;
/// - DropIn: anything else at `/etc/systemd/system/{unit}` (nothing, a
///   zero-length file or a link, which includes both forms of a mask) and
///   `/etc/systemd/system/{unit}.d/` exists. A drop-in beside a mask is an
///   ordinary drop-in: § 7.7 of the units spec gives it no other label.
///
/// An `Err` names a path whose form could not be read.
fn detect_shadow_type(exec: &dyn Executor, unit_name: &str) -> Result<Option<ShadowType>, String> {
    let read_failure = |e: ResolveError| match e {
        ResolveError::Read { path, error } => format!("{path}: {error}"),
        ResolveError::Linked { path, target } => format!("{path}: link to {target}"),
    };
    let etc_path = format!("{ETC_UNIT_DIR}/{unit_name}");
    let usr_path = format!("{VENDOR_UNIT_DIR}/{unit_name}");
    let dropin_dir = format!("{ETC_UNIT_DIR}/{unit_name}.d");

    let replaced = matches!(
        resolve::entry(exec, &etc_path).map_err(read_failure)?,
        Entry::File { empty: false }
    );
    if replaced {
        let vendor = resolve::entry(exec, &usr_path).map_err(read_failure)?;
        return Ok((!matches!(vendor, Entry::Missing)).then_some(ShadowType::FullShadow));
    }
    let dropin_exists = exec.run("test", &["-d", &dropin_dir]).exit_code == 0;
    Ok(dropin_exists.then_some(ShadowType::DropIn))
}

/// Collect what the units section owns under `/etc/systemd/system` (§ 7.7
/// of the units spec): every `.conf` in a unit's drop-in directory, then
/// every full shadow ([`full_shadow`]).
///
/// A drop-in directory is collected for any name, whether or not it parses
/// as a [`UnitName`]: a `.device` or `.scope` unit, a generated unit, or a
/// name with a typo. The row keeps the name as text in `unit`. A type-wide
/// directory (`service.d`) belongs to no unit: its files stay config
/// entries.
///
/// Returns (drop_ins, redaction_hints, read_failures).
/// Read failures on listed `.conf` files are correctness-bearing: the
/// snapshot would look complete when an override was actually missed.
fn collect_drop_ins(exec: &dyn Executor) -> (Vec<SystemdDropIn>, Vec<RedactionHint>, Vec<String>) {
    let dropin_base = Path::new(ETC_UNIT_DIR);
    let mut drop_ins = Vec::new();
    let mut hints = Vec::new();
    let mut read_failures = Vec::new();

    // Strict listings: a partial one would leave a drop-in out of the
    // export without a word, and the link reader leaves a failed listing
    // of this directory to this pass to report.
    let entries = match exec.read_dir_strict(dropin_base) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Directory doesn't exist: no drop-ins to collect.
            return (drop_ins, hints, read_failures);
        }
        Err(e) => {
            // Directory exists but is unreadable (PermissionDenied, etc.):
            // all drop-in configurations are hidden.
            read_failures.push(format!("{}: {e}", dropin_base.display()));
            return (drop_ins, hints, read_failures);
        }
    };

    for entry in &entries {
        // "httpd.service.d" → "httpd.service"
        let Some(unit) = entry.strip_suffix(".d") else {
            continue;
        };
        if UNIT_TYPE_SUFFIXES.contains(&unit) {
            continue;
        }
        let dir_path = dropin_base.join(entry);

        let conf_files = match exec.read_dir_strict(&dir_path) {
            Ok(c) => c,
            // A regular file whose name ends in `.d` is no drop-in
            // directory; the config walk records it.
            Err(e) if e.kind() == std::io::ErrorKind::NotADirectory => continue,
            Err(e) => {
                read_failures.push(format!("{}: {e}", dir_path.to_string_lossy()));
                continue;
            }
        };

        for conf in &conf_files {
            if !conf.ends_with(".conf") {
                continue;
            }
            let file_path = dir_path.join(conf);
            let content = match exec.read_file(&file_path) {
                Ok(c) => c,
                // Not text, or over the executor's size cap: no row, as for
                // a full shadow (see [`full_shadow`]). Its config entry
                // records that the content was not captured.
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => continue,
                Err(e) => {
                    read_failures.push(format!("{}: {e}", file_path.to_string_lossy()));
                    continue;
                }
            };

            let path_str = file_path.to_string_lossy().to_string();

            // Check for secret-like environment variables
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(env_val) = trimmed.strip_prefix("Environment=") {
                    // Check if the variable name contains secret patterns
                    // Format: NAME=VALUE or just NAME
                    let var_name = env_val.split('=').next().unwrap_or("");
                    let upper = var_name.to_uppercase();
                    if SECRET_PATTERNS.iter().any(|p| upper.contains(p)) {
                        hints.push(RedactionHint {
                            path: path_str.clone(),
                            reason: format!(
                                "environment variable '{}' may contain a secret",
                                var_name
                            ),
                            confidence: Some(Confidence::Medium),
                        });
                    }
                }
            }

            // Detect shadow type and set rationale for full shadows
            let shadow_type = match detect_shadow_type(exec, unit) {
                Ok(shadow_type) => shadow_type,
                Err(failure) => {
                    read_failures.push(failure);
                    None
                }
            };
            let shadow_rationale = if shadow_type == Some(ShadowType::FullShadow) {
                Some(
                    "Full unit shadow: base image updates to this unit will be silently ignored"
                        .to_string(),
                )
            } else {
                None
            };

            drop_ins.push(SystemdDropIn {
                unit: unit.to_string(),
                path: path_str,
                content,
                disposition: FindingKind::included(),
                shadow_type,
                shadow_rationale,
                ..Default::default()
            });
        }
    }

    for entry in &entries {
        match full_shadow(exec, entry) {
            Ok(Some(shadow)) => drop_ins.push(shadow),
            Ok(None) => {}
            Err(failure) => read_failures.push(failure),
        }
    }

    (drop_ins, hints, read_failures)
}

/// The full shadow row for `entry`, a name directly under
/// `/etc/systemd/system`: a regular, non-empty file of an ordinary kind
/// with a vendor file of the same name (§ 7.7 of the units spec). Any
/// other path is `None`, because another section, or a state, owns it:
///
/// - a name that is not a unit, such as a `.wants` directory;
/// - a storage kind: its file is a config entry and the unit a mount
///   finding;
/// - a link (a mask or an alias) or a zero-length file (a mask): the unit's
///   state change records it, and the image gets it from `systemctl`;
/// - no vendor file: a local unit, whose file is a config entry;
/// - a file that is not text or is over the executor's size cap: its
///   config entry records that the content was not captured. A drop-in
///   row would stage an empty file, which systemd reads as a mask.
///
/// The form test is the one [`resolve::resolve`] applies, so a full shadow
/// row and a `Shadowed` fragment agree. An `Err` names a file that could
/// not be read.
fn full_shadow(exec: &dyn Executor, entry: &str) -> Result<Option<SystemdDropIn>, String> {
    let Ok(name) = UnitName::try_from(entry.to_owned()) else {
        return Ok(None);
    };
    match name.kind() {
        UnitKind::Storage(_) => return Ok(None),
        UnitKind::Ordinary(_) => {}
    }
    let read_failure = |e: ResolveError| match e {
        ResolveError::Read { path, error } => format!("{path}: {error}"),
        ResolveError::Linked { path, target } => format!("{path}: link to {target}"),
    };
    // The `/etc` form first: a link there (an alias or a mask) is never a
    // shadow and costs no vendor read.
    let path = format!("{ETC_UNIT_DIR}/{entry}");
    if !matches!(
        resolve::entry(exec, &path).map_err(read_failure)?,
        Entry::File { empty: false }
    ) {
        return Ok(None);
    }
    let vendor = format!("{VENDOR_UNIT_DIR}/{entry}");
    if matches!(
        resolve::entry(exec, &vendor).map_err(read_failure)?,
        Entry::Missing
    ) {
        return Ok(None);
    }
    let content = match exec.read_file(Path::new(&path)) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => return Ok(None),
        Err(e) => return Err(format!("{path}: {e}")),
    };
    Ok(Some(SystemdDropIn {
        unit: entry.to_owned(),
        path,
        content,
        disposition: FindingKind::included(),
        shadow_type: Some(ShadowType::FullShadow),
        shadow_rationale: Some(FULL_SHADOW_RATIONALE.to_string()),
        ..Default::default()
    }))
}

/// What a mount finding records from its unit file (§ 6.5 of the units
/// spec): the mount point and whether the unit is `nofail`.
#[derive(Debug, PartialEq, Eq)]
struct MountSettings {
    /// `Where=`, when it is an absolute path. A swap unit has none.
    mount_point: Option<String>,
    /// Whether `Options=` holds `nofail` with no `fail` after it. An
    /// automount has no `Options=`.
    nofail: bool,
}

/// The section a storage unit's own settings are in.
fn storage_section(kind: StorageKind) -> &'static str {
    match kind {
        StorageKind::Mount => "Mount",
        StorageKind::Automount => "Automount",
        StorageKind::Swap => "Swap",
    }
}

/// Read `Where=` and `Options=` from a storage unit file as systemd reads
/// them: by logical line ([`logical_lines`]), so the continuation of one
/// setting is never read as a key or a section header of its own; only in
/// the section of the unit's own kind; the last assignment winning (an
/// empty one resets); comment lines skipped; and the key and value
/// stripped of the whitespace systemd strips. `Where=` belongs to mounts
/// and automounts, `Options=` to mounts and swaps. `What=` is never read:
/// it can hold a credential, and the finding has no field for it. `nofail`
/// is decided as systemd decides it, by whichever of `nofail` and `fail`
/// comes last among the options [`mount_options`] splits `Options=` into.
///
/// Only the unit file itself is read. Drop-ins
/// (`<name>.mount.d/*.conf`) that set `Where=` or `Options=` are not read,
/// so they can leave `mount_point` and `nofail` short of what systemd uses.
fn mount_settings(kind: StorageKind, content: &str) -> MountSettings {
    let own_section = storage_section(kind);
    let mut section = None;
    let mut where_value = None;
    let mut options = None;
    let lines = logical_lines(content);
    for line in &lines {
        let line = line.trim_matches(UNIT_FILE_WHITESPACE);
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = Some(name);
            continue;
        }
        if section != Some(own_section) {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim_matches(UNIT_FILE_WHITESPACE);
        match (key.trim_matches(UNIT_FILE_WHITESPACE), kind) {
            ("Where", StorageKind::Mount | StorageKind::Automount) => where_value = Some(value),
            ("Options", StorageKind::Mount | StorageKind::Swap) => options = Some(value),
            _ => {}
        }
    }
    MountSettings {
        mount_point: where_value
            .filter(|path| path.starts_with('/'))
            .map(str::to_owned),
        nofail: options.is_some_and(|o| {
            mount_options(o).into_iter().rev().find_map(|option| {
                if names_option(option, "nofail") {
                    Some(true)
                } else if names_option(option, "fail") {
                    Some(false)
                } else {
                    None
                }
            }) == Some(true)
        }),
    }
}

/// The options in an `Options=` value, split where systemd's option
/// matcher (`fstab_filter_options` in `src/shared/fstab-util.c`) splits
/// them: at each comma not escaped by a backslash, a backslash escaping
/// whatever character follows it. Nothing is trimmed: whitespace inside the
/// value is part of the option beside it.
fn mount_options(value: &str) -> Vec<&str> {
    let mut options = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (i, c) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == ',' {
            options.push(&value[start..i]);
            start = i + 1;
        }
    }
    options.push(&value[start..]);
    options
}

/// Whether `option` is the mount option `name`, alone or with a value
/// (`name=...`), as systemd's option matcher compares them.
fn names_option(option: &str, name: &str) -> bool {
    option
        .strip_prefix(name)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('='))
}

/// Fill in a finding's `mount_point` and `nofail` from the unit file
/// systemd loads for it: the `/etc` file for a local or shadowed unit, the
/// vendor file for an unowned vendor unit (masked or not). `nofail` is
/// `Some` only for a mount or swap unit whose file was read; an automount
/// has no `Options=`. A mask with no unit file has neither. A file that is
/// not text, or is over the executor's size cap, leaves both `None`; its
/// config entry, if it has one, records that the content was not
/// captured. Any other read error leaves both `None` and goes to
/// `read_failures`.
fn read_mount_settings(
    exec: &dyn Executor,
    finding: &mut MountUnitFinding,
    read_failures: &mut Vec<String>,
) {
    let dir = match finding.unit.fragment() {
        AdminStorageFragment::Local | AdminStorageFragment::Shadowed { .. } => ETC_UNIT_DIR,
        AdminStorageFragment::UnownedVendor => VENDOR_UNIT_DIR,
        AdminStorageFragment::MaskOnly => return,
    };
    let name = finding.unit.name();
    let path = format!("{dir}/{}", name.as_unit_name().as_str());
    let content = match exec.read_file(Path::new(&path)) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => return,
        Err(e) => {
            read_failures.push(format!("{path}: {e}"));
            return;
        }
    };
    let settings = mount_settings(name.kind(), &content);
    finding.mount_point = settings.mount_point;
    finding.nofail = match name.kind() {
        StorageKind::Mount | StorageKind::Swap => Some(settings.nofail),
        StorageKind::Automount => None,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Where=` and `Options=` are read from the section of the unit's own
    /// kind, last assignment winning, as systemd reads them; `What=` is
    /// never read. CRLF line ends and trailing whitespace are not part of a
    /// value (plan review focus 3).
    #[test]
    fn mount_settings_read_where_and_options_from_the_kinds_section() {
        use StorageKind::{Automount, Mount, Swap};
        let settings = |mount_point: Option<&str>, nofail: bool| MountSettings {
            mount_point: mount_point.map(str::to_owned),
            nofail,
        };
        let cases: &[(&str, StorageKind, &str, MountSettings)] = &[
            (
                "absolute",
                Mount,
                "[Mount]\nWhere=/srv\n",
                settings(Some("/srv"), false),
            ),
            (
                "relative",
                Mount,
                "[Mount]\nWhere=srv\n",
                settings(None, false),
            ),
            (
                "empty Where",
                Mount,
                "[Mount]\nWhere=\n",
                settings(None, false),
            ),
            (
                "CRLF and trailing whitespace",
                Mount,
                "[Mount]\r\nWhere=/srv \r\nOptions=defaults,nofail\r\n",
                settings(Some("/srv"), true),
            ),
            (
                "spaces round =",
                Mount,
                "[Mount]\nWhere = /srv\nOptions = nofail\n",
                settings(Some("/srv"), true),
            ),
            (
                "nofail among others",
                Mount,
                "[Mount]\nOptions=rw,nofail,noatime\n",
                settings(None, true),
            ),
            (
                "nofail as a prefix",
                Mount,
                "[Mount]\nOptions=nofailover,x-nofail\n",
                settings(None, false),
            ),
            (
                "header with trailing space",
                Mount,
                "[Mount]  \nWhere=/srv\n",
                settings(Some("/srv"), false),
            ),
            (
                "keys in [Unit]",
                Mount,
                "[Unit]\nWhere=/srv\nOptions=nofail\n[Mount]\nWhat=/dev/sdb1\n",
                settings(None, false),
            ),
            (
                "keys in [Install]",
                Mount,
                "[Mount]\nWhat=x\n[Install]\nWhere=/srv\nOptions=nofail\n",
                settings(None, false),
            ),
            (
                "another kind's section",
                Mount,
                "[Swap]\nOptions=nofail\n[Automount]\nWhere=/srv\n",
                settings(None, false),
            ),
            (
                "later Where overrides",
                Mount,
                "[Mount]\nWhere=/srv\nWhere=/data\n",
                settings(Some("/data"), false),
            ),
            (
                "empty Options resets",
                Mount,
                "[Mount]\nOptions=nofail\nOptions=\n",
                settings(None, false),
            ),
            (
                "later Options overrides",
                Mount,
                "[Mount]\nOptions=defaults\nOptions=nofail\n",
                settings(None, true),
            ),
            (
                "fail after nofail",
                Mount,
                "[Mount]\nOptions=nofail,fail\n",
                settings(None, false),
            ),
            (
                "nofail after fail",
                Mount,
                "[Mount]\nOptions=fail,nofail\n",
                settings(None, true),
            ),
            (
                "commented keys",
                Mount,
                "[Mount]\n#Where=/srv\n;Options=nofail\n",
                settings(None, false),
            ),
            (
                "automount Where",
                Automount,
                "[Automount]\nWhere=/mnt/a\n",
                settings(Some("/mnt/a"), false),
            ),
            (
                "automount has no Options",
                Automount,
                "[Automount]\nWhere=/mnt/a\nOptions=nofail\n",
                settings(Some("/mnt/a"), false),
            ),
            (
                "swap Options",
                Swap,
                "[Swap]\nWhat=/dev/sdb2\nOptions=defaults,nofail\r\n",
                settings(None, true),
            ),
            (
                "swap has no Where",
                Swap,
                "[Swap]\nWhere=/swap\n",
                settings(None, false),
            ),
            (
                "no section",
                Mount,
                "Where=/srv\nOptions=nofail\n",
                settings(None, false),
            ),
            ("not text a unit holds", Mount, "", settings(None, false)),
            // Logical lines: a line ending in `\` continues onto the next,
            // so the next line is part of the value, never a key or a
            // section header of its own.
            (
                "a key in a continuation is part of the value",
                Mount,
                "[Mount]\nWhere=/srv/data\nWhat=https://files.example/volume?\\\n\
                 Where=/opt/token=mount-secret-91c2\nOptions=nofail\n",
                settings(Some("/srv/data"), true),
            ),
            (
                "a continued Where",
                Mount,
                "[Mount]\nWhere=/srv/\\\ndata\n",
                settings(Some("/srv/ data"), false),
            ),
            (
                "a continued Options",
                Mount,
                "[Mount]\nOptions=defaults\\\n,nofail\n",
                settings(None, true),
            ),
            (
                "a continuation that looks like [Unit] stays in [Mount]",
                Mount,
                "[Mount]\nWhat=/dev/sdb1\\\n[Unit]\nWhere=/srv\n",
                settings(Some("/srv"), false),
            ),
            (
                "a continuation that looks like [Mount] stays in [Unit]",
                Mount,
                "[Unit]\nDescription=data\\\n[Mount]\nWhere=/srv\n",
                settings(None, false),
            ),
            (
                "a comment inside a continuation is skipped",
                Mount,
                "[Mount]\nWhat=/dev/sdb1\\\n# Where=/opt\nWhere=/srv\nOptions=nofail\n",
                settings(None, true),
            ),
            (
                "an empty line ends a continuation",
                Mount,
                "[Mount]\nWhat=/dev/sdb1\\\n\nWhere=/srv\n",
                settings(Some("/srv"), false),
            ),
            (
                "an escaped backslash does not continue",
                Mount,
                "[Mount]\nWhat=/dev/sdb1\\\\\nWhere=/srv\n",
                settings(Some("/srv"), false),
            ),
            (
                "a continuation open at the end of the file",
                Mount,
                "[Mount]\nWhere=/srv\nWhat=/dev/sdb1\\",
                settings(Some("/srv"), false),
            ),
            // Option boundaries, as systemd's option matcher reads them:
            // only an unescaped comma separates; whitespace inside the
            // value belongs to the option beside it; an option matches a
            // name alone or followed by `=`.
            (
                "whitespace inside the value is part of an option",
                Mount,
                "[Mount]\nOptions=rw, nofail\n",
                settings(None, false),
            ),
            (
                "whitespace on both sides of an option",
                Mount,
                "[Mount]\nOptions=rw, nofail ,noatime\n",
                settings(None, false),
            ),
            (
                "whitespace round the whole value is not",
                Mount,
                "[Mount]\nOptions=  nofail  \n",
                settings(None, true),
            ),
            (
                "fail after nofail with a space",
                Mount,
                "[Mount]\nOptions=nofail, fail\n",
                settings(None, true),
            ),
            (
                "an escaped comma joins two options",
                Mount,
                "[Mount]\nOptions=x-opt\\,nofail\n",
                settings(None, false),
            ),
            (
                "an escaped comma after nofail",
                Mount,
                "[Mount]\nOptions=nofail\\,x-opt\n",
                settings(None, false),
            ),
            (
                "an escaped backslash before a comma",
                Mount,
                "[Mount]\nOptions=x-opt\\\\,nofail\n",
                settings(None, true),
            ),
            (
                "nofail with a value",
                Mount,
                "[Mount]\nOptions=nofail=1\n",
                settings(None, true),
            ),
            (
                "fail with a value after nofail",
                Mount,
                "[Mount]\nOptions=nofail,fail=0\n",
                settings(None, false),
            ),
            (
                "empty options",
                Mount,
                "[Mount]\nOptions=,nofail,\n",
                settings(None, true),
            ),
            (
                "a continuation after the comma leaves a space before nofail",
                Mount,
                "[Mount]\nOptions=defaults,\\\nnofail\n",
                settings(None, false),
            ),
            (
                "CRLF with a continuation",
                Mount,
                "[Mount]\r\nWhere=/srv\r\nWhat=x?\\\r\nWhere=/opt/token=y\r\n",
                settings(Some("/srv"), false),
            ),
            (
                "a lone CR ends a line",
                Mount,
                "[Mount]\nWhere=/srv\rWhat=https://files.example/v?token=z\n",
                settings(Some("/srv"), false),
            ),
        ];
        for (label, kind, content, expected) in cases {
            assert_eq!(&mount_settings(*kind, content), expected, "{label}");
        }
    }

    /// A swap finding's `nofail` is set only when its unit file was read:
    /// `None` for a file that is not text or is over the size cap
    /// (`InvalidData`) and for a read error, which also goes to
    /// `read_failures`; `Some` from a readable file (the controls). The
    /// read error is reached here, not through a scan, because the scan
    /// resolves the same file first and stops on the same error.
    #[test]
    fn read_mount_settings_records_nofail_only_when_the_file_was_read() {
        use crate::executor::mock::MockExecutor;
        use std::io::ErrorKind;
        const PATH: &str = "/etc/systemd/system/data.swap";
        let finding = || {
            let name = UnitName::try_from("data.swap".to_owned()).expect("a unit name");
            let subject = UnitSubject::new(name.clone(), UnitFragment::Local { file: name })
                .expect("a subject");
            MountUnitFinding {
                unit: AdvisoryUnit::try_from(subject).expect("an advisory unit"),
                state_change: None,
                mount_point: None,
                nofail: None,
                aggregate: None,
            }
        };
        let cases = [
            (
                "nofail",
                MockExecutor::new().with_file(PATH, "[Swap]\nOptions=nofail\n"),
                Some(true),
                false,
            ),
            (
                "no nofail",
                MockExecutor::new().with_file(PATH, "[Swap]\nWhat=/dev/vdb\n"),
                Some(false),
                false,
            ),
            (
                "not text",
                MockExecutor::new().with_file_error(PATH, ErrorKind::InvalidData),
                None,
                false,
            ),
            (
                "read error",
                MockExecutor::new().with_file_error(PATH, ErrorKind::PermissionDenied),
                None,
                true,
            ),
        ];
        for (label, exec, expected, fails) in cases {
            let mut finding = finding();
            let mut read_failures = Vec::new();
            read_mount_settings(&exec, &mut finding, &mut read_failures);
            assert_eq!(finding.nofail, expected, "{label}");
            assert_eq!(
                !read_failures.is_empty(),
                fails,
                "{label}: {read_failures:?}"
            );
        }
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("sshd.service", "sshd.service"));
        assert!(!glob_match("sshd.service", "httpd.service"));
    }

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("*", "anything.service"));
        assert!(glob_match("ssh*", "sshd.service"));
        assert!(glob_match("*.service", "httpd.service"));
    }

    #[test]
    fn test_glob_match_question() {
        assert!(glob_match("ssh?.service", "sshd.service"));
        assert!(!glob_match("ssh?.service", "sshdd.service"));
    }

    #[test]
    fn test_parse_unit_files_basic() {
        let input = "UNIT FILE                                  STATE           PRESET\n\
                     sshd.service                               enabled         enabled\n\
                     cups.service                               disabled        disabled\n\
                     \n\
                     2 unit files listed.\n";
        let entries = parse_unit_files(input);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].unit, "sshd.service");
        assert_eq!(entries[0].state, "enabled");
        assert_eq!(entries[0]._preset, "enabled");
    }

    #[test]
    fn test_parse_unit_files_empty() {
        let input = "UNIT FILE                                  STATE           PRESET\n\
                     \n\
                     0 unit files listed.\n";
        let entries = parse_unit_files(input);
        assert!(entries.is_empty());
    }

    // --- Size-cap / read-failure → Degraded tests ---

    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::traits::progress::NullProgress;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::system::SourceSystem;

    fn svc_test_os_release() -> OsRelease {
        OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        }
    }

    fn svc_base_mock() -> MockExecutor {
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sshd.service                               enabled         enabled\n\
                             httpd.service                              enabled         disabled\n\
                             \n\
                             2 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\ndisable *\n",
            );
        with_enabled_vendor_unit(
            with_enabled_vendor_unit(exec, "sshd.service"),
            "httpd.service",
        )
    }

    /// The `[Install]` section of an enabled unit in these tests.
    const WANTED_BY_MULTI_USER: &str = "[Install]\nWantedBy=multi-user.target\n";

    /// `exec` with a vendor `unit` wanted by `multi-user.target` and the
    /// link `systemctl enable` makes for it, added to what
    /// `/etc/systemd/system` already lists: the host on which
    /// `list-unit-files` says `enabled`.
    fn with_enabled_vendor_unit(exec: MockExecutor, unit: &str) -> MockExecutor {
        with_enable_link(
            exec.with_file(
                &format!("/usr/lib/systemd/system/{unit}"),
                &format!("[Unit]\nDescription={unit}\n\n{WANTED_BY_MULTI_USER}"),
            ),
            unit,
        )
    }

    /// `exec` with the `multi-user.target.wants` link to `unit`, added to
    /// what `/etc/systemd/system` already lists.
    fn with_enable_link(exec: MockExecutor, unit: &str) -> MockExecutor {
        let wants = "/etc/systemd/system/multi-user.target.wants";
        exec.with_dir_entry("/etc/systemd/system", "multi-user.target.wants")
            .with_dir_entry(wants, unit)
            .with_link(
                &format!("{wants}/{unit}"),
                &format!("/usr/lib/systemd/system/{unit}"),
            )
    }

    #[test]
    fn test_dropin_read_failure_triggers_degraded() {
        // Drop-in directory listed, .conf file listed, but file read fails (size cap)
        let exec = svc_base_mock()
            .with_dir_entry("/etc/systemd/system", "httpd.service.d")
            .with_dir("/etc/systemd/system/httpd.service.d", vec!["override.conf"]);
        // NOTE: no .with_file for override.conf → read_file will return NotFound

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Degraded { reason, partial }) => {
                assert!(
                    reason.contains("drop-in") || reason.contains("override.conf"),
                    "degraded reason must mention the failed drop-in: {reason}"
                );
                // Partial data should still contain state_changes from systemctl
                if let SectionData::Units(ref svc) = partial.section {
                    assert!(
                        !svc.state_changes.is_empty() || !svc.inventory.is_empty(),
                        "partial data must contain systemctl results"
                    );
                }
            }
            other => panic!("expected Degraded for unreadable drop-in, got {other:?}"),
        }
    }

    #[test]
    fn test_preset_file_read_failure_triggers_degraded() {
        // Directory is readable and lists a preset file, but file read fails
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sshd.service                               enabled         enabled\n\
                             \n\
                             1 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            // NOTE: no .with_file for the preset → read_file returns NotFound
            .with_dir("/etc/systemd/system", vec![]);
        let exec = with_enabled_vendor_unit(exec, "sshd.service");

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Degraded { reason, partial }) => {
                assert!(
                    reason.contains("preset") || reason.contains("90-default.preset"),
                    "degraded reason must mention the failed preset file: {reason}"
                );
                // Partial data should still contain systemctl results
                if let SectionData::Units(ref svc) = partial.section {
                    assert!(
                        !svc.inventory.is_empty(),
                        "partial data must contain systemctl results"
                    );
                }
            }
            other => panic!("expected Degraded for unreadable preset file, got {other:?}"),
        }
    }

    #[test]
    fn test_dropin_dir_unreadable_triggers_degraded() {
        // Per-unit drop-in directory listed but unreadable
        let exec = svc_base_mock().with_dir_entry("/etc/systemd/system", "httpd.service.d");
        // NOTE: no .with_dir for httpd.service.d → read_dir returns Err

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Degraded { reason, .. }) => {
                assert!(
                    reason.contains("drop-in") || reason.contains("httpd.service.d"),
                    "degraded reason must mention the unreadable drop-in dir: {reason}"
                );
            }
            other => panic!("expected Degraded for unreadable drop-in directory, got {other:?}"),
        }
    }

    #[test]
    fn test_etc_preset_dir_permission_denied_triggers_degraded() {
        // /etc/systemd/system-preset exists but read_dir returns PermissionDenied.
        // Admin-priority presets may be hidden: must degrade.
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sshd.service                               enabled         enabled\n\
                             \n\
                             1 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\ndisable *\n",
            )
            .with_dir_error(
                "/etc/systemd/system-preset",
                std::io::ErrorKind::PermissionDenied,
            );

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Degraded { reason, .. }) => {
                assert!(
                    reason.contains("preset") && reason.contains("/etc/systemd/system-preset"),
                    "degraded reason must mention the unreadable preset dir: {reason}"
                );
            }
            other => panic!(
                "expected Degraded for PermissionDenied on /etc/systemd/system-preset, got {other:?}"
            ),
        }
    }

    #[test]
    fn test_etc_preset_dir_not_found_not_degraded() {
        // /etc/systemd/system-preset doesn't exist (NotFound): this is
        // normal on systems without admin presets. NOT degraded.
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sshd.service                               enabled         enabled\n\
                             \n\
                             1 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\ndisable *\n",
            );
        // NOTE: no /etc/systemd/system-preset registered → read_dir returns NotFound

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        assert!(
            result.is_ok(),
            "/etc/systemd/system-preset NotFound must NOT degrade, got: {result:?}"
        );
    }

    #[test]
    fn test_etc_systemd_system_permission_denied_triggers_degraded() {
        // /etc/systemd/system (base drop-in directory) exists but read_dir
        // returns PermissionDenied. All drop-in configurations are hidden.
        let exec = svc_base_mock()
            .with_dir_error("/etc/systemd/system", std::io::ErrorKind::PermissionDenied);

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Degraded { reason, .. }) => {
                assert!(
                    reason.contains("drop-in") && reason.contains("/etc/systemd/system"),
                    "degraded reason must mention the unreadable drop-in base dir: {reason}"
                );
            }
            other => panic!(
                "expected Degraded for PermissionDenied on /etc/systemd/system, got {other:?}"
            ),
        }
    }

    // --- Operator-intent whitelist tests ---

    /// The name gate keeps every kind and `dbus-broker.service`, and drops a
    /// `dbus-org.` activation alias, an instance, and a template that is not
    /// masked.
    #[test]
    fn name_gate_table() {
        let cases: &[(&str, &str, bool)] = &[
            ("sshd.service", "enabled", true),
            ("dbus.service", "enabled", true),
            ("dbus-broker.service", "enabled", true),
            // Not the activation-alias prefix.
            ("dbus-:1.2-org.something.service", "enabled", true),
            ("systemd-sysupdate.timer", "enabled", true),
            ("multi-user.target", "enabled", true),
            ("dbus.socket", "enabled", true),
            ("tmp.mount", "masked", true),
            ("getty@.service", "masked", true),
            (
                "dbus-org.freedesktop.NetworkManager.service",
                "enabled",
                false,
            ),
            ("dbus-org.freedesktop.timedate1.service", "disabled", false),
            ("dbus-org.bluez.service", "masked", false),
            ("getty@.service", "enabled", false),
            ("getty@.service", "disabled", false),
            // A listed instance: a concrete instance file, or its mask.
            ("getty@tty1.service", "enabled", true),
            ("getty@tty1.service", "masked", true),
            // Kinds § 5 excludes, in any state, durable or not.
            ("session-7.scope", "transient", false),
            ("init.scope", "static", false),
            ("dev-sda.device", "enabled", false),
            ("dev-sda.device", "masked-runtime", false),
        ];
        for &(unit, state, kept) in cases {
            let entry = UnitFileEntry {
                unit: unit.into(),
                state: state.into(),
                _preset: String::new(),
            };
            let mut warnings = Vec::new();
            let gated = gate(&entry, &HostLinks::default(), &mut warnings);
            assert_eq!(gated.is_some(), kept, "{unit} {state}");
            assert!(warnings.is_empty(), "{unit} {state}: {warnings:?}");
        }
    }

    /// A unit of a kind the scan leaves out (`.scope`, `.device`) is
    /// dropped before its state is read, so it gets no warning in any state.
    /// Control: a service in the same transient state keeps today's warning.
    #[test]
    fn excluded_kinds_get_no_state_warning() {
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             session-7.scope                            transient       -\n\
                             dev-sda.device                             enabled         enabled\n\
                             run-r1.service                             transient       -\n\
                             \n\
                             3 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "disable *\n",
            )
            .with_dir("/etc/systemd/system", vec![]);
        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let output = UnitsInspector::new()
            .inspect(&ctx, &NullProgress)
            .expect("the scan completes");
        let messages: Vec<&str> = output.warnings.iter().map(|w| w.message.as_str()).collect();
        assert_eq!(
            messages,
            ["unit run-r1.service has state 'transient' - transient state is not migrated"]
        );
    }

    #[test]
    fn test_dbus_alias_not_captured_no_operator_evidence() {
        // D-Bus aliases in a different state than their preset are NOT
        // captured: the operator-intent model excludes them structurally
        // because they are not real services.
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             httpd.service                              enabled         disabled\n\
                             dbus-org.freedesktop.NetworkManager.service disabled        enabled\n\
                             dbus-org.freedesktop.timedate1.service     disabled        enabled\n\
                             \n\
                             3 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable dbus-org.freedesktop.NetworkManager.service\n\
                 enable dbus-org.freedesktop.timedate1.service\n\
                 disable *\n",
            )
            .with_dir("/etc/systemd/system", vec![]);
        let exec = with_enabled_vendor_unit(exec, "httpd.service");

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress).unwrap();
        if let SectionData::Units(ref svc) = result.section {
            // httpd should be captured (operator enabled a preset-disabled service)
            assert!(
                svc.state_changes
                    .iter()
                    .any(|sc| sc.unit().name().as_str() == "httpd.service"),
                "httpd.service must be in state_changes"
            );
            // D-Bus aliases must NOT appear: no operator evidence
            assert!(
                !svc.state_changes
                    .iter()
                    .any(|sc| sc.unit().name().as_str().starts_with("dbus-")),
                "D-Bus aliases must not be in state_changes (no operator evidence), got: {:?}",
                svc.state_changes
            );
        } else {
            panic!("expected SectionData::Units");
        }
    }

    #[test]
    fn test_masked_service_captured_as_operator_intent() {
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sshd.service                               enabled         enabled\n\
                             cups.service                               masked          enabled\n\
                             \n\
                             2 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\nenable cups.service\ndisable *\n",
            )
            .with_dir("/etc/systemd/system", vec![])
            .with_file("/usr/lib/systemd/system/cups.service", "[Unit]\n")
            .with_link("/etc/systemd/system/cups.service", "/dev/null");

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress).unwrap();
        if let SectionData::Units(ref svc) = result.section {
            // sshd matches preset: should not be in state_changes
            assert!(
                !svc.state_changes
                    .iter()
                    .any(|sc| sc.unit().name().as_str() == "sshd.service"),
                "sshd.service matches preset, must not be in state_changes"
            );
            // cups is masked: operator intent, must be captured
            let cups = svc
                .state_changes
                .iter()
                .find(|sc| sc.unit().name().as_str() == "cups.service");
            assert!(
                cups.is_some(),
                "masked cups.service must be in state_changes"
            );
            let cups = cups.unwrap();
            assert_eq!(
                cups.implied_action(),
                inspectah_core::types::units::UnitAction::Mask,
                "masked service action must be Mask"
            );
            assert_eq!(cups.current_state(), UnitState::Masked);
            assert!(
                cups.disposition.is_included(),
                "masked service must be included"
            );
        } else {
            panic!("expected SectionData::Units");
        }
    }

    #[test]
    fn test_operator_disabled_preset_enabled_service_captured() {
        // A service that is enabled by preset but disabled by operator
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             firewalld.service                          disabled        enabled\n\
                             \n\
                             1 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable firewalld.service\ndisable *\n",
            )
            .with_dir("/etc/systemd/system", vec![])
            .with_file(
                "/usr/lib/systemd/system/firewalld.service",
                &format!("[Unit]\n\n{WANTED_BY_MULTI_USER}"),
            );

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress).unwrap();
        if let SectionData::Units(ref svc) = result.section {
            let fw = svc
                .state_changes
                .iter()
                .find(|sc| sc.unit().name().as_str() == "firewalld.service");
            assert!(
                fw.is_some(),
                "operator-disabled firewalld must be in state_changes"
            );
            assert_eq!(
                fw.unwrap().implied_action(),
                inspectah_core::types::units::UnitAction::Disable
            );
        } else {
            panic!("expected SectionData::Units");
        }
    }

    #[test]
    fn test_sssd_untouched_services_not_captured() {
        // SSSD services that the operator never touched: their preset is
        // "disable" and current state is "disabled": no divergence, no
        // operator evidence. Must NOT appear in state_changes.
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sssd.service                               disabled        disabled\n\
                             sssd-kcm.service                           disabled        disabled\n\
                             sssd-autofs.service                        disabled        disabled\n\
                             \n\
                             3 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "disable sssd*\ndisable *\n",
            )
            .with_dir("/etc/systemd/system", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress).unwrap();
        if let SectionData::Units(ref svc) = result.section {
            assert!(
                !svc.state_changes
                    .iter()
                    .any(|sc| sc.unit().name().as_str().starts_with("sssd")),
                "untouched SSSD services must not be in state_changes (no operator evidence), got: {:?}",
                svc.state_changes
            );
        } else {
            panic!("expected SectionData::Units");
        }
    }

    #[test]
    fn test_sysupdate_not_captured_no_operator_evidence() {
        // systemd-sysupdate services diverge from preset but the operator
        // never touched them. With the whitelist model, they are excluded
        // because there's no operator action evidence (state matches preset
        // or no preset match at all).
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             systemd-sysupdate.service                  disabled        disabled\n\
                             systemd-sysupdate-cleanup.service          disabled        disabled\n\
                             sshd.service                               enabled         enabled\n\
                             \n\
                             3 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\ndisable *\n",
            )
            .with_dir("/etc/systemd/system", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress).unwrap();
        if let SectionData::Units(ref svc) = result.section {
            assert!(
                !svc.state_changes
                    .iter()
                    .any(|sc| sc.unit().name().as_str().contains("sysupdate")),
                "sysupdate services must not be in state_changes (no operator evidence), got: {:?}",
                svc.state_changes
            );
        } else {
            panic!("expected SectionData::Units");
        }
    }

    #[test]
    fn test_preset_divergence_without_operator_action_not_captured() {
        // A service whose state differs from preset but where the difference
        // is just "package installation vs. image presets": no evidence
        // the operator ran systemctl. With no preset match, it's excluded.
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             some-obscure.service                       disabled        enabled\n\
                             \n\
                             1 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                // Note: no preset rule for some-obscure.service, only
                // the wildcard disable catches it
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\ndisable *\n",
            )
            .with_dir("/etc/systemd/system", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress).unwrap();
        if let SectionData::Units(ref svc) = result.section {
            // The wildcard "disable *" matches some-obscure.service with
            // default_state="disabled", and current state is also "disabled",
            // so there is no divergence. If they were different, the whitelist model
            // captures it because the preset is definitive. This test verifies
            // the matching path.
            //
            // In this case: preset says "disabled" (via wildcard), state is
            // "disabled": match, not captured. Good.
            assert!(
                !svc.state_changes
                    .iter()
                    .any(|sc| sc.unit().name().as_str() == "some-obscure.service"),
                "service matching preset via wildcard must not be in state_changes"
            );
        } else {
            panic!("expected SectionData::Units");
        }
    }

    #[test]
    fn test_dropin_captured_for_service() {
        // Drop-in override files represent operator customization
        let exec = svc_base_mock()
            .with_dir_entry("/etc/systemd/system", "httpd.service.d")
            .with_dir("/etc/systemd/system/httpd.service.d", vec!["restart.conf"])
            .with_file(
                "/etc/systemd/system/httpd.service.d/restart.conf",
                "[Service]\nRestart=always\nRestartSec=5\n",
            );

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress).unwrap();
        if let SectionData::Units(ref svc) = result.section {
            assert!(
                !svc.drop_ins.is_empty(),
                "drop-in overrides must be captured"
            );
            let dropin = svc.drop_ins.iter().find(|d| d.unit == "httpd.service");
            assert!(dropin.is_some(), "httpd.service drop-in must be captured");
            let dropin = dropin.unwrap();
            assert!(dropin.content.contains("Restart=always"));
            assert!(dropin.disposition.is_included());
        } else {
            panic!("expected SectionData::Units");
        }
    }

    #[test]
    fn test_services_ok_when_dropins_readable() {
        let exec = svc_base_mock()
            .with_dir_entry("/etc/systemd/system", "httpd.service.d")
            .with_dir("/etc/systemd/system/httpd.service.d", vec!["override.conf"])
            .with_file(
                "/etc/systemd/system/httpd.service.d/override.conf",
                "[Service]\nRestart=always\n",
            );

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        assert!(
            result.is_ok(),
            "all drop-ins readable → must succeed, got: {result:?}"
        );
    }

    #[test]
    fn test_detect_shadow_type_full_shadow() {
        // Both /etc and /usr/lib unit files exist → FullShadow
        let exec = MockExecutor::new()
            .with_file("/etc/systemd/system/httpd.service", "[Unit]\n")
            .with_file("/usr/lib/systemd/system/httpd.service", "[Unit]\n")
            .with_command(
                "test -d /etc/systemd/system/httpd.service.d",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let shadow_type = detect_shadow_type(&exec, "httpd.service");
        assert_eq!(shadow_type, Ok(Some(ShadowType::FullShadow)));
    }

    #[test]
    fn test_detect_shadow_type_drop_in() {
        // Only drop-in directory exists → DropIn
        let exec = MockExecutor::new()
            .with_file("/usr/lib/systemd/system/httpd.service", "[Unit]\n")
            .with_command(
                "test -d /etc/systemd/system/httpd.service.d",
                ExecResult {
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let shadow_type = detect_shadow_type(&exec, "httpd.service");
        assert_eq!(shadow_type, Ok(Some(ShadowType::DropIn)));
    }

    #[test]
    fn test_detect_shadow_type_none() {
        // Neither full shadow nor drop-in exists → None
        let exec = MockExecutor::new().with_command(
            "test -d /etc/systemd/system/httpd.service.d",
            ExecResult {
                exit_code: 1,
                ..Default::default()
            },
        );

        let shadow_type = detect_shadow_type(&exec, "httpd.service");
        assert_eq!(shadow_type, Ok(None));
    }

    #[test]
    fn test_detect_shadow_type_custom_service_not_drop_in() {
        // A service defined only in /etc (not in /usr) is a custom service,
        // not a drop-in override, even if it has a .d/ directory
        let exec = MockExecutor::new()
            // Exists in /etc, NOT in /usr.
            .with_file("/etc/systemd/system/custom.service", "[Unit]\n")
            .with_command(
                "test -d /etc/systemd/system/custom.service.d",
                ExecResult {
                    exit_code: 0, // has .d/ dir
                    ..Default::default()
                },
            );

        let shadow_type = detect_shadow_type(&exec, "custom.service");
        assert_eq!(shadow_type, Ok(None));
    }

    #[test]
    fn test_full_shadow_carries_rationale() {
        // Full shadow should populate shadow_rationale field
        let exec = svc_base_mock()
            .with_dir_entry("/etc/systemd/system", "httpd.service.d")
            .with_dir("/etc/systemd/system/httpd.service.d", vec!["override.conf"])
            .with_file(
                "/etc/systemd/system/httpd.service.d/override.conf",
                "[Service]\nRestart=always\n",
            )
            .with_file("/etc/systemd/system/httpd.service", "[Unit]\n")
            .with_file("/usr/lib/systemd/system/httpd.service", "[Unit]\n")
            .with_command(
                "test -d /etc/systemd/system/httpd.service.d",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress).unwrap();
        if let SectionData::Units(ref svc) = result.section {
            let dropin = svc.drop_ins.iter().find(|d| d.unit == "httpd.service");
            assert!(dropin.is_some(), "httpd.service drop-in must be captured");
            let dropin = dropin.unwrap();
            assert_eq!(dropin.shadow_type, Some(ShadowType::FullShadow));
            assert_eq!(
                dropin.shadow_rationale,
                Some(
                    "Full unit shadow: base image updates to this unit will be silently ignored"
                        .to_string()
                )
            );
        } else {
            panic!("expected SectionData::Units");
        }
    }

    #[test]
    fn test_drop_in_no_rationale() {
        // Drop-in (not full shadow) should have shadow_type but no rationale
        let exec = svc_base_mock()
            .with_dir_entry("/etc/systemd/system", "httpd.service.d")
            .with_dir("/etc/systemd/system/httpd.service.d", vec!["override.conf"])
            .with_file(
                "/etc/systemd/system/httpd.service.d/override.conf",
                "[Service]\nRestart=always\n",
            )
            .with_file("/usr/lib/systemd/system/httpd.service", "[Unit]\n")
            .with_command(
                "test -d /etc/systemd/system/httpd.service.d",
                ExecResult {
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress).unwrap();
        if let SectionData::Units(ref svc) = result.section {
            let dropin = svc.drop_ins.iter().find(|d| d.unit == "httpd.service");
            assert!(dropin.is_some(), "httpd.service drop-in must be captured");
            let dropin = dropin.unwrap();
            assert_eq!(dropin.shadow_type, Some(ShadowType::DropIn));
            assert_eq!(dropin.shadow_rationale, None);
        } else {
            panic!("expected SectionData::Units");
        }
    }

    /// A drop-in's `shadow_type` follows the form of its parent's path, as
    /// the resolver and the full shadow scan read it: a zero-length file
    /// and a link to `/dev/null` are masks, so a drop-in beside either is an
    /// ordinary drop-in with no full-shadow rationale. Control: a non-empty
    /// file there, comment-only, replaces the vendor unit, and the drop-in
    /// is labelled a full shadow as before.
    #[test]
    fn a_drop_in_under_a_mask_is_not_a_full_shadow() {
        const UNIT: &str = "quiet.service";
        let parent = format!("/etc/systemd/system/{UNIT}");
        let cases: &[(&str, &str, Option<ShadowType>, bool)] = &[
            (
                "zero-length file",
                "masked",
                Some(ShadowType::DropIn),
                false,
            ),
            (
                "link to /dev/null",
                "masked",
                Some(ShadowType::DropIn),
                false,
            ),
            (
                "comment-only file",
                "static",
                Some(ShadowType::FullShadow),
                true,
            ),
        ];
        for (form, state, expected, rationale) in cases {
            let exec = MockExecutor::new()
                .with_command(
                    "systemctl list-unit-files --no-pager",
                    ExecResult {
                        stdout: format!(
                            "UNIT FILE                                  STATE           PRESET\n\
                             {UNIT} {state} disabled\n\n1 unit files listed.\n"
                        ),
                        exit_code: 0,
                        ..Default::default()
                    },
                )
                .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
                .with_file(
                    "/usr/lib/systemd/system-preset/90-default.preset",
                    "disable *\n",
                )
                .with_dir_entry("/etc/systemd/system", "quiet.service.d")
                .with_dir("/etc/systemd/system/quiet.service.d", vec!["override.conf"])
                .with_file(
                    "/etc/systemd/system/quiet.service.d/override.conf",
                    "[Service]\nNice=5\n",
                )
                .with_file("/usr/lib/systemd/system/quiet.service", "[Service]\n")
                .with_command(
                    "test -d /etc/systemd/system/quiet.service.d",
                    ExecResult::default(),
                );
            let exec = match *form {
                "zero-length file" => exec.with_file(&parent, ""),
                "link to /dev/null" => exec.with_link(&parent, "/dev/null"),
                _ => exec.with_file(&parent, "# replaced by hand\n"),
            };
            let source = SourceSystem::PackageBased {
                os_release: svc_test_os_release(),
            };
            let ctx = InspectionContext {
                source_system: &source,
                executor: &exec,
                rpm_state: None,
                baseline_data: None,
            };

            let output = UnitsInspector::new()
                .inspect(&ctx, &NullProgress)
                .unwrap_or_else(|e| panic!("{form}: {e:?}"));
            let SectionData::Units(units) = output.section else {
                panic!("{form}: expected SectionData::Units");
            };
            let rows: Vec<_> = units
                .drop_ins
                .iter()
                .filter(|d| d.path.ends_with("/override.conf"))
                .collect();
            assert_eq!(rows.len(), 1, "{form}: {:?}", units.drop_ins);
            assert_eq!(&rows[0].shadow_type, expected, "{form}");
            assert_eq!(rows[0].shadow_rationale.is_some(), *rationale, "{form}");
        }
    }

    #[test]
    fn test_full_shadow_detected_for_preset_matched_service() {
        // sshd is enabled with preset=enable → a preset-matched inventory entry,
        // NOT state_changes. A full shadow at /etc/systemd/system/sshd.service
        // must still be detected by the independent scan (step 5c).
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sshd.service                               enabled         enabled\n\
                             \n\
                             1 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\ndisable *\n",
            )
            // /etc/systemd/system has the bare sshd.service file (full shadow)
            .with_dir_entry("/etc/systemd/system", "sshd.service")
            .with_file(
                "/etc/systemd/system/sshd.service",
                &format!(
                    "[Unit]\nDescription=Custom sshd\n[Service]\nExecStart=/usr/sbin/sshd -D\n\n\
                     {WANTED_BY_MULTI_USER}"
                ),
            )
            // Vendor counterpart exists, with the same `[Install]`: a vendor
            // file with none would make the shadow's `WantedBy=` an added
            // link, and the unit an `enable` row (§ 7.4).
            .with_file(
                "/usr/lib/systemd/system/sshd.service",
                &format!("[Unit]\n\n{WANTED_BY_MULTI_USER}"),
            );
        let exec = with_enable_link(exec, "sshd.service");

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress).unwrap();
        if let SectionData::Units(ref svc) = result.section {
            // sshd matches preset: must NOT be in state_changes
            assert!(
                !svc.state_changes
                    .iter()
                    .any(|sc| sc.unit().name().as_str() == "sshd.service"),
                "sshd.service matches preset, must not be in state_changes"
            );
            // sshd must be a preset-matched inventory entry
            assert!(
                svc.inventory
                    .iter()
                    .any(|e| e.unit.as_str() == "sshd.service" && e.matches_preset()),
                "sshd.service must be a preset-matched inventory entry"
            );
            // The full shadow must be detected via the independent scan
            let shadow = svc.drop_ins.iter().find(|d| {
                d.unit == "sshd.service" && d.shadow_type == Some(ShadowType::FullShadow)
            });
            assert!(
                shadow.is_some(),
                "full shadow for preset-matched sshd.service must be detected, got drop_ins: {:?}",
                svc.drop_ins
            );
            let shadow = shadow.unwrap();
            assert!(
                shadow.shadow_rationale.is_some(),
                "full shadow must carry a rationale"
            );
            assert!(
                shadow.content.contains("Custom sshd"),
                "shadow content must be read from /etc/systemd/system/sshd.service"
            );
        } else {
            panic!("expected SectionData::Units");
        }
    }

    #[test]
    fn test_unreadable_shadow_file_degrades_inspector() {
        // A full shadow file that cannot be read should degrade the inspector,
        // not silently produce an empty content entry. The listing does not
        // name the unit, so the shadow scan is the first to read its file (a
        // listed unit's file is read when it is resolved, which degrades the
        // scan with its own reason).
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             other.service                              disabled        disabled\n\
                             \n\
                             1 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "disable sshd.service\ndisable *\n",
            )
            .with_dir_entry("/etc/systemd/system", "sshd.service")
            // Shadow file exists but is unreadable
            .with_file_error(
                "/etc/systemd/system/sshd.service",
                std::io::ErrorKind::PermissionDenied,
            )
            // Vendor counterpart exists
            .with_file("/usr/lib/systemd/system/sshd.service", "[Unit]\n")
            .with_file("/usr/lib/systemd/system/other.service", "[Unit]\n");

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = UnitsInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Degraded { reason, .. }) => {
                assert!(
                    reason.contains("drop-in conf read failures"),
                    "degradation reason should mention read failures, got: {reason}"
                );
            }
            Ok(_) => panic!("expected Degraded error for unreadable shadow file"),
            Err(e) => panic!("expected Degraded, got: {e:?}"),
        }
    }

    /// A listed storage unit that systemctl calls `enabled`.
    fn storage_unit(unit: &str) -> Listed {
        Listed {
            name: UnitName::try_from(unit.to_owned()).unwrap(),
            raw_state: "enabled".into(),
            listed: Some(UnitState::Enabled),
        }
    }

    /// The link `enable` makes for a unit wanted by `local-fs.target`.
    fn local_fs_link(unit: &str) -> HostLinks {
        let mut links = HostLinks::default();
        links.admin.insert(
            UnitName::try_from(unit.to_owned()).unwrap(),
            vec![links::Link {
                relation: inspectah_core::types::units::LinkRelation::Wants,
                parent: UnitName::try_from("local-fs.target".to_owned()).unwrap(),
            }],
        );
        links
    }

    /// A preset that disables every unit.
    fn disable_all() -> Vec<PresetRule> {
        vec![PresetRule::Plain {
            verb: PresetDefault::Disable,
            pattern: "*".into(),
        }]
    }

    /// § 7.8: a storage unit's owner is looked up before it is classified.
    /// A vendor `tmp.mount` whose owner is found is a carried state change;
    /// the same unit whose lookup fails, or answers with an empty line, is
    /// the administrator's: a mount finding and no state change.
    #[test]
    fn storage_unit_owner_is_looked_up_before_classification() {
        let rpm = |stdout: &str, exit_code: i32| ExecResult {
            stdout: stdout.into(),
            exit_code,
            ..Default::default()
        };
        let query = "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/tmp.mount";
        let vendor = || {
            MockExecutor::new().with_file(
                "/usr/lib/systemd/system/tmp.mount",
                "[Mount]\n\n[Install]\nWantedBy=local-fs.target\n",
            )
        };

        let owned = vendor().with_command(query, rpm("systemd\n", 0));
        let (mut warnings, mut failures) = (Vec::new(), Vec::new());
        let UnitFindings {
            state_changes: changes,
            mount_findings: findings,
            ..
        } = build_state_changes(
            &owned,
            vec![storage_unit("tmp.mount")],
            &local_fs_link("tmp.mount"),
            &disable_all(),
            &HashSet::new(),
            &mut warnings,
            &mut failures,
            &mut HashSet::new(),
        );
        assert!(
            warnings.is_empty() && failures.is_empty(),
            "{warnings:?} {failures:?}"
        );
        assert_eq!(changes.len(), 1);
        assert!(findings.is_empty(), "{findings:?}");
        assert!(
            matches!(changes[0].unit(), inspectah_core::types::units::CarriedUnit::PackagedStorage(u)
                if u.owning_package() == "systemd"),
            "{:?}",
            changes[0].unit()
        );

        for answer in [rpm("file is not owned by any package\n", 1), rpm("\n", 0)] {
            let unowned = vendor().with_command(query, answer);
            let mut warnings = Vec::new();
            let UnitFindings {
                state_changes: changes,
                mount_findings: findings,
                ..
            } = build_state_changes(
                &unowned,
                vec![storage_unit("tmp.mount")],
                &local_fs_link("tmp.mount"),
                &disable_all(),
                &HashSet::new(),
                &mut warnings,
                &mut failures,
                &mut HashSet::new(),
            );
            assert!(changes.is_empty(), "{changes:?}");
            assert!(warnings.is_empty(), "{warnings:?}");
            assert_eq!(findings.len(), 1, "{findings:?}");
            assert_eq!(findings[0].unit.name().as_unit_name().as_str(), "tmp.mount");
        }
    }

    fn plain(verb: PresetDefault, pattern: &str) -> PresetRule {
        PresetRule::Plain {
            verb,
            pattern: pattern.into(),
        }
    }

    /// Parse each line of `text` as one preset file named `file`.
    fn rules(file: &str, text: &str) -> Vec<PresetRule> {
        text.lines()
            .enumerate()
            .filter_map(|(i, line)| PresetRule::parse(line, file, i + 1))
            .collect()
    }

    /// T39, the parser: the list form, the plain forms, and a first word
    /// that is neither verb kept with its file and line.
    #[test]
    fn preset_lines_parse_into_their_variants() {
        use PresetDefault::{Disable, Enable};
        let file = "/etc/systemd/system-preset/10-t.preset";
        let cases = [
            (
                "enable t@.service two three",
                Some(PresetRule::EnableInstances {
                    template: "t@.service".into(),
                    instances: vec!["two".into(), "three".into()],
                }),
            ),
            ("enable t@two.service", Some(plain(Enable, "t@two.service"))),
            ("enable t@.service", Some(plain(Enable, "t@.service"))),
            ("disable t@.service x", Some(plain(Disable, "t@.service"))),
            ("enable t@*", Some(plain(Enable, "t@*"))),
            ("disable *", Some(plain(Disable, "*"))),
            (
                "enable app.service extra words",
                Some(plain(Enable, "app.service")),
            ),
            (
                "frobnicate t@.service",
                Some(PresetRule::Unknown {
                    pattern: "t@.service".into(),
                    file: file.into(),
                    line: 7,
                }),
            ),
            ("enable", None),
        ];
        for (line, expected) in cases {
            assert_eq!(PresetRule::parse(line, file, 7), expected, "{line}");
        }
    }

    /// T39, the lookup: what each rule set gives an instance with its own
    /// unit file, an instance backed only by its template, and a unit that
    /// is not an instance.
    #[test]
    fn the_preset_lookup_follows_what_the_package_ships() {
        use PresetDefault::{Disable, Enable};
        use PresetLookup::{NoRule, Rule};
        let file = "/usr/lib/systemd/system-preset/90-t.preset";
        // (rules, unit, own file, DefaultInstance=, preset)
        let cases: &[(&str, &str, bool, Option<&str>, PresetLookup)] = &[
            // An instance with its own unit file.
            (
                "enable t@two.service\ndisable *",
                "t@two.service",
                true,
                Some("one"),
                Rule(Enable),
            ),
            (
                "enable t@*\ndisable *",
                "t@two.service",
                true,
                Some("one"),
                Rule(Enable),
            ),
            (
                "enable t@.service two three\ndisable *",
                "t@two.service",
                true,
                Some("one"),
                Rule(Enable),
            ),
            (
                "enable t@.service one\ndisable *",
                "t@two.service",
                true,
                Some("one"),
                Rule(Disable),
            ),
            (
                "enable t@.service\ndisable *",
                "t@two.service",
                true,
                Some("two"),
                Rule(Disable),
            ),
            // An instance backed only by its template.
            (
                "enable t@.service\ndisable *",
                "t@one.service",
                false,
                Some("one"),
                Rule(Enable),
            ),
            (
                "enable t@.service\ndisable *",
                "t@two.service",
                false,
                Some("one"),
                Rule(Disable),
            ),
            (
                "enable t@.service\ndisable *",
                "t@one.service",
                false,
                None,
                Rule(Disable),
            ),
            (
                "enable t@*\ndisable *",
                "t@one.service",
                false,
                Some("one"),
                Rule(Enable),
            ),
            (
                "enable t@.service two three\ndisable *",
                "t@two.service",
                false,
                Some("one"),
                Rule(Enable),
            ),
            (
                "enable t@.service two three\ndisable *",
                "t@one.service",
                false,
                Some("one"),
                Rule(Disable),
            ),
            (
                "enable t@two.service\ndisable *",
                "t@two.service",
                false,
                Some("one"),
                Rule(Disable),
            ),
            (
                "disable t@.service",
                "t@one.service",
                false,
                Some("one"),
                Rule(Disable),
            ),
            (
                "frobnicate t@.service\nenable t@.service",
                "t@one.service",
                false,
                Some("one"),
                PresetLookup::Unknown {
                    file: file.into(),
                    line: 1,
                },
            ),
            (
                "enable other@.service\nenable app.service",
                "t@one.service",
                false,
                Some("one"),
                NoRule,
            ),
            // A unit that is not an instance.
            (
                "enable app.service\ndisable *",
                "app.service",
                true,
                None,
                Rule(Enable),
            ),
            (
                "disable app*\nenable app.service",
                "app.service",
                true,
                None,
                Rule(Disable),
            ),
            (
                "enable other.service\nfrobnicate app.service",
                "app.service",
                true,
                None,
                PresetLookup::Unknown {
                    file: file.into(),
                    line: 2,
                },
            ),
            ("enable other.service", "app.service", true, None, NoRule),
        ];
        for (text, unit, own_file, default_instance, expected) in cases {
            let name = UnitName::try_from(unit.to_string()).unwrap();
            assert_eq!(
                preset_for(&name, *own_file, *default_instance, &rules(file, text)),
                *expected,
                "{unit} (own file {own_file}, default {default_instance:?}) under {text:?}"
            );
        }
    }

    /// T39 and § 7.4's named case: RHEL 9.8's and RHEL 10.2's getty rules,
    /// read from their preset files in file-name order. Neither instance
    /// has a unit file, and `getty@.service` has `DefaultInstance=tty1`:
    /// `tty1` is `enable` from `90-systemd.preset` on both, and `tty2` is
    /// `disable` on both. RHEL 9.8's `enable getty@tty1.service`, which sorts
    /// first, names an instance and is not read for it.
    #[test]
    fn getty_presets_on_both_releases() {
        let dir = "/usr/lib/systemd/system-preset";
        let mut rhel9 = vec![
            ("90-default.preset", "enable getty@tty1.service\n"),
            ("90-systemd.preset", "enable getty@.service\n"),
            ("99-default-disable.preset", "disable *\n"),
        ];
        let rhel10 = rhel9.split_off(1);
        for (release, files) in [
            ("RHEL 9.8", rhel9.iter().chain(&rhel10).collect::<Vec<_>>()),
            ("RHEL 10.2", rhel10.iter().collect()),
        ] {
            let mut exec = MockExecutor::new()
                .with_dir(dir, files.iter().rev().map(|(name, _)| *name).collect());
            for (name, text) in &files {
                exec = exec.with_file(&format!("{dir}/{name}"), text);
            }
            let (rules, failures) = read_preset_rules(&exec).expect("the presets are read");
            assert!(failures.is_empty(), "{release}: {failures:?}");
            for (unit, expected) in [
                ("getty@tty1.service", PresetDefault::Enable),
                ("getty@tty2.service", PresetDefault::Disable),
            ] {
                let name = UnitName::try_from(unit.to_owned()).unwrap();
                assert_eq!(
                    preset_for(&name, false, Some("tty1"), &rules),
                    PresetLookup::Rule(expected),
                    "{release}: {unit}"
                );
            }
        }
    }

    /// T39, the warning: one per line, naming the line, its file and every
    /// unit it decided, and saying their state was not compared.
    #[test]
    fn an_unknown_preset_line_is_one_warning_naming_its_units() {
        let file = "/etc/systemd/system-preset/10-t.preset";
        let units = vec![UnitName::try_from("t@one.service".to_owned()).unwrap()];
        let warning = unknown_preset_warning(file, 3, &units);
        assert_eq!(warning.inspector, "units");
        assert_eq!(warning.extra["file"], serde_json::json!(file));
        assert_eq!(warning.extra["line"], serde_json::json!(3));
        assert_eq!(warning.extra["units"], serde_json::json!(["t@one.service"]));
        assert!(
            warning.message.contains(file)
                && warning.message.contains("line 3")
                && warning.message.contains("t@one.service")
                && warning.message.contains("not compared with a preset"),
            "{}",
            warning.message
        );
    }

    /// A drop-in ranked above `/etc` that hides an `/etc` install drop-in
    /// is recorded beside it (§ 10.2 rule 5), in `read_install`'s order:
    /// a directory of the instance's own name before its template's, and
    /// of equal rank the first in search-path order. One that hides
    /// nothing, or hides an `/etc` drop-in that is no install file, is not
    /// recorded, and the `/etc` list itself is unchanged.
    #[test]
    fn a_drop_in_ranked_above_etc_hiding_an_install_drop_in_is_recorded() {
        const INSTALL: &str = "[Install]\nWantedBy=graphical.target\n";
        const OTHER: &str = "[Service]\nCPUQuota=20%\n";
        const OWN_D: &str = "t@a.service.d";
        const TEMPLATE_D: &str = "t@.service.d";
        const CONTROL: &str = "/etc/systemd/system.control";
        const RUN_CONTROL: &str = "/run/systemd/system.control";
        let at = |base: &str, dir: &str, file: &str| format!("{base}/{dir}/{file}");
        let etc_own = at(ETC_UNIT_DIR, OWN_D, "50-x.conf");
        let etc_template = at(ETC_UNIT_DIR, TEMPLATE_D, "50-x.conf");
        let run_own = at(RUN_CONTROL, OWN_D, "50-x.conf");
        let run_template = at(RUN_CONTROL, TEMPLATE_D, "50-x.conf");
        let control_own = at(CONTROL, OWN_D, "50-x.conf");
        let control_template = at(CONTROL, TEMPLATE_D, "50-x.conf");
        let run_other = at(RUN_CONTROL, OWN_D, "50-y.conf");
        // (label, files and their content, the `/etc` install drop-ins,
        // the hiding drop-ins)
        type Case<'a> = (&'a str, Vec<(&'a str, &'a str)>, Vec<&'a str>, Vec<&'a str>);
        let cases: Vec<Case> = vec![
            (
                "the same .d, the same name",
                vec![(&etc_own, INSTALL), (&run_own, OTHER)],
                vec![&etc_own],
                vec![&run_own],
            ),
            (
                "another name hides nothing",
                vec![(&etc_own, INSTALL), (&run_other, OTHER)],
                vec![&etc_own],
                vec![],
            ),
            (
                "the hidden file holds no [Install]",
                vec![(&etc_own, OTHER), (&run_own, OTHER)],
                vec![],
                vec![],
            ),
            (
                "the instance's .d hides the template's",
                vec![(&etc_template, INSTALL), (&run_own, OTHER)],
                vec![&etc_template],
                vec![&run_own],
            ),
            (
                "the template's .d does not hide the instance's",
                vec![(&etc_own, INSTALL), (&run_template, OTHER)],
                vec![&etc_own],
                vec![],
            ),
            (
                "of equal rank, the first in search-path order",
                vec![
                    (&etc_own, INSTALL),
                    (&control_own, OTHER),
                    (&run_own, OTHER),
                ],
                vec![&etc_own],
                vec![&control_own],
            ),
            (
                "the directory's rank before search-path order",
                vec![
                    (&etc_template, INSTALL),
                    (&control_template, OTHER),
                    (&run_own, OTHER),
                ],
                vec![&etc_template],
                vec![&run_own],
            ),
        ];
        let name = UnitName::try_from("t@a.service".to_owned()).unwrap();
        let template = UnitName::try_from("t@.service".to_owned()).unwrap();
        let fragment = UnitFragment::Vendor {
            file: template.clone(),
            owning_package: None,
        };
        for (label, files, install, hiding) in cases {
            let mut dirs: BTreeMap<String, Vec<&str>> = BTreeMap::new();
            let mut exec = MockExecutor::new().with_file(
                &format!("{VENDOR_UNIT_DIR}/{}", template.as_str()),
                "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n",
            );
            for (path, content) in &files {
                let (dir, file) = path.rsplit_once('/').unwrap();
                dirs.entry(dir.to_owned()).or_default().push(file);
                exec = exec.with_file(path, content);
            }
            for (dir, entries) in dirs {
                exec = exec.with_dir(&dir, entries);
            }
            let evidence = install_evidence(&exec, &name, &fragment).unwrap();
            assert_eq!(evidence.install_drop_ins, install, "{label}");
            assert_eq!(evidence.hiding_drop_ins, hiding, "{label}");
            let mut row: Vec<&str> = install.iter().chain(&hiding).copied().collect();
            row.sort_unstable();
            assert_eq!(evidence.row_install_drop_ins(), row, "{label}");
        }
    }
}
