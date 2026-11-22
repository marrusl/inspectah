use inspectah_core::types::symlink_allowlist::{crosses_tree_boundary, is_allowlisted};
use inspectah_core::types::{AdvisoryType, FindingKind};
pub(crate) mod admit;
mod audit_rules;
pub mod classify;
mod ghost;
pub mod rpmva;
pub mod walk;

use classify::classify_config_path;
use inspectah_core::traits::executor::{Executor, FileKind};
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput, RpmState,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::redaction::RedactionHint;
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::warnings::Warning;
use std::collections::HashSet;
use std::path::Path;
use walk::{
    dhcp_connection_paths, is_dev_artifact, is_excluded_unowned, walk_etc_recursive,
    walk_etc_with_reasons,
};

use super::modernization;

/// Maximum file size to read (256 KiB). A larger file is collected as a
/// `ContentNotCaptured` advisory and marks the section degraded, to prevent
/// memory bloat.
pub(crate) const MAX_CONFIG_FILE_SIZE: usize = 256 * 1024;

/// Config inspector: RPM-owned modified, unowned /etc files, orphaned configs.
///
/// For package-mode systems: uses rpm -Va output (from RpmState) to find
/// modified files, walks /etc to find unowned files, and cross-references
/// dnf history for orphan detection.
///
/// For ostree/bootc systems: diffs /usr/etc against /etc overlays.
pub struct ConfigInspector;

impl ConfigInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ConfigInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector for ConfigInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Config
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        use inspectah_core::types::progress::{MetricKind, ProgressEvent, StepId, StepOutcome};

        let rpm_state = match ctx.rpm_state {
            None => {
                return Err(InspectorError::Failed {
                    reason: "RPM prerequisite unavailable".into(),
                });
            }
            Some(state) => state,
        };

        let exec = ctx.executor;
        let inspector_id = InspectorId::Config;
        let mut warnings: Vec<Warning> = Vec::new();
        let hints: Vec<RedactionHint> = Vec::new();
        let mut degraded_reasons: Vec<String> = Vec::new();

        // Branch for ostree/bootc systems
        if is_ostree_system(ctx.source_system) {
            let section = run_ostree_config(exec, rpm_state, &mut warnings);
            return Ok(InspectorOutput {
                section: SectionData::Config(section),
                warnings,
                redaction_hints: hints,
            });
        }

        let mut section = ConfigSection::default();

        // Early exit if /etc doesn't exist (use read_dir since /etc is a directory)
        if exec.read_dir(Path::new("/etc")).is_err() && !exec.file_exists(Path::new("/etc")) {
            return Ok(InspectorOutput {
                section: SectionData::Config(section),
                warnings,
                redaction_hints: hints,
            });
        }

        // Detect crypto policy
        detect_crypto_policy(exec, &mut warnings);

        // Build DHCP exclusion set
        let dhcp_paths: HashSet<String> = dhcp_connection_paths(exec).into_iter().collect();

        // 1) RPM-owned modified files (from rpm -Va / verification_results).
        //    Files with only metadata changes (mtime, mode, etc.) are
        //    classified as RpmOwnedDefault and skipped — defaults don't
        //    belong in the snapshot, same as base-image packages.
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::ApplyingRpmVerification,
        });
        let mut rpm_va_paths: HashSet<String> = HashSet::new();
        let mut va_entries: Vec<(&str, &str, Option<&str>)> = Vec::new();
        // Without a completed `rpm -Va` no row is known; this branch then
        // collects nothing, and the walk below still skips owned paths.
        for entry in rpm_state.verification_results().unwrap_or_default() {
            if entry.path.starts_with("/etc") {
                rpm_va_paths.insert(entry.path.clone());
                va_entries.push((&entry.path, &entry.flags, entry.package.as_deref()));
            }
        }

        // Sort for deterministic output
        va_entries.sort_by(|a, b| a.0.cmp(b.0));

        for (path, flags, package) in &va_entries {
            if !exec.file_exists(Path::new(path)) {
                continue;
            }
            let located = match admit::locate(exec, path) {
                Ok(located) => located,
                Err(admit::Refusal::NotUnderEtc) => continue,
                Err(refusal) => {
                    warnings.push(refused_path_warning(path, &refusal));
                    continue;
                }
            };
            if located
                .spellings()
                .any(|p| dhcp_paths.contains(p) || is_selinux_fcontext_store_file(p))
            {
                continue;
            }
            if is_authselect_pam_link(exec, path, flags) {
                continue;
            }

            let kind = if is_rpm_owned_dev_null_link(exec, path, flags) {
                ConfigFileKind::RpmOwnedModified
            } else {
                classify_rpm_va_kind(flags)
            };

            // Default/unmodified configs don't belong in the snapshot.
            // Only modified (content-changed) RPM configs are migration-relevant.
            // Decided before any read: a row rpm reported only for its
            // metadata is never opened.
            if kind == ConfigFileKind::RpmOwnedDefault {
                continue;
            }

            let (content, disposition) = match capture_config(exec, path, &mut degraded_reasons) {
                Ok(captured) => captured,
                Err(
                    admit::Unreadable::LinkLeavesEtc(target)
                    | admit::Unreadable::LinkToWithheld(target),
                ) => {
                    warnings.push(withheld_link_warning(path, &target));
                    continue;
                }
                Err(admit::Unreadable::Special | admit::Unreadable::NotAFile) => continue,
            };

            section.files.push(ConfigFileEntry {
                path: path.to_string(),
                kind,
                category: classify_config_path(path),
                content,
                rpm_va_flags: Some(flags.to_string()),
                package: package.map(|p| p.to_string()),
                diff_against_rpm: None, // Phase 3
                disposition,
                ..Default::default()
            });
        }
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::ApplyingRpmVerification,
            outcome: StepOutcome::Complete,
        });

        // 1b) `%ghost` files: owned, so the walk below skips them, and
        //     their content never verified, so rpm -Va above reports at
        //     most a metadata change, which it drops as unmodified.
        collect_ghost_configs(
            exec,
            rpm_state,
            &dhcp_paths,
            &mut section,
            &mut degraded_reasons,
            &mut warnings,
        );
        authselect_edit_advisories(
            exec,
            rpm_state,
            &mut section,
            &mut degraded_reasons,
            &mut warnings,
        );

        // 2) Unowned files: in /etc but not RPM-owned
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::WalkingFilesystem,
        });
        match walk_etc_with_reasons(exec, "/etc", &mut degraded_reasons) {
            Ok(files) => {
                for rel_path in files {
                    let abs_path = format!("/etc/{rel_path}");

                    // Under a directory link, the file it names is
                    // outside /etc or withheld: not collected, and an
                    // unowned path is not named by a warning. Every
                    // exclusion below applies to both spellings.
                    let Ok(located) = admit::locate(exec, &abs_path) else {
                        continue;
                    };
                    // Skip already captured rpm -Va paths, and RPM-owned
                    // ones (not modified), by the walked spelling only.
                    // An alias whose physical file another branch
                    // carries, or a package owns, is carried as well: the
                    // directory link is never carried, and the
                    // application reads the file by the alias.
                    if rpm_va_paths.contains(&located.path)
                        || rpm_state.is_rpm_owned(Path::new(&located.path))
                    {
                        continue;
                    }
                    // Also resolve symlinks: a file discovered at
                    // /etc/ssl/certs/ca-bundle.crt may be a symlink to
                    // /etc/pki/tls/certs/ca-bundle.crt — RPM owns the
                    // target path, not the symlink path.
                    if is_rpm_owned_via_symlink(exec, rpm_state, &abs_path) {
                        continue;
                    }
                    // Never open a FIFO, socket or device, before any
                    // check below reads the file.
                    if admit::readable(exec, &abs_path) == Err(admit::Unreadable::Special) {
                        continue;
                    }
                    // Skip excluded unowned files, dev artifacts and
                    // DHCP connections.
                    if located.spellings().any(|p| {
                        is_excluded_unowned(exec, p) || is_dev_artifact(p) || dhcp_paths.contains(p)
                    }) {
                        continue;
                    }

                    // Cross-tree symlink detection: advisories for symlinks
                    // that cross /etc→/var or /etc→/usr boundaries
                    if exec.read_link(Path::new(&abs_path)).is_ok() {
                        match exec.resolve_final_target(Path::new(&abs_path)) {
                            Err(_) => {
                                section.files.push(ConfigFileEntry {
                                    path: abs_path,
                                    kind: ConfigFileKind::Unowned,
                                    category: classify_config_path(&format!("/etc/{rel_path}")),
                                    content: String::new(),
                                    disposition: FindingKind::advisory(
                                        AdvisoryType::CrossTreeSymlink,
                                        "Broken cross-tree symlink",
                                    ),
                                    ..Default::default()
                                });
                                continue;
                            }
                            Ok(resolved) => {
                                let resolved_str = resolved.to_string_lossy();
                                if admit::is_authselect_profile_link(&abs_path, &resolved_str) {
                                    // Collected below with the content
                                    // it resolves to.
                                } else if is_allowlisted(&abs_path, &resolved_str) {
                                    continue;
                                } else if let Some(rationale) =
                                    crosses_tree_boundary(&abs_path, &resolved_str)
                                {
                                    section.files.push(ConfigFileEntry {
                                        path: abs_path,
                                        kind: ConfigFileKind::Unowned,
                                        category: classify_config_path(&format!("/etc/{rel_path}")),
                                        content: String::new(),
                                        disposition: FindingKind::advisory(
                                            AdvisoryType::CrossTreeSymlink,
                                            rationale,
                                        ),
                                        ..Default::default()
                                    });
                                    continue;
                                }
                            }
                        }
                    }

                    let (content, disposition) =
                        match capture_config(exec, &abs_path, &mut degraded_reasons) {
                            Ok(captured) => captured,
                            Err(admit::Unreadable::LinkLeavesEtc(_)) => (
                                String::new(),
                                FindingKind::advisory(
                                    AdvisoryType::CrossTreeSymlink,
                                    LINK_LEAVES_ETC_RATIONALE,
                                ),
                            ),
                            // The target is collected, or excluded, under
                            // its own path.
                            Err(admit::Unreadable::LinkToWithheld(_)) => continue,
                            Err(admit::Unreadable::Special | admit::Unreadable::NotAFile) => {
                                continue;
                            }
                        };

                    section.files.push(ConfigFileEntry {
                        path: abs_path,
                        kind: ConfigFileKind::Unowned,
                        category: classify_config_path(&format!("/etc/{rel_path}")),
                        content,
                        diff_against_rpm: None,
                        disposition,
                        ..Default::default()
                    });
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                degraded_reasons.push(format!("permission denied during /etc walk: {e}"));
            }
            Err(_) => {
                // /etc walk failed for other reasons — continue with what we have
            }
        }
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::WalkingFilesystem,
            outcome: StepOutcome::Complete,
        });

        // 3) Orphaned configs from removed packages
        progress.emit(ProgressEvent::StepStarted {
            inspector: inspector_id,
            step: StepId::ClassifyingConfigs,
        });
        detect_orphaned_configs(
            exec,
            rpm_state,
            &rpm_va_paths,
            &dhcp_paths,
            &mut section,
            &mut degraded_reasons,
        );

        // 4) Modernization advisories: upgrade existing entries or add new ones
        if let Some(major) = modernization::os_major_version(ctx.source_system) {
            for (path, disposition) in modernization::check_modernization_patterns(exec, major) {
                if let Some(existing) = section.files.iter_mut().find(|f| f.path == path) {
                    existing.disposition = with_modernization(&existing.disposition, disposition);
                } else {
                    section.files.push(ConfigFileEntry {
                        path: path.clone(),
                        kind: ConfigFileKind::Unowned,
                        category: classify_config_path(&path),
                        content: String::new(),
                        disposition,
                        ..Default::default()
                    });
                }
            }
        }

        // Sort all files by path for deterministic output
        section.files.sort_by(|a, b| a.path.cmp(&b.path));

        // Emit count of all config files found (modified + unowned + orphaned)
        progress.emit(ProgressEvent::Metric {
            inspector: inspector_id,
            kind: MetricKind::ConfigsModified,
            value: section.files.len(),
        });
        progress.emit(ProgressEvent::StepFinished {
            inspector: inspector_id,
            step: StepId::ClassifyingConfigs,
            outcome: StepOutcome::Complete,
        });

        // Check for degraded state
        if !degraded_reasons.is_empty() {
            let reason = format!("config inspector degraded: {}", degraded_reasons.join("; "));
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::Config(section),
                    warnings,
                    redaction_hints: hints,
                }),
                reason,
            });
        }

        Ok(InspectorOutput {
            section: SectionData::Config(section),
            warnings,
            redaction_hints: hints,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Why the ghost-file query was not answered, for the degraded reason.
const GHOST_QUERY_FAILED: &str =
    "rpm file-flags query failed, so edits to %ghost files under /etc were not checked";

/// What the degraded reason says when the query's output is not the
/// format it asked for; the parse error follows.
const GHOST_QUERY_UNREADABLE: &str = "rpm file-flags query output could not be read, so edits to \
     %ghost files under /etc were not checked";

/// The scan warning for a ghost path holding a control character.
const GHOST_PATH_REFUSED: &str = "is a %ghost file under /etc whose name holds a control \
     character; it was not read and is not carried";

/// The scan warning for a ghost file withheld as a credential; the path
/// comes first.
const GHOST_CREDENTIAL_WITHHELD: &str = "holds key material, so it was withheld: its content is \
     not in the scan and the image does not carry it. Provision it on each deployed host.";

/// The `%ghost` branch (`ghost.rs`): the ghost files under `/etc` that
/// are general configuration no other section owns, collected as modified
/// owned configs with their content.
///
/// A ghost file's package ships no content, so whatever the file holds
/// was written on the host: by an admin, or by a tool such as authselect.
/// The kind is `RpmOwnedModified` with no `rpm_va_flags`, since rpm did
/// not verify its content.
///
/// Skipped, in order: a path an earlier branch already collected (an
/// `rpm -Va` row with a content flag). A path `rpm -Va` reported only
/// for its metadata is not skipped: that branch drops such a row as
/// unmodified, and for a ghost file the metadata is all rpm checks, so
/// the row says nothing about the content. Then a path that is
/// not a regular file by `lstat` ([`Executor::file_kind`]): absent, a
/// symlink (its state is the link, not content), a directory (`%ghost
/// %dir`), or a FIFO, socket or device, which is never opened because
/// opening a FIFO with no writer blocks. Then a path [`admit::locate`]
/// refuses, a scan warning: a directory above it is a link that does not
/// resolve, or names a file outside `/etc` or one the section withholds.
/// Then every path, in its normalized spelling,
/// [`ghost::classify_ghost`] does not call [`ghost::GhostClass::Config`]:
/// a file another section records (fstab, crypttab, the `*.repo` files
/// the repos section reads, the SELinux config, the account databases,
/// the locale), a machine-state member, a file the host generates, or a
/// keymap file the locale item decides. A credential by path is never
/// opened and is withheld with a scan warning naming it.
/// `/etc/audit/audit.rules` is skipped when the audit loader units are
/// exactly as packaged, verified by a completed `rpm -Va`, so `augenrules`
/// writes it, and otherwise carried with a scan warning
/// ([`audit_rules::source`]). Then the unowned walk's
/// exclusions that need the host (`is_excluded_unowned`'s unit-link
/// test, DHCP connections), on both spellings of a path under a
/// directory link, which is collected at the spelling it was found by.
/// Last, once read, a file whose content is a keytab or a whole-file
/// private key (`ghost::is_credential_content`) is dropped before the
/// content is kept, with the same warning.
///
/// When the machine-state class lands, the member skip goes and each
/// surviving candidate goes through `ConfigCollector::admit` with
/// `kind: RpmOwnedModified`, `package: Some(..)` and `rpm_va_flags: None`
/// in place of the `section.files.push` here, so this branch and every
/// other one share the class's single admission point.
///
/// A ghost path holding a control character is not read and becomes a
/// scan warning ([`ghost::GhostQuery::refused`]). Output that is not the
/// query's record format degrades the section, and nothing in it is
/// collected.
///
/// No `/etc` path is RPM-owned when `owned_paths` is empty, so the query
/// is not run then.
fn collect_ghost_configs(
    exec: &dyn Executor,
    rpm_state: &RpmState,
    dhcp_paths: &HashSet<String>,
    section: &mut ConfigSection,
    degraded_reasons: &mut Vec<String>,
    warnings: &mut Vec<Warning>,
) {
    if rpm_state.owned_paths().is_empty() {
        return;
    }
    let result = exec.run(
        "rpm",
        &["-qa", "--queryformat", ghost::RPM_FILE_FLAGS_FORMAT],
    );
    if !result.success() {
        degraded_reasons.push(GHOST_QUERY_FAILED.to_string());
        return;
    }
    let query = match ghost::parse_ghost_files(&result.stdout) {
        Ok(query) => query,
        Err(error) => {
            degraded_reasons.push(format!("{GHOST_QUERY_UNREADABLE} ({error})"));
            return;
        }
    };
    for path in &query.refused {
        warnings.push(Warning {
            inspector: "config".into(),
            message: format!("{path:?} {GHOST_PATH_REFUSED}"),
            ..Default::default()
        });
    }
    let collected: HashSet<String> = section.files.iter().map(|f| f.path.clone()).collect();
    for ghost in query.files {
        let requested = ghost.path.as_str();
        if collected.contains(requested)
            || !matches!(exec.file_kind(Path::new(requested)), Ok(FileKind::Regular))
        {
            continue;
        }
        let located = match admit::locate(exec, requested) {
            Ok(located) => located,
            Err(refusal) => {
                warnings.push(refused_path_warning(requested, &refusal));
                continue;
            }
        };
        let path = located.path.as_str();
        match ghost::classify_ghost(path) {
            ghost::GhostClass::Config => {}
            ghost::GhostClass::AuditRules => match audit_rules::source(
                exec,
                &query.packaged_units,
                rpm_state.verification_results(),
            ) {
                audit_rules::Source::Augenrules => continue,
                audit_rules::Source::Carried(reason) => {
                    warnings.push(Warning {
                        inspector: "config".into(),
                        message: format!("{requested} {AUDIT_RULES_UNCERTAIN} ({reason})."),
                        ..Default::default()
                    });
                }
            },
            ghost::GhostClass::Credential => {
                warnings.push(ghost_credential_warning(requested));
                continue;
            }
            ghost::GhostClass::OwnedBy(_)
            | ghost::GhostClass::MachineState
            | ghost::GhostClass::Generated
            | ghost::GhostClass::Deferred => continue,
        }
        if located
            .spellings()
            .any(|p| is_excluded_unowned(exec, p) || dhcp_paths.contains(p))
        {
            continue;
        }

        let Ok((content, disposition)) = capture_config(exec, path, degraded_reasons) else {
            continue;
        };
        if ghost::is_credential_content(&content) {
            warnings.push(ghost_credential_warning(path));
            continue;
        }
        section.files.push(ConfigFileEntry {
            path: ghost.path.clone(),
            kind: ConfigFileKind::RpmOwnedModified,
            category: classify_config_path(path),
            content,
            rpm_va_flags: None,
            package: Some(ghost.package),
            diff_against_rpm: None,
            disposition,
            ..Default::default()
        });
    }
}

/// The start of the scan warning for a packaged path refused by
/// [`admit::locate`]; the path comes first and the reason follows.
const PATH_REFUSED: &str = "was not read and is not carried:";

/// Why a path under a directory link was refused, after the file it
/// names.
const ALIAS_TO_WITHHELD: &str = "is key material, host identity, a generated file, or a file \
     another section records, so it is left out under either name";

/// The scan warning for a modified packaged file that is a link to a file
/// this section does not carry; the path and the target come first.
const LINKED_FILE_WITHHELD: &str = "which is outside /etc, key material, host identity, or a file \
     another section records, so it was not read and is not carried.";

/// The scan warning when `/etc/audit/audit.rules` is carried although the
/// scan could not tell whether the host writes it again; the path comes
/// first and the reason follows.
const AUDIT_RULES_UNCERTAIN: &str = "is carried as it is on the host, because the scan could not \
     establish whether augenrules writes it from /etc/audit/rules.d. In an image where augenrules \
     loads the audit rules, it writes this file again at every audit start, so keep rules that \
     must survive in /etc/audit/rules.d";

/// Why a walked link whose target is outside `/etc` carries no content.
const LINK_LEAVES_ETC_RATIONALE: &str = "Symlink points outside /etc: its target was not read and \
     is not carried. Recreate the link in the image if it is needed.";

/// The scan warning naming a packaged path [`admit::locate`] refused and
/// why. It names paths only, never content.
fn refused_path_warning(path: &str, refusal: &admit::Refusal) -> Warning {
    let reason = match refusal {
        admit::Refusal::NotUnderEtc => "it is not a path under /etc".to_string(),
        admit::Refusal::UnresolvedAncestor => {
            "a directory above it could not be resolved (a dangling or looping symbolic link, \
             or one that could not be read)"
                .to_string()
        }
        admit::Refusal::AliasLeavesEtc(physical) => format!(
            "a directory above it is a symbolic link, and the file it names, {physical}, is \
             outside /etc"
        ),
        admit::Refusal::AliasToWithheld(physical) => format!(
            "a directory above it is a symbolic link, and the path or the file it names, \
             {physical}, {ALIAS_TO_WITHHELD}"
        ),
    };
    Warning {
        inspector: "config".into(),
        message: format!("{path} {PATH_REFUSED} {reason}."),
        ..Default::default()
    }
}

fn withheld_link_warning(path: &str, target: &str) -> Warning {
    Warning {
        inspector: "config".into(),
        message: format!("{path} is a symbolic link to {target}, {LINKED_FILE_WITHHELD}"),
        ..Default::default()
    }
}

/// The scan warning naming a ghost file withheld as a credential.
fn ghost_credential_warning(path: &str) -> Warning {
    Warning {
        inspector: "config".into(),
        message: format!("{path} {GHOST_CREDENTIAL_WITHHELD}"),
        ..Default::default()
    }
}

/// Why a file authselect generates is an advisory and not copied.
const AUTHSELECT_EDITED_RATIONALE: &str = "This file differs from what authselect generates for the \
     selected profile, so it was changed outside authselect. The image runs authselect select, \
     which writes it again from the profile, so the change is not carried. To keep it, put it in a \
     custom profile (authselect create-profile), or for nsswitch, in \
     /etc/authselect/user-nsswitch.conf.";

/// The scan warning when `authselect check` reports a changed
/// configuration and no file is found to name.
const AUTHSELECT_EDITED_WARNING: &str = "authselect check reports the authentication configuration \
     was changed outside authselect, without naming the files. The image runs authselect select, \
     which writes those files again from the profile, so the change is not carried.";

/// The scan warning when the generated files could not be checked; the
/// reasons follow.
const AUTHSELECT_UNCHECKED_WARNING: &str = "The files authselect generates could not all be \
     checked for hand edits. The image runs authselect select, which writes them again from the \
     profile, so compare them with authselect test on the host before relying on the image";

/// When the selected authselect profile is collected, find every file
/// authselect generates that was changed outside it, and make each an
/// advisory: the export renders the profile as an `authselect select`
/// line, which writes those files again, so a hand edit to one is lost
/// and must be named. Two sources, since neither is enough alone:
///
/// - `authselect check`, for a changed mode, owner or link, which it
///   names on both releases, and content on 1.2.6;
/// - for each generated file under `/etc/authselect/` that is a regular
///   file, its content against what `authselect test` prints for the
///   recorded profile and features ([`ghost::AUTHSELECT_GENERATED`]),
///   because authselect 1.5.2's check passes a content edit.
///
/// An entry already collected at a named path (the ghost branch collects
/// `/etc/nsswitch.conf` when it is a regular file, for one) becomes the
/// advisory, so the export does not copy a file over what `authselect
/// select` writes. A check that reports a change but no file is found
/// is a scan warning, and so is a check or a comparison that could not
/// run ([`ghost::AuthselectCheck::Failed`]): an unknown is never read
/// as clean.
fn authselect_edit_advisories(
    exec: &dyn Executor,
    rpm_state: &RpmState,
    section: &mut ConfigSection,
    degraded_reasons: &mut Vec<String>,
    warnings: &mut Vec<Warning>,
) {
    use inspectah_core::types::config::AUTHSELECT_CONF_PATH;

    let Some(selection) = section
        .files
        .iter()
        .find(|f| f.path == AUTHSELECT_CONF_PATH)
        .map(|f| f.content.clone())
    else {
        return;
    };
    let mut edited: Vec<String> = Vec::new();
    let mut unchecked: Vec<String> = Vec::new();
    let mut check_said_edited = false;

    let check = exec.run("authselect", &["check"]);
    match ghost::parse_authselect_check(check.exit_code, &check.stdout, &check.stderr) {
        ghost::AuthselectCheck::Valid => {}
        ghost::AuthselectCheck::Edited { paths } => {
            check_said_edited = true;
            edited.extend(paths);
        }
        ghost::AuthselectCheck::Failed { exit_code } => {
            unchecked.push(format!(
                "authselect check exited {exit_code} without naming a file"
            ));
        }
    }

    match ghost::selection_args(&selection) {
        None => unchecked.push(format!(
            "{AUTHSELECT_CONF_PATH} does not name a profile and features authselect test can take"
        )),
        Some(selection) => {
            for (path, option) in ghost::AUTHSELECT_GENERATED {
                if !matches!(exec.file_kind(Path::new(path)), Ok(FileKind::Regular))
                    || admit::locate(exec, path).is_err()
                {
                    continue;
                }
                let Ok(on_host) = exec.read_file(Path::new(path)) else {
                    unchecked.push(format!("{path} could not be read"));
                    continue;
                };
                let mut args = vec!["test", *option];
                args.extend(selection.iter().copied());
                let expected = exec.run("authselect", &args);
                match ghost::test_body(&expected.stdout) {
                    Some(body) if expected.success() => {
                        if body != ghost::generated_body(&on_host) {
                            edited.push(path.to_string());
                        }
                    }
                    _ => unchecked.push(format!(
                        "authselect test {option} exited {}",
                        expected.exit_code
                    )),
                }
            }
        }
    }

    if !unchecked.is_empty() {
        warnings.push(Warning {
            inspector: "config".into(),
            message: format!("{AUTHSELECT_UNCHECKED_WARNING} ({}).", unchecked.join("; ")),
            ..Default::default()
        });
    }
    let mut seen = HashSet::new();
    edited.retain(|path| {
        path != AUTHSELECT_CONF_PATH && path.starts_with("/etc/") && seen.insert(path.clone())
    });
    if edited.is_empty() {
        if check_said_edited {
            warnings.push(Warning {
                inspector: "config".into(),
                message: AUTHSELECT_EDITED_WARNING.into(),
                ..Default::default()
            });
        }
        return;
    }
    let advisory =
        || FindingKind::advisory(AdvisoryType::Modernization, AUTHSELECT_EDITED_RATIONALE);
    for path in edited {
        if let Some(existing) = section.files.iter_mut().find(|f| f.path == path) {
            existing.disposition = with_modernization(&existing.disposition, advisory());
            continue;
        }
        if !matches!(exec.file_kind(Path::new(&path)), Ok(FileKind::Regular))
            || admit::locate(exec, &path).is_err()
        {
            continue;
        }
        let Ok((content, captured)) = capture_config(exec, &path, degraded_reasons) else {
            continue;
        };
        let kind = if rpm_state.is_rpm_owned(Path::new(&path)) {
            ConfigFileKind::RpmOwnedModified
        } else {
            ConfigFileKind::Unowned
        };
        section.files.push(ConfigFileEntry {
            category: classify_config_path(&path),
            package: rpm_state
                .package_for_path(Path::new(&path))
                .map(|p| p.name.clone()),
            path,
            kind,
            content,
            disposition: with_modernization(&captured, advisory()),
            ..Default::default()
        });
    }
}

/// `file_contexts.local`/`file_contexts.subs` under the SELinux policy
/// store are derived policy-store state collected as typed `FcontextRule`s
/// (see `inspectors::selinux::collect_fcontext_rules`), not config. The
/// unowned-file walk already excludes them via `UNOWNED_EXCLUDE_GLOBS`
/// (`contexts/files/*`), but `rpm -Va` can also report them as a modified
/// owned file (they are `rpm_owned_modified` on a host where policycoreutils
/// owns them), which takes the separate code path above and needs its own
/// skip.
fn is_selinux_fcontext_store_file(path: &str) -> bool {
    path.starts_with("/etc/selinux/")
        && (path.ends_with("/contexts/files/file_contexts.local")
            || path.ends_with("/contexts/files/file_contexts.subs"))
}

/// Classify an RPM-verified config file as Modified or Default based on
/// verification flags.
///
/// `rpm -Va` flags: `S.5....T.` where each position indicates a change:
///   S=Size, M=Mode, 5=digest, D=Device, L=readLink, U=User, G=Group,
///   T=mTime, P=caPabilities.
///
/// Files with content changes (digest `5` or size `S`) are genuinely
/// modified. Files with only metadata changes (mtime, group, mode, etc.)
/// are typically scriptlet-regenerated defaults that match the package's
/// intended state — classified as RpmOwnedDefault so the attention system
/// treats them as Routine instead of NeedsReview.
fn classify_rpm_va_kind(flags: &str) -> ConfigFileKind {
    let has_content_change = flags.chars().enumerate().any(|(i, c)| {
        match i {
            0 => c == 'S', // Size
            2 => c == '5', // digest (MD5/SHA)
            _ => false,
        }
    });
    if has_content_change {
        ConfigFileKind::RpmOwnedModified
    } else {
        ConfigFileKind::RpmOwnedDefault
    }
}

/// Symlink-aware RPM ownership check.
///
/// If the file at `path` is a symlink, resolves the full chain and checks
/// whether the *target* path is RPM-owned. This catches cases like
/// `/etc/ssl/certs/ca-bundle.crt` -> `/etc/pki/tls/certs/ca-bundle.crt`
/// where RPM owns the target but not the symlink path.
///
/// Returns `false` if the path is not a symlink or resolution fails.
fn is_rpm_owned_via_symlink(exec: &dyn Executor, rpm_state: &RpmState, path: &str) -> bool {
    // Only check paths that are actually symlinks.
    if exec.read_link(Path::new(path)).is_err() {
        return false;
    }
    if let Ok(resolved) = exec.resolve_final_target(Path::new(path)) {
        rpm_state.is_rpm_owned(&resolved)
    } else {
        false
    }
}

/// Directory holding the authselect profile files the `/etc/pam.d/` links
/// point at.
const AUTHSELECT_PREFIX: &str = "/etc/authselect/";

/// Directory the authselect-managed PAM links live in.
const PAM_D_PREFIX: &str = "/etc/pam.d/";

/// § 3.6's authselect rule: an `rpm -Va` entry under `/etc/pam.d/` whose
/// only flag is `L` is an RPM-owned symlink whose target changed. When that
/// target resolves under `/etc/authselect/` it is not a local PAM
/// customization: the customization is the authselect profile, which the
/// `%ghost` branch collects from `/etc/authselect/authselect.conf` and the
/// export carries as an `authselect select` line. Classifying
/// the link as a modified config would carry the same content twice, under
/// a path the target system regenerates.
///
/// On a host running authselect this is the state of `system-auth`,
/// `password-auth`, `fingerprint-auth`, `smartcard-auth` and `postlogin`.
///
/// Reachability, measured rather than assumed: `classify_rpm_va_kind` reads
/// only the size and digest columns, so a flags string whose only flag is
/// `L` is already `RpmOwnedDefault` and already dropped a few lines below
/// this call. This rule is therefore a guard on the classification rather
/// than the filter that does the work today, and deleting it changes no
/// output. It is kept because the premise it defends -- that the link is
/// never the subject -- does not depend on which columns
/// `classify_rpm_va_kind` happens to read.
fn is_authselect_pam_link(exec: &dyn Executor, path: &str, flags: &str) -> bool {
    if !path.starts_with(PAM_D_PREFIX) {
        return false;
    }
    if !flags.contains('L') || !flags.chars().all(|c| c == '.' || c == 'L') {
        return false;
    }
    match exec.resolve_final_target(Path::new(path)) {
        Ok(resolved) => resolved.to_string_lossy().starts_with(AUTHSELECT_PREFIX),
        Err(_) => false,
    }
}

/// Where an admin points a package symlink to mask the file it names.
const DEV_NULL: &str = "/dev/null";

/// § 5.1's RPM-owned mask rule: an `rpm -Va` entry with the readLink flag
/// `L` whose path resolves, through any number of hops, to `/dev/null` is
/// an RPM-owned config an admin masked, so it is `RpmOwnedModified`.
///
/// RPM verifies a file that is not a regular file through `L` alone: it
/// skips the size and digest checks for it (`lib/verify.c`,
/// `rpmVerifyFile`). `classify_rpm_va_kind` reads only those two columns,
/// so without this rule the row is `RpmOwnedDefault` and dropped, and
/// because its path is in `rpm_va_paths` the unowned walk skips it too.
/// The mask is then lost. Its content is read as for any entry, which for
/// `/dev/null` is empty, so the image gets an empty file of the same name.
///
/// A link re-pointed to any other file is not covered: carrying it needs
/// a decision the spec leaves to the defaults work.
fn is_rpm_owned_dev_null_link(exec: &dyn Executor, path: &str, flags: &str) -> bool {
    flags.contains('L')
        && exec
            .resolve_final_target(Path::new(path))
            .is_ok_and(|resolved| resolved == Path::new(DEV_NULL))
}

/// Why the collector could not capture a config file's content.
#[derive(Debug)]
enum CaptureFailure {
    /// Over [`MAX_CONFIG_FILE_SIZE`].
    TooLarge {
        bytes: usize,
    },
    PermissionDenied,
    /// Any other read error, including content that is not UTF-8 text.
    Unreadable(std::io::Error),
}

impl CaptureFailure {
    /// The rationale the `ContentNotCaptured` advisory carries to the
    /// audit report, the HTML report and refine.
    fn rationale(&self) -> String {
        let cause = match self {
            Self::TooLarge { bytes } => {
                format!("file too large ({bytes} bytes, limit {MAX_CONFIG_FILE_SIZE})")
            }
            Self::PermissionDenied => "permission denied reading the file".to_string(),
            Self::Unreadable(e) => format!("file could not be read as text ({e})"),
        };
        format!("Content not captured: {cause}. Add the file to the image by hand if it is needed.")
    }
}

/// Reads file content with size guard.
///
/// Oversized and permission-denied files are also recorded in
/// `degraded_reasons`; other read errors are not, matching what the
/// inspector reported before failures became advisories.
fn read_config_content(
    exec: &dyn Executor,
    path: &str,
    degraded_reasons: &mut Vec<String>,
) -> Result<String, CaptureFailure> {
    match exec.read_file(Path::new(path)) {
        Ok(content) if content.len() > MAX_CONFIG_FILE_SIZE => {
            degraded_reasons.push(format!("file too large ({} bytes): {path}", content.len()));
            Err(CaptureFailure::TooLarge {
                bytes: content.len(),
            })
        }
        Ok(content) => Ok(content),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            degraded_reasons.push(format!("permission denied reading {path}"));
            Err(CaptureFailure::PermissionDenied)
        }
        Err(e) => Err(CaptureFailure::Unreadable(e)),
    }
}

/// The disposition a collected entry takes when a modernization pattern
/// matches its path.
///
/// A `ContentNotCaptured` entry keeps that type, with the modernization
/// advice appended to its rationale, so the reports carry both reasons.
/// The type is what tells every later stage that the entry has no content:
/// the fleet merge keeps it out of the content variants on that basis.
/// Any other disposition is replaced by the modernization advisory.
fn with_modernization(existing: &FindingKind, modernization: FindingKind) -> FindingKind {
    match (existing, &modernization) {
        (
            FindingKind::Advisory {
                advisory_type: AdvisoryType::ContentNotCaptured,
                rationale: failure,
            },
            FindingKind::Advisory {
                rationale: advice, ..
            },
        ) => FindingKind::advisory(
            AdvisoryType::ContentNotCaptured,
            format!("{failure} Modernization: {advice}"),
        ),
        _ => modernization,
    }
}

/// The content and starting disposition of a collected config file, or
/// why its content is not read at all ([`admit::readable`]): the one read
/// every branch makes of a file it keeps.
///
/// Captured content, empty or not, keeps the default disposition. A link
/// to `/dev/null` is a mask: its content is empty, and nothing is opened.
/// A failed capture becomes a `ContentNotCaptured` advisory with no
/// content: the config tree stages only included entries, and refine and
/// the fleet merge leave advisories alone, so no export path can copy an
/// empty file over the one on the host.
fn capture_config(
    exec: &dyn Executor,
    path: &str,
    degraded_reasons: &mut Vec<String>,
) -> Result<(String, FindingKind), admit::Unreadable> {
    if admit::readable(exec, path)? == admit::Readable::DevNullMask {
        return Ok((String::new(), FindingKind::default()));
    }
    Ok(match read_config_content(exec, path, degraded_reasons) {
        Ok(content) => (content, FindingKind::default()),
        Err(failure) => (
            String::new(),
            FindingKind::advisory(AdvisoryType::ContentNotCaptured, failure.rationale()),
        ),
    })
}

/// Checks if the source system is an ostree/bootc variant.
fn is_ostree_system(source: &SourceSystem) -> bool {
    matches!(
        source,
        SourceSystem::RpmOstree { .. } | SourceSystem::Bootc { .. }
    )
}

/// The file naming the system crypto policy.
const CRYPTO_POLICY_CONFIG: &str = "/etc/crypto-policies/config";

/// The scan warning when the crypto policy file's first line is not a
/// policy name; the path comes first. It never quotes the line: the file
/// is read before any config is collected.
const CRYPTO_POLICY_UNEXPECTED: &str = "does not start with a policy name (capital letters, \
     digits, `_`, `:`, `.` and `-`), so the Containerfile update-crypto-policies command will be \
     skipped";

/// The scan warning when the crypto policy is not `DEFAULT`; the path
/// comes first. It never names the policy: a value that passes the name
/// check can still be a secret (`PASSWORD:<value>` does), and redaction
/// does not scan warnings. The collected file carries the name.
const CRYPTO_POLICY_NOT_DEFAULT: &str = "names a crypto policy other than DEFAULT, which the base \
     image may use";

/// Detects the system crypto policy and adds a warning if non-default.
///
/// The file is opened only as a capture would open it
/// ([`admit::may_read_to_decide`]). Every warning here is fixed text and
/// the path, never a value read from the file.
fn detect_crypto_policy(exec: &dyn Executor, warnings: &mut Vec<Warning>) {
    if !admit::may_read_to_decide(exec, CRYPTO_POLICY_CONFIG) {
        return;
    }
    let content = match exec.read_file(Path::new(CRYPTO_POLICY_CONFIG)) {
        Ok(c) => c,
        Err(_) => return,
    };

    let first_line = content.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        return;
    }

    // Strip inline comments
    let policy = if let Some(idx) = first_line.find('#') {
        first_line[..idx].trim()
    } else {
        first_line
    };

    if policy.is_empty() {
        return;
    }

    // Validate policy name format: uppercase letters, digits, underscores, colons, dots, hyphens
    let valid = policy.chars().all(|c| {
        c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '_' | ':' | '.' | '-')
    });
    if !valid || !policy.starts_with(|c: char| c.is_ascii_uppercase()) {
        warnings.push(Warning {
            inspector: "config".into(),
            message: format!("{CRYPTO_POLICY_CONFIG} {CRYPTO_POLICY_UNEXPECTED}"),
            ..Default::default()
        });
        return;
    }

    if policy != "DEFAULT" {
        warnings.push(Warning {
            inspector: "config".into(),
            message: format!("{CRYPTO_POLICY_CONFIG} {CRYPTO_POLICY_NOT_DEFAULT}"),
            ..Default::default()
        });
    }
}

/// Detects orphaned config files from removed packages.
///
/// Uses `dnf history` to find removed packages, then walks /etc looking
/// for config files whose basename contains the removed package name.
fn detect_orphaned_configs(
    exec: &dyn Executor,
    rpm_state: &RpmState,
    rpm_va_paths: &HashSet<String>,
    dhcp_paths: &HashSet<String>,
    section: &mut ConfigSection,
    degraded_reasons: &mut Vec<String>,
) {
    // Get removed packages from dnf history
    let result = exec.run("dnf", &["history", "list", "--reverse"]);
    if result.exit_code != 0 {
        // dnf history not available — not fatal, just skip orphan detection
        return;
    }

    let removed_packages = parse_removed_packages(&result.stdout);
    if removed_packages.is_empty() {
        return;
    }

    // Build set of already-captured paths (owned strings to avoid borrow conflict)
    let seen_paths: HashSet<String> = section.files.iter().map(|f| f.path.clone()).collect();

    let etc_files = match walk_etc_recursive(exec, "/etc") {
        Ok(files) => files,
        Err(_) => return,
    };

    for pkg_name in &removed_packages {
        for rel_path in &etc_files {
            let basename = rel_path.rsplit('/').next().unwrap_or(rel_path);
            if !basename.contains(pkg_name.as_str()) {
                continue;
            }

            let abs_path = format!("/etc/{rel_path}");
            if seen_paths.contains(&abs_path) {
                continue;
            }
            let Ok(located) = admit::locate(exec, &abs_path) else {
                continue;
            };
            // Ownership by the walked spelling only, as the walk above.
            if rpm_state.is_rpm_owned(Path::new(&located.path))
                || rpm_va_paths.contains(&located.path)
                || located.spellings().any(|p| dhcp_paths.contains(p))
            {
                continue;
            }
            if is_rpm_owned_via_symlink(exec, rpm_state, &abs_path) {
                continue;
            }

            let Ok((content, disposition)) = capture_config(exec, &abs_path, degraded_reasons)
            else {
                continue;
            };

            section.files.push(ConfigFileEntry {
                path: abs_path,
                kind: ConfigFileKind::Orphaned,
                category: classify_config_path(&format!("/etc/{rel_path}")),
                content,
                package: Some(pkg_name.clone()),
                diff_against_rpm: None,
                disposition,
                ..Default::default()
            });
        }
    }
}

/// Parses `dnf history list` output for removed packages.
///
/// Looks for lines containing "Erase" or "Remove" actions and extracts
/// the package name.
fn parse_removed_packages(output: &str) -> Vec<String> {
    let mut packages = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.contains("Erase") || trimmed.contains("Removed") || trimmed.contains("Remove") {
            // dnf history output varies, extract package names that appear
            // before the action keyword
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                // Package name is typically in the first or second column
                let candidate = parts[0];
                if !candidate.chars().all(|c| c.is_ascii_digit()) {
                    packages.push(candidate.to_string());
                }
            }
        }
    }
    packages
}

/// Ostree/bootc config detection.
///
/// Diffs /usr/etc against /etc overlays to find modified and unowned configs.
fn run_ostree_config(
    exec: &dyn Executor,
    _rpm_state: &RpmState,
    _warnings: &mut [Warning],
) -> ConfigSection {
    let mut section = ConfigSection::default();

    let etc = "/etc";
    let usr_etc = "/usr/etc";

    // Check if /etc exists (try read_dir since it's a directory)
    if exec.read_dir(Path::new(etc)).is_err() && !exec.file_exists(Path::new(etc)) {
        return section;
    }

    // Ostree volatile names — system-generated, skip
    let volatile_names: HashSet<&str> = [
        "resolv.conf",
        "hostname",
        "machine-id",
        ".updated",
        "ld.so.cache",
    ]
    .into_iter()
    .collect();

    let skip_basenames: HashSet<&str> = ["os-release"].into_iter().collect();

    // Track /etc paths covered by Tier 1 (have a /usr/etc counterpart)
    let mut tier1_paths: HashSet<String> = HashSet::new();

    // Tier 1: /usr/etc -> /etc diff
    if (exec.read_dir(Path::new(usr_etc)).is_ok() || exec.file_exists(Path::new(usr_etc)))
        && let Ok(files) = walk_etc_recursive(exec, usr_etc)
    {
        for rel_path in files {
            let basename = rel_path.rsplit('/').next().unwrap_or(&rel_path);
            if volatile_names.contains(basename) {
                continue;
            }

            let etc_path = format!("{etc}/{rel_path}");
            tier1_paths.insert(etc_path.clone());

            if !exec.file_exists(Path::new(&etc_path)) {
                continue; // Only in /usr/etc — normal ostree behavior
            }

            let display_path = format!("etc/{rel_path}");

            // Content comparison
            let usr_content = match exec.read_file(Path::new(&format!("{usr_etc}/{rel_path}"))) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let etc_content = match exec.read_file(Path::new(&etc_path)) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if usr_content != etc_content {
                section.files.push(ConfigFileEntry {
                    path: display_path,
                    kind: ConfigFileKind::RpmOwnedModified,
                    category: classify_config_path(&format!("/etc/{rel_path}")),
                    content: etc_content,
                    diff_against_rpm: None, // Phase 3
                    ..Default::default()
                });
            }
        }
    }

    // Tier 2: /etc-only files (no /usr/etc counterpart)
    if let Ok(files) = walk_etc_recursive(exec, etc) {
        for rel_path in files {
            let abs_path = format!("{etc}/{rel_path}");
            if tier1_paths.contains(&abs_path) {
                continue;
            }

            let basename = rel_path.rsplit('/').next().unwrap_or(&rel_path);
            if volatile_names.contains(basename) || skip_basenames.contains(basename) {
                continue;
            }

            let display_path = format!("etc/{rel_path}");
            let content = exec.read_file(Path::new(&abs_path)).unwrap_or_default();

            section.files.push(ConfigFileEntry {
                path: display_path.clone(),
                kind: ConfigFileKind::Unowned,
                category: classify_config_path(&format!("/{display_path}")),
                content,
                diff_against_rpm: None,
                ..Default::default()
            });
        }
    }

    section
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::traits::progress::NullProgress;
    use inspectah_core::types::config::ConfigCategory;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmVaEntry};
    use inspectah_core::types::system::SourceSystem;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn test_os_release() -> OsRelease {
        OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        }
    }

    fn test_source_system() -> SourceSystem {
        SourceSystem::PackageBased {
            os_release: test_os_release(),
        }
    }

    fn empty_rpm_state() -> RpmState {
        RpmState::default()
    }

    fn rpm_state_with_va_and_owned(
        va_entries: Vec<RpmVaEntry>,
        owned: Vec<&str>,
        packages: Vec<PackageEntry>,
    ) -> RpmState {
        let mut owned_paths = HashSet::new();
        for p in &owned {
            owned_paths.insert(PathBuf::from(p));
        }

        let mut path_to_package = HashMap::new();
        for (idx, pkg) in packages.iter().enumerate() {
            // Simple mapping: use package name to find owned paths
            for op in &owned {
                if op.contains(&pkg.name) {
                    path_to_package.insert(PathBuf::from(op), idx);
                }
            }
        }

        RpmState {
            owned_paths,
            packages,
            verification_results: Some(va_entries),
            path_to_package,
            ..Default::default()
        }
    }

    /// The mock command key of the ghost-file query.
    fn ghost_query_key() -> String {
        format!("rpm -qa --queryformat {}", ghost::RPM_FILE_FLAGS_FORMAT)
    }

    /// `mock` answering the ghost-file query with `stdout`, lines as rpm
    /// prints them for [`ghost::RPM_FILE_FLAGS_FORMAT`].
    fn with_ghost_query(mock: MockExecutor, stdout: &str) -> MockExecutor {
        mock.with_command(
            &ghost_query_key(),
            ExecResult {
                stdout: stdout.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
    }

    fn base_mock_with_etc() -> MockExecutor {
        with_ghost_query(MockExecutor::new(), "")
            .with_dir("/etc", vec![])
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1, // no dnf history available
                    ..Default::default()
                },
            )
    }

    // ---- Test 15: test_config_rpm_owned_modified ----

    #[test]
    fn test_config_rpm_owned_modified() {
        let exec = with_ghost_query(MockExecutor::new(), "")
            .with_dir("/etc", vec!["httpd"])
            .with_dir("/etc/httpd", vec!["conf"])
            .with_dir("/etc/httpd/conf", vec!["httpd.conf"])
            .with_file(
                "/etc/httpd/conf/httpd.conf",
                "ServerRoot \"/etc/httpd\"\nListen 8080\n",
            )
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let rpm_state = rpm_state_with_va_and_owned(
            vec![RpmVaEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                flags: "S.5....T.".into(),
                package: Some("httpd".into()),
            }],
            vec!["/etc/httpd/conf/httpd.conf"],
            vec![PackageEntry {
                name: "httpd".into(),
                version: "2.4.57".into(),
                state: PackageState::Added,
                ..Default::default()
            }],
        );

        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::Config(ref section) = output.section {
            assert_eq!(section.files.len(), 1);
            assert_eq!(section.files[0].kind, ConfigFileKind::RpmOwnedModified);
            assert_eq!(section.files[0].rpm_va_flags, Some("S.5....T.".to_string()));
            assert_eq!(section.files[0].package, Some("httpd".to_string()));
            assert!(section.files[0].content.contains("Listen 8080"));
        } else {
            panic!("expected Config section");
        }
    }

    // ---- Test 16: test_config_unowned ----

    #[test]
    fn test_config_unowned() {
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["custom-app.conf"])
            .with_file("/etc/custom-app.conf", "setting=value\n")
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        // RPM state has no owned paths and no va entries
        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::Config(ref section) = output.section {
            assert_eq!(section.files.len(), 1);
            assert_eq!(section.files[0].kind, ConfigFileKind::Unowned);
            assert_eq!(section.files[0].path, "/etc/custom-app.conf");
        } else {
            panic!("expected Config section");
        }
    }

    /// Registration state that insights-client and subscription-manager
    /// write under `/etc` is host identity, not configuration, and must not
    /// reach the image: every machine deployed from it would carry the
    /// source host's Insights machine-id and `.registered` marker.
    ///
    /// The excluded paths are the unowned files a real RHEL 10.2 scan
    /// staged, plus `.unregistered`, the client's other registration
    /// marker. The same directories hold configuration the admin writes:
    /// Insights tags and redaction rules, and custom facts. Those must be
    /// carried, or the snapshot loses them with no record refine could
    /// recover. The sibling paths share a leading string with an excluded
    /// directory but not the directory itself, so they pin that the
    /// exclusion holds on a path boundary.
    ///
    /// The owned, modified files pin that the exclusions stay out of the
    /// rpm -Va branch: a package's own file, edited by the admin, is
    /// collected even in a directory the exclusions reach. The owned
    /// dotfile is one the dotfile rule matches, so the case fails if that
    /// rule is ever applied to rpm -Va results.
    #[test]
    fn test_config_unowned_skips_registration_state() {
        // (path, carried as an unowned config file)
        let cases: &[(&str, bool)] = &[
            ("/etc/insights-client/machine-id", false),
            ("/etc/insights-client/.registered", false),
            ("/etc/insights-client/.unregistered", false),
            ("/etc/insights-client/.lastupload", false),
            ("/etc/insights-client/.last-upload.results", false),
            ("/etc/rhsm/facts/insights-client.facts", false),
            ("/etc/pki/product/419.pem", false),
            ("/etc/insights-client/tags.yaml", true),
            ("/etc/insights-client/file-redaction.yaml", true),
            ("/etc/insights-client/file-content-redaction.yaml", true),
            ("/etc/rhsm/facts/site.facts", true),
            ("/etc/insights-client-local/machine-id", true),
            ("/etc/rhsm/facts-custom/site.facts", true),
            ("/etc/pki/product-default/README", true),
            ("/etc/pki/products/419.pem", true),
        ];
        // (path, content) of package-owned files the admin modified
        let owned_cases: &[(&str, &str)] = &[
            (
                "/etc/insights-client/insights-client.conf",
                "[insights-client]\nhttp_timeout=120\n",
            ),
            ("/etc/insights-client/.exp.sed", "s/secret/REDACTED/\n"),
        ];
        const CONTENT_CHANGED: &str = "S.5....T.";

        let mut exec = base_mock_with_etc();
        let all_paths = cases
            .iter()
            .map(|(path, _)| (*path, "host state\n"))
            .chain(owned_cases.iter().copied());
        for (path, content) in all_paths {
            let mut parent = String::from("/etc");
            let rel = path.strip_prefix("/etc/").expect("case path is under /etc");
            for component in rel.split('/') {
                exec = exec.with_dir_entry(&parent, component);
                parent = format!("{parent}/{component}");
            }
            exec = exec.with_file(path, content);
        }

        let rpm_state = rpm_state_with_va_and_owned(
            owned_cases
                .iter()
                .map(|(path, _)| RpmVaEntry {
                    path: (*path).into(),
                    flags: CONTENT_CHANGED.into(),
                    package: Some("insights-client".into()),
                })
                .collect(),
            owned_cases.iter().map(|(path, _)| *path).collect(),
            vec![PackageEntry {
                name: "insights-client".into(),
                version: "3.2.8".into(),
                state: PackageState::Added,
                ..Default::default()
            }],
        );
        let source = test_source_system();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let output = ConfigInspector::new()
            .inspect(&ctx, &NullProgress)
            .expect("should succeed");
        let SectionData::Config(ref section) = output.section else {
            panic!("expected Config section");
        };
        let carried: HashSet<&str> = section
            .files
            .iter()
            .filter(|f| f.kind == ConfigFileKind::Unowned)
            .map(|f| f.path.as_str())
            .collect();
        let collected: HashSet<&str> = section.files.iter().map(|f| f.path.as_str()).collect();

        for (path, expected) in cases {
            assert_eq!(
                carried.contains(path),
                *expected,
                "{path}: expected carried={expected}, unowned files were {carried:?}"
            );
            if !*expected {
                assert!(
                    !collected.contains(path),
                    "{path}: excluded registration file reached the section, files were {collected:?}"
                );
            }
        }

        for (path, content) in owned_cases {
            let owned = section
                .files
                .iter()
                .find(|f| f.path == *path)
                .unwrap_or_else(|| panic!("{path} not collected, files were {collected:?}"));
            assert_eq!(owned.kind, ConfigFileKind::RpmOwnedModified, "{path}");
            assert_eq!(
                owned.rpm_va_flags.as_deref(),
                Some(CONTENT_CHANGED),
                "{path}"
            );
            assert_eq!(owned.package.as_deref(), Some("insights-client"), "{path}");
            assert_eq!(owned.content, *content, "{path}");
        }
    }

    // ---- Test 17: test_config_orphaned ----

    #[test]
    fn test_config_orphaned() {
        // Orphan detection: a file whose basename contains a removed package name
        // but is NOT already captured as unowned (because the unowned walk
        // already found it). To test orphan detection specifically, we need a
        // file that the unowned walk skips (e.g., it's rpm-owned) but the package
        // was removed — leaving the file behind.
        //
        // Simpler approach: verify the orphan detection logic runs by having a
        // file that the unowned walk finds AND verifying dnf history parsing works.
        // A file found in step 2 as Unowned is the normal behavior; orphan
        // detection (step 3) only catches files missed by step 2.
        let exec = with_ghost_query(MockExecutor::new(), "")
            .with_dir("/etc", vec!["oldpkg.conf", "active.conf"])
            .with_file("/etc/oldpkg.conf", "leftover config\n")
            .with_file("/etc/active.conf", "active config\n")
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    stdout: "ID | Command line     | Date       | Action | Altered\n\
                             1  | install httpd    | 2024-01-01 | Install | 5\n\
                             2  | remove oldpkg    | 2024-02-01 | Erase  | 1\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        // Mark active.conf as RPM-owned so only oldpkg.conf shows as unowned
        let rpm_state = rpm_state_with_va_and_owned(
            vec![],
            vec!["/etc/active.conf"],
            vec![PackageEntry {
                name: "active".into(),
                version: "1.0".into(),
                state: PackageState::Added,
                ..Default::default()
            }],
        );

        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::Config(ref section) = output.section {
            // oldpkg.conf should appear — either as Unowned (step 2 catches it
            // because it's not RPM-owned) or as Orphaned (step 3 would catch it
            // if step 2 didn't). In practice, step 2 finds it first as Unowned.
            // The orphaned path only catches files that step 2 missed.
            assert!(!section.files.is_empty(), "should find config files");
            let oldpkg = section
                .files
                .iter()
                .find(|f| f.path.contains("oldpkg"))
                .expect("should find oldpkg.conf");
            // Found as Unowned since step 2 picks it up first
            assert_eq!(oldpkg.kind, ConfigFileKind::Unowned);

            // active.conf is RPM-owned and not modified — should NOT appear
            assert!(
                !section.files.iter().any(|f| f.path.contains("active")),
                "RPM-owned unmodified file should not appear"
            );
        } else {
            panic!("expected Config section");
        }
    }

    // ---- Test 18: test_config_ostree_branch ----

    #[test]
    fn test_config_ostree_branch() {
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["custom.conf", "base.conf"])
            .with_file("/etc/custom.conf", "custom content\n")
            .with_dir("/usr/etc", vec!["base.conf"])
            .with_file("/usr/etc/base.conf", "base content\n")
            .with_file("/etc/base.conf", "modified content\n")
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        // Use Bootc source system to trigger ostree branch
        let source = SourceSystem::Bootc {
            os_release: test_os_release(),
            booted_image: "quay.io/test:latest".into(),
            staged_image: None,
        };

        let rpm_state = empty_rpm_state();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::Config(ref section) = output.section {
            // Should find modified base.conf (different content in /etc vs /usr/etc)
            let modified: Vec<_> = section
                .files
                .iter()
                .filter(|f| f.kind == ConfigFileKind::RpmOwnedModified)
                .collect();
            assert!(!modified.is_empty(), "should detect modified ostree config");
            // Should find unowned custom.conf (only in /etc, not in /usr/etc)
            let unowned: Vec<_> = section
                .files
                .iter()
                .filter(|f| f.kind == ConfigFileKind::Unowned)
                .collect();
            assert!(!unowned.is_empty(), "should detect unowned ostree config");
        } else {
            panic!("expected Config section");
        }
    }

    // ---- Test 19: test_config_crypto_policy_detection ----

    #[test]
    fn test_config_crypto_policy_detection() {
        let exec = MockExecutor::new()
            .with_dir("/etc", vec![])
            .with_file("/etc/crypto-policies/config", "FUTURE\n")
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        let warning = output
            .warnings
            .iter()
            .find(|w| w.message.starts_with(CRYPTO_POLICY_CONFIG))
            .expect("should warn about non-DEFAULT crypto policy");
        assert!(
            !warning.message.contains("FUTURE"),
            "the warning never quotes the file: {}",
            warning.message
        );
    }

    // ---- Test 20: test_config_degraded_permission_denied ----

    #[test]
    fn test_config_degraded_permission_denied() {
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["readable", "secret"])
            .with_dir("/etc/readable", vec!["file.conf"])
            .with_file("/etc/readable/file.conf", "content\n")
            .with_file_error(
                "/etc/secret/shadow.conf",
                std::io::ErrorKind::PermissionDenied,
            )
            .with_dir("/etc/secret", vec!["shadow.conf"])
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Degraded { reason, partial }) => {
                assert!(
                    reason.contains("permission denied"),
                    "expected permission denied in reason, got: {reason}"
                );
                if let SectionData::Config(ref section) = partial.section {
                    // Should still have partial results
                    assert!(
                        !section.files.is_empty(),
                        "degraded should still have partial files"
                    );
                } else {
                    panic!("expected Config section in degraded output");
                }
            }
            other => panic!("expected Degraded error for permission denied, got: {other:?}"),
        }
    }

    // ---- Test 21: test_config_empty_etc ----

    #[test]
    fn test_config_empty_etc() {
        let exec = base_mock_with_etc();

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed on empty /etc");
        if let SectionData::Config(ref section) = output.section {
            assert!(section.files.is_empty());
        } else {
            panic!("expected Config section");
        }
    }

    // ---- Test 22: test_config_content_read ----

    #[test]
    fn test_config_content_read() {
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["app.conf"])
            .with_file(
                "/etc/app.conf",
                "# Application config\nport=8080\nhost=0.0.0.0\n",
            )
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::Config(ref section) = output.section {
            assert_eq!(section.files.len(), 1);
            assert!(section.files[0].content.contains("port=8080"));
        } else {
            panic!("expected Config section");
        }
    }

    // ---- Test 23: test_config_dhcp_excluded ----

    #[test]
    fn test_config_dhcp_excluded() {
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["NetworkManager"])
            .with_dir("/etc/NetworkManager", vec!["system-connections"])
            .with_dir(
                "/etc/NetworkManager/system-connections",
                vec!["eth0.nmconnection"],
            )
            .with_file(
                "/etc/NetworkManager/system-connections/eth0.nmconnection",
                "[connection]\nid=eth0\n[ipv4]\nmethod=auto\n",
            )
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::Config(ref section) = output.section {
            let has_dhcp = section.files.iter().any(|f| f.path.contains("eth0"));
            assert!(!has_dhcp, "DHCP connections should be excluded from config");
        } else {
            panic!("expected Config section");
        }
    }

    // ---- Test 24: test_config_category_assignment ----

    #[test]
    fn test_config_category_assignment() {
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["tmpfiles.d", "sysctl.d"])
            .with_dir("/etc/tmpfiles.d", vec!["custom.conf"])
            .with_file("/etc/tmpfiles.d/custom.conf", "d /tmp/custom 0755\n")
            .with_dir("/etc/sysctl.d", vec!["99-custom.conf"])
            .with_file("/etc/sysctl.d/99-custom.conf", "vm.swappiness=10\n")
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::Config(ref section) = output.section {
            let tmpfiles_entry = section
                .files
                .iter()
                .find(|f| f.path.contains("tmpfiles.d"))
                .expect("should find tmpfiles entry");
            assert_eq!(tmpfiles_entry.category, ConfigCategory::Tmpfiles);

            let sysctl_entry = section
                .files
                .iter()
                .find(|f| f.path.contains("sysctl.d"))
                .expect("should find sysctl entry");
            assert_eq!(sysctl_entry.category, ConfigCategory::Sysctl);
        } else {
            panic!("expected Config section");
        }
    }

    // ---- Test 25: test_config_diff_against_rpm_always_none ----

    #[test]
    fn test_config_diff_against_rpm_always_none() {
        let exec = with_ghost_query(MockExecutor::new(), "")
            .with_dir("/etc", vec!["custom.conf"])
            .with_file("/etc/custom.conf", "content\n")
            .with_file("/etc/httpd/conf/httpd.conf", "modified\n")
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let rpm_state = rpm_state_with_va_and_owned(
            vec![RpmVaEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                flags: "S.5....T.".into(),
                package: Some("httpd".into()),
            }],
            vec!["/etc/httpd/conf/httpd.conf"],
            vec![PackageEntry {
                name: "httpd".into(),
                version: "2.4.57".into(),
                state: PackageState::Added,
                ..Default::default()
            }],
        );

        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::Config(ref section) = output.section {
            for file in &section.files {
                assert!(
                    file.diff_against_rpm.is_none(),
                    "diff_against_rpm should be None in Phase 2 for path: {}",
                    file.path
                );
            }
        } else {
            panic!("expected Config section");
        }
    }

    // ---- RPM state None -> Failed ----

    #[test]
    fn test_rpm_state_none_returns_failed() {
        let exec = MockExecutor::new();
        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Failed { reason }) => {
                assert!(
                    reason.contains("RPM prerequisite"),
                    "expected RPM prerequisite message, got: {reason}"
                );
            }
            other => panic!("expected Failed error for None rpm_state, got: {other:?}"),
        }
    }

    // -- classify_rpm_va_kind tests -------------------------------------------

    #[test]
    fn va_digest_change_is_modified() {
        // S.5....T. — size and digest changed (content modification)
        assert_eq!(
            classify_rpm_va_kind("S.5....T."),
            ConfigFileKind::RpmOwnedModified
        );
    }

    #[test]
    fn va_size_only_is_modified() {
        // S........  — only size changed
        assert_eq!(
            classify_rpm_va_kind("S........"),
            ConfigFileKind::RpmOwnedModified
        );
    }

    #[test]
    fn va_digest_only_is_modified() {
        // ..5......  — only digest changed
        assert_eq!(
            classify_rpm_va_kind("..5......"),
            ConfigFileKind::RpmOwnedModified
        );
    }

    #[test]
    fn va_mtime_only_is_default() {
        // .......T.  — only mtime changed (scriptlet-regenerated)
        assert_eq!(
            classify_rpm_va_kind(".......T."),
            ConfigFileKind::RpmOwnedDefault
        );
    }

    #[test]
    fn va_mode_and_mtime_is_default() {
        // .M.....T.  — mode + mtime (metadata-only change)
        assert_eq!(
            classify_rpm_va_kind(".M.....T."),
            ConfigFileKind::RpmOwnedDefault
        );
    }

    #[test]
    fn va_group_only_is_default() {
        // ......G..  — only group changed
        assert_eq!(
            classify_rpm_va_kind("......G.."),
            ConfigFileKind::RpmOwnedDefault
        );
    }

    #[test]
    fn va_all_dots_is_default() {
        // .........  — all dots (no changes detected but listed)
        assert_eq!(
            classify_rpm_va_kind("........."),
            ConfigFileKind::RpmOwnedDefault
        );
    }

    /// Tripwire for `is_authselect_pam_link`, which is unreachable while
    /// this holds.
    ///
    /// The authselect skip exists for an `rpm -Va` entry whose only flag
    /// is `L`. This classifier reads the size and digest columns only, so
    /// such an entry is already `RpmOwnedDefault` and already dropped
    /// before the skip can matter. Widening the classifier to read the
    /// readLink column is the one change that makes the skip live, and it
    /// fails this assertion at exactly that moment.
    #[test]
    fn va_link_only_is_default_which_is_why_the_authselect_skip_is_unreachable() {
        assert_eq!(
            classify_rpm_va_kind("....L...."),
            ConfigFileKind::RpmOwnedDefault
        );
    }

    #[test]
    fn test_config_inspector_emits_progress_events() {
        use inspectah_core::traits::progress::VecProgress;
        use inspectah_core::types::progress::{MetricKind, ProgressEvent, StepId};

        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["app.conf"])
            .with_file("/etc/app.conf", "setting=value\n")
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let progress = VecProgress::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &progress);
        assert!(result.is_ok(), "inspect should succeed");

        let events = progress.events();

        // Verify the three step-started events appear in order
        let step_ids: Vec<&StepId> = events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::StepStarted { step, .. } => Some(step),
                _ => None,
            })
            .collect();
        assert_eq!(
            step_ids,
            &[
                &StepId::ApplyingRpmVerification,
                &StepId::WalkingFilesystem,
                &StepId::ClassifyingConfigs,
            ]
        );

        // Verify each step has a matching StepFinished
        let finished_ids: Vec<&StepId> = events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::StepFinished { step, .. } => Some(step),
                _ => None,
            })
            .collect();
        assert_eq!(
            finished_ids,
            &[
                &StepId::ApplyingRpmVerification,
                &StepId::WalkingFilesystem,
                &StepId::ClassifyingConfigs,
            ]
        );

        // Verify ConfigsModified metric is emitted
        let has_metric = events.iter().any(|e| {
            matches!(
                e,
                ProgressEvent::Metric {
                    kind: MetricKind::ConfigsModified,
                    ..
                }
            )
        });
        assert!(has_metric, "should emit ConfigsModified metric");
    }

    // ---- Symlink-aware ownership tests ----

    #[test]
    fn test_is_rpm_owned_via_symlink_resolves_to_owned_path() {
        // /etc/ssl/certs/ca-bundle.crt -> /etc/pki/tls/certs/ca-bundle.crt
        // RPM owns the target, not the symlink.
        let exec = MockExecutor::new().with_link(
            "/etc/ssl/certs/ca-bundle.crt",
            "/etc/pki/tls/certs/ca-bundle.crt",
        );

        let rpm_state = rpm_state_with_va_and_owned(
            vec![],
            vec!["/etc/pki/tls/certs/ca-bundle.crt"],
            vec![PackageEntry {
                name: "ca-certificates".into(),
                version: "2023.2.60".into(),
                state: PackageState::Added,
                ..Default::default()
            }],
        );

        assert!(
            is_rpm_owned_via_symlink(&exec, &rpm_state, "/etc/ssl/certs/ca-bundle.crt"),
            "symlink to RPM-owned path should be detected as owned"
        );
    }

    #[test]
    fn test_is_rpm_owned_via_symlink_not_a_symlink() {
        // Regular file, not a symlink — should return false
        let exec =
            MockExecutor::new().with_file("/etc/httpd/conf/httpd.conf", "ServerRoot /etc/httpd");

        let rpm_state = rpm_state_with_va_and_owned(
            vec![],
            vec!["/etc/httpd/conf/httpd.conf"],
            vec![PackageEntry {
                name: "httpd".into(),
                version: "2.4.57".into(),
                state: PackageState::Added,
                ..Default::default()
            }],
        );

        assert!(
            !is_rpm_owned_via_symlink(&exec, &rpm_state, "/etc/httpd/conf/httpd.conf"),
            "non-symlink should not trigger symlink ownership check"
        );
    }

    #[test]
    fn test_is_rpm_owned_via_symlink_resolves_to_unowned() {
        // Symlink to a path that is also not RPM-owned
        let exec = MockExecutor::new().with_link("/etc/custom/link.conf", "/etc/custom/real.conf");

        let rpm_state = empty_rpm_state();

        assert!(
            !is_rpm_owned_via_symlink(&exec, &rpm_state, "/etc/custom/link.conf"),
            "symlink to unowned path should return false"
        );
    }

    #[test]
    fn test_symlinked_dir_files_not_in_unowned_output() {
        // Integration test: /etc/httpd/modules is a directory symlink to
        // /usr/lib64/httpd/modules. Files under it should NOT appear as
        // unowned in the inspector output.
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["httpd"])
            .with_dir("/etc/httpd", vec!["conf", "modules"])
            .with_dir("/etc/httpd/conf", vec!["httpd.conf"])
            // modules dir is a symlink outside /etc
            .with_dir("/etc/httpd/modules", vec!["mod_ssl.so", "mod_proxy.so"])
            .with_link("/etc/httpd/modules", "/usr/lib64/httpd/modules")
            .with_file("/etc/httpd/conf/httpd.conf", "ServerRoot /etc/httpd")
            .with_file("/etc/httpd/modules/mod_ssl.so", "<binary>")
            .with_file("/etc/httpd/modules/mod_proxy.so", "<binary>")
            .with_command(
                "rpm -Va",
                ExecResult {
                    stdout: "S.5....T.  c /etc/httpd/conf/httpd.conf\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1, // dnf not available
                    ..Default::default()
                },
            );

        let rpm_state = rpm_state_with_va_and_owned(
            vec![RpmVaEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                flags: "S.5....T.".into(),
                package: Some("httpd".into()),
            }],
            vec!["/etc/httpd/conf/httpd.conf"],
            vec![PackageEntry {
                name: "httpd".into(),
                version: "2.4.57".into(),
                state: PackageState::Added,
                ..Default::default()
            }],
        );

        let ctx = InspectionContext {
            source_system: &test_source_system(),
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let inspector = ConfigInspector::new();
        let progress = NullProgress;
        let result = inspector.inspect(&ctx, &progress);

        let section = match result {
            Ok(output) => match output.section {
                SectionData::Config(s) => s,
                _ => panic!("expected Config section"),
            },
            Err(InspectorError::Degraded { partial, .. }) => match partial.section {
                SectionData::Config(s) => s,
                _ => panic!("expected Config section"),
            },
            Err(e) => panic!("unexpected error: {e:?}"),
        };

        // httpd.conf should be captured as modified
        assert!(
            section.files.iter().any(|f| f.path.contains("httpd.conf")),
            "httpd.conf should be in output"
        );

        // .so files should NOT appear — the directory symlink is skipped
        assert!(
            !section.files.iter().any(|f| f.path.contains("mod_ssl")),
            "mod_ssl.so should not appear (directory symlink outside /etc)"
        );
        assert!(
            !section.files.iter().any(|f| f.path.contains("mod_proxy")),
            "mod_proxy.so should not appear (directory symlink outside /etc)"
        );
    }

    // ---- Cross-tree symlink advisory tests ----

    #[test]
    fn test_cross_tree_symlink_etc_to_var_emits_advisory() {
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["mydb"])
            .with_dir("/etc/mydb", vec!["config"])
            .with_link("/etc/mydb/config", "/var/lib/mydb/config")
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::Config(ref section) = output.section {
            let entry = section
                .files
                .iter()
                .find(|f| f.path.contains("mydb/config"))
                .expect("should find cross-tree symlink entry");
            assert!(
                entry.disposition.is_advisory(),
                "cross-tree symlink should be advisory"
            );
            match &entry.disposition {
                FindingKind::Advisory {
                    advisory_type,
                    rationale,
                } => {
                    assert_eq!(*advisory_type, AdvisoryType::CrossTreeSymlink);
                    assert!(
                        rationale.contains("/etc \u{2192} /var"),
                        "rationale should mention /etc \u{2192} /var, got: {rationale}"
                    );
                }
                _ => panic!("expected Advisory disposition"),
            }
        } else {
            panic!("expected Config section");
        }
    }

    #[test]
    fn test_allowlisted_symlink_skipped() {
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["localtime", "custom.conf"])
            .with_link("/etc/localtime", "/usr/share/zoneinfo/UTC")
            .with_file("/etc/custom.conf", "custom content")
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::Config(ref section) = output.section {
            assert!(
                !section.files.iter().any(|f| f.path.contains("localtime")),
                "allowlisted symlink should be suppressed"
            );
            assert!(
                section.files.iter().any(|f| f.path.contains("custom.conf")),
                "non-symlink file should still appear"
            );
        } else {
            panic!("expected Config section");
        }
    }

    #[test]
    fn test_broken_symlink_emits_advisory() {
        // Symlink loop simulates a broken resolve (resolve_final_target fails)
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["broken-link"])
            .with_link("/etc/broken-link", "/etc/broken-link")
            .with_command(
                "dnf history list --reverse",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ConfigInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::Config(ref section) = output.section {
            let entry = section
                .files
                .iter()
                .find(|f| f.path.contains("broken-link"))
                .expect("should find broken symlink entry");
            assert!(
                entry.disposition.is_advisory(),
                "broken symlink should be advisory"
            );
            match &entry.disposition {
                FindingKind::Advisory { rationale, .. } => {
                    assert!(
                        rationale.contains("Broken"),
                        "rationale should mention broken, got: {rationale}"
                    );
                }
                _ => panic!("expected Advisory disposition"),
            }
        } else {
            panic!("expected Config section");
        }
    }
}
