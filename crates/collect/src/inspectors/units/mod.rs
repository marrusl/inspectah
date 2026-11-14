use inspectah_core::baseline::STOCK_STATE;
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
    AdminStorageFragment, AdvisoryUnit, Classified, MountUnitFinding, OrdinaryKind, PresetDefault,
    StorageKind, SystemdDropIn, UNIT_TYPE_SUFFIXES, UnitFragment, UnitInventoryEntry, UnitKind,
    UnitName, UnitSection, UnitState, UnitStateChange, UnitSubject,
};
use inspectah_core::types::warnings::{Warning, WarningSeverity};
use std::collections::HashSet;
use std::path::Path;

mod resolve;

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

/// The name gate (§ 7.4 of the units spec): the name and state of a listed
/// unit the section records, or `None` for one it does not. A state that is
/// not durable is dropped or warned about by [`parse_unit_state`]. The name
/// must parse as a [`UnitName`] and not be a D-Bus activation alias. A
/// template is recorded only when masked, since its mask masks every
/// instance; enabling a template means nothing without an instance. An
/// instance is not recorded: reading its install state is a later step.
fn gate(unit: &UnitFileEntry, warnings: &mut Vec<Warning>) -> Option<(UnitName, UnitState)> {
    let state = match parse_unit_state(&unit.unit, &unit.state) {
        ParsedUnitState::Durable(state) => state,
        ParsedUnitState::SilentDrop => return None,
        ParsedUnitState::Warn(warning) => {
            warnings.push(warning);
            return None;
        }
    };
    let name = proved_unit_name(&unit.unit, &unit.state, warnings)?;
    if name.as_str().starts_with(DBUS_ACTIVATION_ALIAS_PREFIX) || name.instance().is_some() {
        return None;
    }
    if name.is_template() && state != UnitState::Masked {
        return None;
    }
    Some((name, state))
}

/// Result of parsing a raw systemd unit state string into the typed contract.
enum ParsedUnitState {
    /// Durable administrator intent: produces a `UnitStateChange`.
    Durable(UnitState),
    /// Known-inert packaging artifact — silently dropped, no warning.
    SilentDrop,
    /// Runtime/linked/bad/unknown — emits a structured warning and drops.
    Warn(Warning),
}

/// Classify a raw systemd unit-file state into the typed contract.
fn parse_unit_state(unit: &str, raw_state: &str) -> ParsedUnitState {
    match raw_state {
        "enabled" => ParsedUnitState::Durable(UnitState::Enabled),
        "disabled" => ParsedUnitState::Durable(UnitState::Disabled),
        "masked" => ParsedUnitState::Durable(UnitState::Masked),
        // Packaging/dependency artifacts — no migration relevance.
        "static" | "alias" | "indirect" | "generated" => ParsedUnitState::SilentDrop,
        // Runtime/volatile states — lost on reboot, cannot be migrated.
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

/// Maps preset rule action strings to typed enum values.
fn resolve_preset_default(unit: &str, rules: &[PresetRule]) -> Option<PresetDefault> {
    for rule in rules {
        if glob_match(&rule.pattern, unit) {
            return match rule.action.as_str() {
                "enable" => Some(PresetDefault::Enable),
                "disable" => Some(PresetDefault::Disable),
                _ => None, // Unknown action → no preset rule found
            };
        }
    }
    None
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

/// A unit that may be a state change, depending on where its file is: a
/// mask, an enabled unit, or a unit disabled against an `enable` preset.
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
///
/// A timer whose file is under `/etc` is left to the scheduled section,
/// which enables every such timer today; a state change would give it a
/// second control. That section stops enabling it in a later phase.
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
    let file_under_etc = matches!(
        fragment,
        UnitFragment::Local { .. } | UnitFragment::Shadowed { .. }
    );
    if file_under_etc && candidate.name.kind() == UnitKind::Ordinary(OrdinaryKind::Timer) {
        return false;
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

/// Resolve each candidate's unit file, keep the state changes and the
/// enabled storage units, look up the owners, then classify: a carried unit
/// is a state change, an administrator's storage unit a mount finding. A
/// candidate that cannot be resolved or classified is reported through
/// `warnings`; a unit file that could not be read goes to `read_failures`.
fn build_state_changes(
    exec: &dyn Executor,
    candidates: Vec<Candidate>,
    warnings: &mut Vec<Warning>,
    read_failures: &mut Vec<String>,
) -> (Vec<UnitStateChange>, Vec<MountUnitFinding>) {
    let mut found = Vec::new();
    let mut fragments = Vec::new();
    for candidate in candidates {
        let warn =
            |detail: &str| service_warning(candidate.name.as_str(), &candidate.raw_state, detail);
        match resolve::resolve(exec, &candidate.name) {
            Ok(Resolved::File { fragment }) | Ok(Resolved::Masked { hidden: fragment }) => {
                let state_change = is_state_change(&candidate, &fragment);
                if state_change || is_enabled_storage(&candidate) {
                    fragments.push(fragment);
                    found.push((candidate, state_change));
                }
            }
            Ok(Resolved::AliasOf(target)) => warnings.push(warn(&format!(
                "unit is an alias of {}; its state is not carried on its own",
                target.as_str()
            ))),
            Ok(Resolved::NotFound) => warnings.push(warn(
                "no unit file under /etc/systemd/system or /usr/lib/systemd/system",
            )),
            // Today's warning for a `linked` unit.
            Err(ResolveError::Linked { .. }) => warnings.push(service_warning(
                candidate.name.as_str(),
                "linked",
                "linked unit requires manual handling",
            )),
            Err(ResolveError::Read { path, error }) => {
                read_failures.push(format!("{path}: {error}"));
            }
        }
    }

    populate_owning_packages(exec, &mut fragments);

    let mut state_changes = Vec::new();
    let mut mount_findings = Vec::new();
    for ((candidate, state_change), fragment) in found.into_iter().zip(fragments) {
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
                        nofail: false,
                        aggregate: None,
                    });
                }
                continue;
            }
        };
        match UnitStateChange::new(unit, candidate.state, candidate.default_state) {
            Ok(change) => state_changes.push(change),
            Err(e) => warnings.push(warn(&e.to_string())),
        }
    }
    (state_changes, mount_findings)
}

/// The mount findings for the storage files under `/etc/systemd/system`
/// that the candidates did not make a finding of (§ 6.5 of the units
/// spec): every regular, non-empty `.mount`, `.automount` or `.swap` file,
/// local or a full copy over a vendor file. Such a unit is not enabled (an
/// enabled one was a candidate) and not masked (its file is no mask), so
/// its finding records no state. `recorded` holds the names that already
/// have a finding.
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
    let Ok(entries) = exec.read_dir(Path::new(ETC_UNIT_DIR)) else {
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
        let warn = |detail: &str| Warning {
            inspector: "units".into(),
            message: format!("unit file {ETC_UNIT_DIR}/{} - {detail}", name.as_str()),
            severity: Some(WarningSeverity::Warning),
            extra: std::collections::HashMap::from([("unit".into(), serde_json::json!(name))]),
        };
        let unit = UnitSubject::new(name.clone(), fragment)
            .map_err(|e| e.to_string())
            .and_then(|subject| AdvisoryUnit::try_from(subject).map_err(|e| e.to_string()));
        match unit {
            Ok(unit) => findings.push(MountUnitFinding {
                unit,
                state_change: None,
                mount_point: None,
                nofail: false,
                aggregate: None,
            }),
            Err(detail) => warnings.push(warn(&detail)),
        }
    }
    findings
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
    /// Parsed from systemctl output but not used — preset comparison
    /// uses the actual preset files for first-match-wins semantics.
    _preset: String,
}

/// A parsed preset rule from a `.preset` file.
#[derive(Debug)]
struct PresetRule {
    action: String,
    pattern: String,
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

        // 3. Read preset files — try both dirs; if neither readable, degrade
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
                        }),
                        warnings,
                        redaction_hints: Vec::new(),
                    }),
                    reason,
                });
            }
        };

        // 4. Every unit past the name gate is in the inventory once, with its
        //    preset. A unit that may be a state change, depending on where
        //    its file is, is a candidate; the rest have no evidence of an
        //    administrator's change and only the inventory records them.
        let mut candidates = Vec::new();
        let mut inventory = Vec::new();
        let mut inventoried = HashSet::new();
        let mut warnings = Vec::new();

        for unit in &units {
            let Some((name, current_state)) = gate(unit, &mut warnings) else {
                continue;
            };
            let preset = resolve_preset_default(&unit.unit, &preset_rules);

            // A name systemctl lists again keeps its first entry.
            if inventoried.insert(name.clone()) {
                inventory.push(UnitInventoryEntry {
                    unit: name.clone(),
                    state: current_state,
                    preset,
                });
            }

            let candidate = match current_state {
                UnitState::Masked | UnitState::Enabled => true,
                UnitState::Disabled => preset == Some(PresetDefault::Enable),
            };
            if candidate {
                candidates.push(Candidate {
                    name,
                    raw_state: unit.state.clone(),
                    state: current_state,
                    default_state: preset,
                });
            }
        }

        // 4b. Resolve each candidate's unit file and owning package, then
        //     build the state changes.
        let mut unit_file_read_failures = Vec::new();
        let (state_changes, mut mount_findings) = build_state_changes(
            exec,
            candidates,
            &mut warnings,
            &mut unit_file_read_failures,
        );

        // 4c. The administrator's other storage files, then what each
        //     finding mounts, read from its unit file (§ 6.5).
        let recorded: HashSet<UnitName> = mount_findings
            .iter()
            .map(|f| f.unit.name().as_unit_name().clone())
            .collect();
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

        // 6. Build result
        let section = UnitSection {
            state_changes,
            inventory,
            drop_ins,
            mount_findings,
        };

        // Preset file read failures are correctness-bearing — a missing
        // preset file means we cannot determine the default state for
        // services whose rules were in that file.
        if !preset_read_failures.is_empty() {
            let reason = format!(
                "preset file read failures: {}",
                preset_read_failures.join("; ")
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

        // Drop-in read failures are correctness-bearing — a missed override
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
/// files from being read — the caller decides whether failures are fatal.
fn read_preset_rules(exec: &dyn Executor) -> Result<(Vec<PresetRule>, Vec<String>), String> {
    let usr_dir = Path::new("/usr/lib/systemd/system-preset");
    let etc_dir = Path::new("/etc/systemd/system-preset");

    let mut preset_files: Vec<(String, String)> = Vec::new(); // (filename, content)
    let mut read_failures: Vec<String> = Vec::new();

    let usr_ok = match exec.read_dir(usr_dir) {
        Ok(entries) => {
            for entry in &entries {
                if entry.ends_with(".preset") {
                    let path = usr_dir.join(entry);
                    match exec.read_file(&path) {
                        Ok(content) => preset_files.push((entry.clone(), content)),
                        Err(e) => {
                            read_failures.push(format!("{}: {e}", path.to_string_lossy()));
                        }
                    }
                }
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Directory doesn't exist — normal on systems without vendor presets.
            false
        }
        Err(e) => {
            // Directory exists but is unreadable (PermissionDenied, etc.) —
            // this is a trust gap: vendor presets may be hidden.
            read_failures.push(format!("{}: {e}", usr_dir.display()));
            false
        }
    };

    let etc_ok = match exec.read_dir(etc_dir) {
        Ok(entries) => {
            for entry in &entries {
                if entry.ends_with(".preset") {
                    let path = etc_dir.join(entry);
                    match exec.read_file(&path) {
                        Ok(content) => preset_files.push((entry.clone(), content)),
                        Err(e) => {
                            read_failures.push(format!("{}: {e}", path.to_string_lossy()));
                        }
                    }
                }
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Directory doesn't exist — normal on systems without admin presets.
            false
        }
        Err(e) => {
            // Directory exists but is unreadable (PermissionDenied, etc.) —
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

    // Sort by filename — numeric prefix ordering (e.g., 85-xxx before 90-xxx)
    preset_files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut rules = Vec::new();
    for (_filename, content) in &preset_files {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                rules.push(PresetRule {
                    action: parts[0].to_string(),
                    pattern: parts[1].to_string(),
                });
            }
        }
    }

    Ok((rules, read_failures))
}

// resolve_preset_default() is defined above, near the parse gate functions.

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
/// could be read: every entry has no preset. A name that does not parse is
/// reported through `warnings` and has no entry.
fn partial_inventory(
    units: &[UnitFileEntry],
    warnings: &mut Vec<Warning>,
) -> Vec<UnitInventoryEntry> {
    let mut inventory = Vec::new();
    let mut inventoried = HashSet::new();
    for u in units {
        let Some((name, state)) = gate(u, warnings) else {
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

/// Detect whether a unit has a full shadow or drop-in override.
///
/// - FullShadow: `/etc/systemd/system/{unit}` exists AND `/usr/lib/systemd/system/{unit}` exists
/// - DropIn: `/etc/systemd/system/{unit}.d/` exists
/// - None: neither condition is met
fn detect_shadow_type(exec: &dyn Executor, unit_name: &str) -> Option<ShadowType> {
    let etc_path = format!("/etc/systemd/system/{}", unit_name);
    let usr_path = format!("/usr/lib/systemd/system/{}", unit_name);
    let dropin_dir = format!("/etc/systemd/system/{}.d", unit_name);

    let etc_exists = exec.run("test", &["-f", &etc_path]).exit_code == 0;
    let usr_exists = exec.run("test", &["-f", &usr_path]).exit_code == 0;
    let dropin_exists = exec.run("test", &["-d", &dropin_dir]).exit_code == 0;

    if etc_exists && usr_exists {
        Some(ShadowType::FullShadow)
    } else if !etc_exists && dropin_exists {
        Some(ShadowType::DropIn)
    } else {
        None
    }
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
/// Read failures on listed `.conf` files are correctness-bearing — the
/// snapshot would look complete when an override was actually missed.
fn collect_drop_ins(exec: &dyn Executor) -> (Vec<SystemdDropIn>, Vec<RedactionHint>, Vec<String>) {
    let dropin_base = Path::new(ETC_UNIT_DIR);
    let mut drop_ins = Vec::new();
    let mut hints = Vec::new();
    let mut read_failures = Vec::new();

    let entries = match exec.read_dir(dropin_base) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Directory doesn't exist — no drop-ins to collect.
            return (drop_ins, hints, read_failures);
        }
        Err(e) => {
            // Directory exists but is unreadable (PermissionDenied, etc.) —
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

        let conf_files = match exec.read_dir(&dir_path) {
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
            let shadow_type = detect_shadow_type(exec, unit);
            let shadow_rationale = if shadow_type == Some(ShadowType::FullShadow) {
                Some(
                    "Full unit shadow — base image updates to this unit will be silently ignored"
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
/// - a timer: the scheduled section writes every timer file under `/etc`
///   until a later phase moves timers here;
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
        UnitKind::Storage(_) | UnitKind::Ordinary(OrdinaryKind::Timer) => return Ok(None),
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
    /// Whether `Options=` holds `nofail`. An automount has no `Options=`.
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
/// them: only in the section of the unit's own kind, the last assignment
/// winning (an empty one resets), comment lines skipped, and the key and
/// value trimmed, which drops a CRLF line end and trailing whitespace.
/// `Where=` belongs to mounts and automounts, `Options=` to mounts and
/// swaps. `What=` is never read: it can hold a credential, and the finding
/// has no field for it. A line continued with a trailing `\` is read as
/// it stands.
fn mount_settings(kind: StorageKind, content: &str) -> MountSettings {
    let own_section = storage_section(kind);
    let mut section = None;
    let mut where_value = None;
    let mut options = None;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
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
        match (key.trim(), kind) {
            ("Where", StorageKind::Mount | StorageKind::Automount) => {
                where_value = Some(value.trim());
            }
            ("Options", StorageKind::Mount | StorageKind::Swap) => options = Some(value.trim()),
            _ => {}
        }
    }
    MountSettings {
        mount_point: where_value
            .filter(|path| path.starts_with('/'))
            .map(str::to_owned),
        nofail: options.is_some_and(|o| o.split(',').any(|option| option.trim() == "nofail")),
    }
}

/// Fill in a finding's `mount_point` and `nofail` from the unit file
/// systemd loads for it: the `/etc` file for a local or shadowed unit, the
/// vendor file for an unowned vendor unit (masked or not). A mask with no
/// unit file has neither. A file that is not text, or is over the
/// executor's size cap, leaves both unset; its config entry, if it has
/// one, records that the content was not captured. Any other read error
/// goes to `read_failures`.
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
    finding.nofail = settings.nofail;
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
                "[Mount]\nOptions=rw, nofail ,noatime\n",
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
        ];
        for (label, kind, content, expected) in cases {
            assert_eq!(&mount_settings(*kind, content), expected, "{label}");
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
        MockExecutor::new()
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
            )
    }

    #[test]
    fn test_dropin_read_failure_triggers_degraded() {
        // Drop-in directory listed, .conf file listed, but file read fails (size cap)
        let exec = svc_base_mock()
            .with_dir("/etc/systemd/system", vec!["httpd.service.d"])
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
        let exec = svc_base_mock().with_dir("/etc/systemd/system", vec!["httpd.service.d"]);
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
        // Admin-priority presets may be hidden — must degrade.
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
        // /etc/systemd/system-preset doesn't exist (NotFound) — this is
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
            ("getty@tty1.service", "enabled", false),
            ("getty@tty1.service", "masked", false),
        ];
        for &(unit, state, kept) in cases {
            let entry = UnitFileEntry {
                unit: unit.into(),
                state: state.into(),
                _preset: String::new(),
            };
            let mut warnings = Vec::new();
            let gated = gate(&entry, &mut warnings);
            assert_eq!(gated.is_some(), kept, "{unit} {state}");
            assert!(warnings.is_empty(), "{unit} {state}: {warnings:?}");
        }
    }

    #[test]
    fn test_dbus_alias_not_captured_no_operator_evidence() {
        // D-Bus aliases in a different state than their preset are NOT
        // captured — the operator-intent model excludes them structurally
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
            .with_dir("/etc/systemd/system", vec![])
            .with_file("/usr/lib/systemd/system/httpd.service", "[Unit]\n");

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
            // D-Bus aliases must NOT appear — no operator evidence
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
            // sshd matches preset — should not be in state_changes
            assert!(
                !svc.state_changes
                    .iter()
                    .any(|sc| sc.unit().name().as_str() == "sshd.service"),
                "sshd.service matches preset, must not be in state_changes"
            );
            // cups is masked — operator intent, must be captured
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
            .with_file("/usr/lib/systemd/system/firewalld.service", "[Unit]\n");

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
        // "disable" and current state is "disabled" — no divergence, no
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
        // is just "package installation vs. image presets" — no evidence
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
                // Note: no preset rule for some-obscure.service — only
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
            // default_state="disabled", and current state is also "disabled"
            // — no divergence. If they were different, the whitelist model
            // captures it because the preset is definitive. This test verifies
            // the matching path.
            //
            // In this case: preset says "disabled" (via wildcard), state is
            // "disabled" — match, not captured. Good.
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
            .with_dir("/etc/systemd/system", vec!["httpd.service.d"])
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
            .with_dir("/etc/systemd/system", vec!["httpd.service.d"])
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
            .with_command(
                "test -f /etc/systemd/system/httpd.service",
                ExecResult {
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "test -f /usr/lib/systemd/system/httpd.service",
                ExecResult {
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "test -d /etc/systemd/system/httpd.service.d",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let shadow_type = detect_shadow_type(&exec, "httpd.service");
        assert_eq!(shadow_type, Some(ShadowType::FullShadow));
    }

    #[test]
    fn test_detect_shadow_type_drop_in() {
        // Only drop-in directory exists → DropIn
        let exec = MockExecutor::new()
            .with_command(
                "test -f /etc/systemd/system/httpd.service",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            )
            .with_command(
                "test -f /usr/lib/systemd/system/httpd.service",
                ExecResult {
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "test -d /etc/systemd/system/httpd.service.d",
                ExecResult {
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let shadow_type = detect_shadow_type(&exec, "httpd.service");
        assert_eq!(shadow_type, Some(ShadowType::DropIn));
    }

    #[test]
    fn test_detect_shadow_type_none() {
        // Neither full shadow nor drop-in exists → None
        let exec = MockExecutor::new()
            .with_command(
                "test -f /etc/systemd/system/httpd.service",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            )
            .with_command(
                "test -f /usr/lib/systemd/system/httpd.service",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            )
            .with_command(
                "test -d /etc/systemd/system/httpd.service.d",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let shadow_type = detect_shadow_type(&exec, "httpd.service");
        assert_eq!(shadow_type, None);
    }

    #[test]
    fn test_detect_shadow_type_custom_service_not_drop_in() {
        // A service defined only in /etc (not in /usr) is a custom service,
        // not a drop-in override, even if it has a .d/ directory
        let exec = MockExecutor::new()
            .with_command(
                "test -f /etc/systemd/system/custom.service",
                ExecResult {
                    exit_code: 0, // exists in /etc
                    ..Default::default()
                },
            )
            .with_command(
                "test -f /usr/lib/systemd/system/custom.service",
                ExecResult {
                    exit_code: 1, // NOT in /usr
                    ..Default::default()
                },
            )
            .with_command(
                "test -d /etc/systemd/system/custom.service.d",
                ExecResult {
                    exit_code: 0, // has .d/ dir
                    ..Default::default()
                },
            );

        let shadow_type = detect_shadow_type(&exec, "custom.service");
        assert_eq!(shadow_type, None);
    }

    #[test]
    fn test_full_shadow_carries_rationale() {
        // Full shadow should populate shadow_rationale field
        let exec = svc_base_mock()
            .with_dir("/etc/systemd/system", vec!["httpd.service.d"])
            .with_dir("/etc/systemd/system/httpd.service.d", vec!["override.conf"])
            .with_file(
                "/etc/systemd/system/httpd.service.d/override.conf",
                "[Service]\nRestart=always\n",
            )
            .with_command(
                "test -f /etc/systemd/system/httpd.service",
                ExecResult {
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "test -f /usr/lib/systemd/system/httpd.service",
                ExecResult {
                    exit_code: 0,
                    ..Default::default()
                },
            )
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
                    "Full unit shadow — base image updates to this unit will be silently ignored"
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
            .with_dir("/etc/systemd/system", vec!["httpd.service.d"])
            .with_dir("/etc/systemd/system/httpd.service.d", vec!["override.conf"])
            .with_file(
                "/etc/systemd/system/httpd.service.d/override.conf",
                "[Service]\nRestart=always\n",
            )
            .with_command(
                "test -f /etc/systemd/system/httpd.service",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            )
            .with_command(
                "test -f /usr/lib/systemd/system/httpd.service",
                ExecResult {
                    exit_code: 0,
                    ..Default::default()
                },
            )
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
            .with_dir("/etc/systemd/system", vec!["sshd.service"])
            .with_file(
                "/etc/systemd/system/sshd.service",
                "[Unit]\nDescription=Custom sshd\n[Service]\nExecStart=/usr/sbin/sshd -D\n",
            )
            // Vendor counterpart exists
            .with_file("/usr/lib/systemd/system/sshd.service", "[Unit]\n")
            .with_command(
                "test -f /usr/lib/systemd/system/sshd.service",
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
            // sshd matches preset — must NOT be in state_changes
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
        // not silently produce an empty content entry. The unit is disabled
        // as its preset says, so it is no candidate and the shadow scan is
        // the first to read its file (an enabled unit's file is read when it
        // is resolved, which degrades the scan with its own reason).
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sshd.service                               disabled        disabled\n\
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
            .with_dir("/etc/systemd/system", vec!["sshd.service"])
            // Shadow file exists but is unreadable
            .with_file_error(
                "/etc/systemd/system/sshd.service",
                std::io::ErrorKind::PermissionDenied,
            )
            // Vendor counterpart exists
            .with_file("/usr/lib/systemd/system/sshd.service", "[Unit]\n");

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

    /// A storage candidate, enabled against a disable preset.
    fn storage_candidate(unit: &str) -> Candidate {
        Candidate {
            name: UnitName::try_from(unit.to_owned()).unwrap(),
            raw_state: "enabled".into(),
            state: UnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
        }
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
        let vendor =
            || MockExecutor::new().with_file("/usr/lib/systemd/system/tmp.mount", "[Mount]\n");

        let owned = vendor().with_command(query, rpm("systemd\n", 0));
        let (mut warnings, mut failures) = (Vec::new(), Vec::new());
        let (changes, findings) = build_state_changes(
            &owned,
            vec![storage_candidate("tmp.mount")],
            &mut warnings,
            &mut failures,
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
            let (changes, findings) = build_state_changes(
                &unowned,
                vec![storage_candidate("tmp.mount")],
                &mut warnings,
                &mut failures,
            );
            assert!(changes.is_empty(), "{changes:?}");
            assert!(warnings.is_empty(), "{warnings:?}");
            assert_eq!(findings.len(), 1, "{findings:?}");
            assert_eq!(findings[0].unit.name().as_unit_name().as_str(), "tmp.mount");
        }
    }
}
