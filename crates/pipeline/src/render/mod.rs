//! Renderer module — produces all output artifacts from an InspectionSnapshot.
//!
//! Produces 8 artifacts:
//!
//! 1. Containerfile — image build definition
//! 2. audit-report.html — minimal PatternFly HTML report
//! 3. audit-report.md — findings and recommendations
//! 4. secrets-review.md — redaction details
//! 5. README.md — summary with build commands
//! 6. kickstart-suggestion.ks — deploy-time settings
//! 7. inspection-snapshot.json — the snapshot itself (written by caller)
//! 8. config/ tree — config files to COPY into the image

pub mod advisory;
pub mod audit;
pub mod configtree;
pub mod containerfile;
pub mod kickstart;
pub mod readme;
pub mod report;
pub mod safety;
pub mod secrets;
pub mod selinux;
pub mod tarball;
pub mod var_dirs;

pub mod users;

pub mod baseline_fmt;
pub mod report_data;

pub mod language_packages;
pub mod service_intent;

pub mod repo_enabling;
pub mod repoless;
pub mod unmanaged;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::{RenderContext, RenderError};
use std::path::Path;

/// Write all 8 output artifacts to the given directory.
///
/// The snapshot JSON itself is written here as well (artifact 7).
/// Config tree writing (artifact 8) is deferred to Task 24.
/// Where the Storage block's `COPY` expects the `/var` provisioning
/// fragment to be, relative to the output directory.
const VAR_TMPFILES_DIR: &str = "config/usr/lib/tmpfiles.d";
const VAR_TMPFILES_FILE: &str = "inspectah-var.conf";

/// Stage the `/var` provisioning fragment, when the snapshot produces one.
///
/// **Every path that renders a Containerfile must call this first.**
/// `var_dir_section_lines` emits
/// `COPY config/usr/lib/tmpfiles.d/inspectah-var.conf …` under exactly the
/// condition this writes under — `render_tmpfiles_conf` returning `Some` —
/// so an output set that has the one and not the other fails at that line
/// under `podman build`, with nothing said before then. That is what
/// happened: this was inlined here and the refine export, which builds its
/// own file set, had no equivalent step.
pub fn write_tmpfiles_conf(
    snap: &InspectionSnapshot,
    output_dir: &Path,
) -> Result<(), RenderError> {
    let Some(content) = containerfile::render_tmpfiles_conf(snap) else {
        return Ok(());
    };
    let dir = output_dir.join(VAR_TMPFILES_DIR);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(VAR_TMPFILES_FILE), content)?;
    Ok(())
}

pub fn render_all(
    snap: &InspectionSnapshot,
    context: &RenderContext,
    output_dir: &Path,
) -> Result<(), RenderError> {
    std::fs::create_dir_all(output_dir)?;

    // 8. config/ tree — materialize FIRST so the Containerfile can use
    //    the actual directory list for its COPY lines (single source of truth).
    let materialized_roots = configtree::write_config_tree(snap, output_dir)?;

    // 8b. env-files/ — .env files go to a separate directory (not config/)
    configtree::write_env_files(snap, output_dir)?;

    // 8c. tmpfiles.d — /var directory provisioning (staged before Containerfile
    //     rendering so the COPY line references an actual file)
    write_tmpfiles_conf(snap, output_dir)?;

    // 8d. language-packages/ — pip/npm/gem manifests the Containerfile's
    //     language sections COPY. Staged on the same condition the
    //     renderer emits those lines under.
    language_packages::write_language_package_manifests(snap, output_dir)?;

    // 1. Containerfile — COPY lines derived from materialized config tree roots
    let containerfile = containerfile::render_containerfile(snap, Some(&materialized_roots), None);
    std::fs::write(output_dir.join("Containerfile"), containerfile)?;

    // 2. audit-report.html
    let html = report::render_report(snap, context);
    std::fs::write(output_dir.join("audit-report.html"), html)?;

    // 3. audit-report.md
    let audit = audit::render_audit(snap);
    std::fs::write(output_dir.join("audit-report.md"), audit)?;

    // 4. secrets-review.md
    let secrets = secrets::render_secrets_review(snap);
    std::fs::write(output_dir.join("secrets-review.md"), secrets)?;

    // 5. README.md
    let readme = readme::render_readme(snap);
    std::fs::write(output_dir.join("README.md"), readme)?;

    // 6. kickstart-suggestion.ks
    let ks = kickstart::render_kickstart(snap);
    std::fs::write(output_dir.join("kickstart-suggestion.ks"), ks)?;

    // 6b. inspectah-users.ks — user/group kickstart fragment
    let users_ks = users::render_kickstart(snap);
    if !users_ks.is_empty() {
        std::fs::write(output_dir.join("inspectah-users.ks"), users_ks)?;
    }

    // 6c. inspectah-users.toml — user/group blueprint TOML fragment
    let users_toml = users::render_blueprint_toml(snap);
    if !users_toml.is_empty() {
        std::fs::write(output_dir.join("inspectah-users.toml"), users_toml)?;
    }

    // 6d. users/ — SSH key staging
    users::stage_ssh_keys(snap, output_dir)
        .map_err(|e| RenderError::Failed(format!("stage SSH keys: {e}")))?;

    // 7. inspection-snapshot.json
    let json = serde_json::to_string_pretty(snap)
        .map_err(|e| RenderError::Failed(format!("serialize snapshot: {e}")))?;
    std::fs::write(output_dir.join("inspection-snapshot.json"), json)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
    use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
    use tempfile::TempDir;

    fn test_snapshot() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                version: "2.4.57".into(),
                release: "5.el9".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                source_repo: "appstream".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap
    }

    #[test]
    fn test_render_all_produces_all_artifacts() {
        let snap = test_snapshot();
        let context = RenderContext { target: None };
        let dir = TempDir::new().unwrap();

        render_all(&snap, &context, dir.path()).unwrap();

        // Verify all 8 artifacts exist
        assert!(
            dir.path().join("Containerfile").exists(),
            "Containerfile missing"
        );
        assert!(
            dir.path().join("audit-report.html").exists(),
            "audit-report.html missing"
        );
        assert!(
            dir.path().join("audit-report.md").exists(),
            "audit-report.md missing"
        );
        assert!(
            dir.path().join("secrets-review.md").exists(),
            "secrets-review.md missing"
        );
        assert!(dir.path().join("README.md").exists(), "README.md missing");
        assert!(
            dir.path().join("kickstart-suggestion.ks").exists(),
            "kickstart-suggestion.ks missing"
        );
        assert!(
            dir.path().join("inspection-snapshot.json").exists(),
            "inspection-snapshot.json missing"
        );
        assert!(
            dir.path().join("config").exists(),
            "config/ directory missing"
        );
    }

    // ---------------------------------------------------------------
    // The unvisited-field guard.
    //
    // Findings 1, 7, 8 and 9 were not parsing bugs. Each was a
    // content-bearing field that nobody wired into `redact()`, and each
    // shipped a credential into the export with the snapshot stamped
    // `FullyRedacted` and no finding raised. A principle in a skill file
    // did not prevent the second, third and fourth of them. These two
    // tests are the thing that does.
    // ---------------------------------------------------------------

    /// Every field of every content-bearing snapshot type, classified.
    ///
    /// **This test exists to fail at compile time.** None of the patterns
    /// below uses `..`, so adding a field to any of these structs stops
    /// the build here until someone writes it into the right group. That
    /// forces the decision the four findings were each missing: does this
    /// new field carry host content, and if so who scrubs it?
    ///
    /// The runtime half is `no_content_bearing_field_survives_the_export`
    /// below, which proves the `scrubbed` group is wired rather than just
    /// declared. This half proves the groups are complete.
    #[test]
    fn every_field_of_a_content_bearing_type_is_classified() {
        use inspectah_core::types::containers::{ComposeFile, QuadletUnit};
        use inspectah_core::types::network::FirewallZone;
        use inspectah_core::types::rpm::RepoFile;
        use inspectah_core::types::scheduled::{AtJob, GeneratedTimerUnit, SystemdTimer};
        use inspectah_core::types::services::SystemdDropIn;

        // `config.files[]` and `non_rpm_software.env_files[]`.
        let ConfigFileEntry {
            content: _,           // scrubbed: config pass
            path: _,              // identifier
            kind: _,              // classification
            category: _,          // classification
            rpm_va_flags: _,      // rpm verify flags
            package: _,           // owning package name
            diff_against_rpm: _,  // exempt: always None, deferred to Phase 3
            disposition: _,       // decision
            locked: _,            // decision
            variant_selection: _, // decision
            aggregate: _,         // fleet counts
            attention_reason: _,  // generated prose
            shadowed_by: _,       // cross-inspector link
        } = ConfigFileEntry::default();

        // `rpm.repo_files[]` and `rpm.gpg_keys[]`.
        let RepoFile {
            content: _, // scrubbed: repo-file pass
            path: _,
            is_default_repo: _,
            disposition: _,
            locked: _,
            aggregate: _,
        } = RepoFile::default();

        // `services.drop_ins[]`.
        let SystemdDropIn {
            content: _, // scrubbed: drop-in pass
            unit: _,
            path: _,
            disposition: _,
            locked: _,
            variant_selection: _,
            aggregate: _,
            attention_reason: _,
            shadow_type: _,
            shadow_rationale: _,
        } = SystemdDropIn::default();

        // `scheduled_tasks.generated_timer_units[]`.
        let GeneratedTimerUnit {
            command: _,         // scrubbed
            service_content: _, // scrubbed
            timer_content: _,   // scrubbed (Finding 9)
            name: _,
            cron_expr: _, // schedule expression, no value field
            source_path: _,
            disposition: _,
            locked: _,
            aggregate: _,
        } = GeneratedTimerUnit::default();

        // `scheduled_tasks.systemd_timers[]`.
        let SystemdTimer {
            exec_start: _,      // scrubbed
            service_content: _, // scrubbed
            timer_content: _,   // scrubbed (Finding 9)
            name: _,
            on_calendar: _, // schedule expression
            description: _, // scrubbed: the one free-text line a .timer has
            source: _,
            path: _,
            disposition: _,
            locked: _,
            aggregate: _,
        } = SystemdTimer::default();

        // `scheduled_tasks.at_jobs[]`.
        let AtJob {
            command: _, // scrubbed
            file: _,
            user: _,
            working_dir: _,
            disposition: _,
            locked: _,
            aggregate: _,
        } = AtJob::default();

        // `containers.quadlet_units[]`.
        let QuadletUnit {
            content: _, // scrubbed (Finding 8) — reaches the built image
            path: _,
            name: _,
            image: _, // image reference; a registry credential would be
            // in the pull secret, not here
            disposition: _,
            locked: _,
            variant_selection: _,
            aggregate: _,
            ports: _,
            volumes: _,
            generated: _,
        } = QuadletUnit::default();

        // `containers.compose_files[]`.
        let ComposeFile {
            raw_content: _, // scrubbed (Finding 7)
            path: _,
            images: _, // service/image names, parsed structure
            disposition: _,
            locked: _,
            variant_selection: _,
            aggregate: _,
        } = ComposeFile::default();

        // `network.firewall_zones[]`.
        let FirewallZone {
            // EXEMPT, deliberately, and pinned by SENTINEL-FIREWALL in
            // the sweep below. firewalld zone XML has no field that
            // holds a credential: services, ports and rich rules are
            // addresses and protocol names. Revisit if a zone ever
            // carries an arbitrary value.
            content: _,
            path: _,
            name: _,
            services: _,
            ports: _,
            rich_rules: _,
            disposition: _,
            locked: _,
            aggregate: _,
        } = FirewallZone::default();

        // `non_rpm_software.items[]`.
        let NonRpmItem {
            manifest_files: _, // scrubbed (Finding 1)
            git_remote: _,     // scrubbed
            // EXEMPT, pinned by SENTINEL-NRSCONTENT in the sweep below:
            // no collector writes this field as of this commit. If one
            // starts, it holds file content and must be scrubbed, and
            // the sweep says so when it stops surviving.
            content: _,
            notes: _, // operator-authored in refine, not host content
            path: _,
            name: _,
            method: _,
            confidence: _,
            disposition: _,
            locked: _,
            acknowledged: _,
            lang: _,
            r#static: _, // ELF linkage, not content
            version: _,
            shared_libs: _,
            system_site_packages: _,
            packages: _,
            has_c_extensions: _,
            git_commit: _,
            git_branch: _,
            rpm_filtered: _,
            files: _,
            aggregate: _,
            review_status: _,
        } = NonRpmItem::default();
    }

    /// Plant a distinct secret in every content-bearing field and assert
    /// that the set which survives the export is **exactly** the set
    /// declared exempt above.
    ///
    /// Both directions matter, and the second one is not decoration. A
    /// field declared `scrubbed` that survives is a leak: this would have
    /// failed for `manifest_files`, `raw_content`,
    /// `quadlet_units[].content` and both `timer_content` fields. A field
    /// declared exempt that stops surviving means somebody wired it in
    /// and the classification above is now stale.
    ///
    /// Asserting only the first direction is what let
    /// `SystemdTimer.description` through: it was classified exempt on
    /// the reasoning that a `Description=` is prose rather than a value,
    /// nothing planted a secret in it, and a real scan then put one in
    /// the snapshot and in the rendered audit report. An exemption that
    /// nothing exercises is an assumption, not a decision.
    #[test]
    fn exactly_the_exempt_fields_survive_the_export() {
        use inspectah_core::types::config::ConfigFileKind;
        use inspectah_core::types::containers::{ComposeFile, ContainerSection, QuadletUnit};
        use inspectah_core::types::scheduled::{
            AtJob, GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer,
        };
        use inspectah_core::types::services::{ServiceSection, SystemdDropIn};

        // Each sentinel is planted behind a keyword the pattern set
        // recognises, so a visited field redacts it and an unvisited one
        // ships it verbatim. The sentinel itself is what the sweep greps
        // for, so it must not look like anything a renderer emits.
        const SCRUBBED: &[&str] = &[
            "SENTINEL-CONFIG",
            "SENTINEL-REPOFILE",
            "SENTINEL-GPGKEY",
            "SENTINEL-DROPIN",
            "SENTINEL-ENVFILE",
            "SENTINEL-GENTIMER-SERVICE",
            "SENTINEL-GENTIMER-TIMER",
            "SENTINEL-GENTIMER-COMMAND",
            "SENTINEL-SYSTIMER-SERVICE",
            "SENTINEL-SYSTIMER-TIMER",
            "SENTINEL-SYSTIMER-EXEC",
            "SENTINEL-SYSTIMER-DESC",
            "SENTINEL-ATJOB",
            "SENTINEL-QUADLET",
            "SENTINEL-COMPOSE",
            "SENTINEL-MANIFEST",
            "SENTINEL-GITREMOTE",
        ];

        // Declared exempt, and planted anyway so the exemption is a
        // measured decision rather than an untested assumption. Each
        // one's reason is recorded beside its field in
        // `every_field_of_a_content_bearing_type_is_classified`.
        const EXEMPT: &[&str] = &[
            "SENTINEL-FIREWALL",   // firewalld zone XML holds no credential
            "SENTINEL-NRSCONTENT", // no collector writes NonRpmItem.content
        ];

        let mut snap = test_snapshot();

        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/myapp/app.conf".into(),
                kind: ConfigFileKind::Unowned,
                content: "password=SENTINEL-CONFIG\n".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
        });

        if let Some(ref mut rpm) = snap.rpm {
            rpm.repo_files = vec![inspectah_core::types::rpm::RepoFile {
                path: "/etc/yum.repos.d/corp.repo".into(),
                content: "[corp]\npassword=SENTINEL-REPOFILE\n".into(),
                ..Default::default()
            }];
            rpm.gpg_keys = vec![inspectah_core::types::rpm::RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-corp".into(),
                content: "password=SENTINEL-GPGKEY\n".into(),
                ..Default::default()
            }];
        }

        snap.services = Some(ServiceSection {
            drop_ins: vec![SystemdDropIn {
                unit: "app.service".into(),
                path: "/etc/systemd/system/app.service.d/override.conf".into(),
                content: "[Service]\nEnvironment=DB_PASSWORD=SENTINEL-DROPIN\n".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });

        snap.scheduled_tasks = Some(ScheduledTaskSection {
            generated_timer_units: vec![GeneratedTimerUnit {
                name: "backup".into(),
                source_path: "/etc/cron.d/backup".into(),
                command: "/usr/bin/backup --password=SENTINEL-GENTIMER-COMMAND".into(),
                service_content: "[Service]\nEnvironment=DB_PASSWORD=SENTINEL-GENTIMER-SERVICE\n"
                    .into(),
                // A `.timer` has no ExecStart= or Environment=, so a
                // secret in one realistically rides on Description=.
                // Not a comment line: comment lines are exempt from
                // scanning on every field, by design.
                timer_content:
                    "[Timer]\nOnCalendar=daily\nDescription=sync api_key=SENTINEL-GENTIMER-TIMER\n"
                        .into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            systemd_timers: vec![SystemdTimer {
                name: "report".into(),
                description: "report job api_key=SENTINEL-SYSTIMER-DESC".into(),
                source: "local".into(),
                path: "/etc/systemd/system/report.timer".into(),
                exec_start: "/usr/bin/report --password=SENTINEL-SYSTIMER-EXEC".into(),
                service_content: "[Service]\nEnvironment=API_TOKEN=SENTINEL-SYSTIMER-SERVICE\n"
                    .into(),
                timer_content:
                    "[Timer]\nOnCalendar=daily\nDescription=sync api_key=SENTINEL-SYSTIMER-TIMER\n"
                        .into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            at_jobs: vec![AtJob {
                file: "/var/spool/at/a0001".into(),
                command: "/usr/bin/sync --password=SENTINEL-ATJOB".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });

        snap.containers = Some(ContainerSection {
            quadlet_units: vec![QuadletUnit {
                path: "/etc/containers/systemd/app.container".into(),
                name: "app.container".into(),
                content: "[Container]\nImage=registry.example.com/app\n\
                          Environment=DB_PASSWORD=SENTINEL-QUADLET\n"
                    .into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            compose_files: vec![ComposeFile {
                path: "opt/myapp/docker-compose.yml".into(),
                raw_content: Some(
                    "services:\n  db:\n    environment:\n      \
                     POSTGRES_PASSWORD: SENTINEL-COMPOSE\n"
                        .into(),
                ),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });

        let mut manifest_files = std::collections::HashMap::new();
        manifest_files.insert(
            "package-lock.json".to_string(),
            "{\n  \"resolved\": \"https://u:SENTINEL-MANIFEST@npm.corp.example.com/x.tgz\"\n}\n"
                .to_string(),
        );
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                path: "opt/webapp".into(),
                name: "webapp".into(),
                content: "password=SENTINEL-NRSCONTENT\n".into(),
                method: inspectah_core::util::METHOD_NPM_LOCKFILE.into(),
                git_remote: "https://u:SENTINEL-GITREMOTE@git.corp.example.com/webapp.git".into(),
                manifest_files,
                ..Default::default()
            }],
            env_files: vec![ConfigFileEntry {
                path: "opt/webapp/.env".into(),
                content: "DB_PASSWORD=SENTINEL-ENVFILE\n".into(),
                ..Default::default()
            }],
        });

        snap.network = Some(inspectah_core::types::network::NetworkSection {
            firewall_zones: vec![inspectah_core::types::network::FirewallZone {
                path: "/etc/firewalld/zones/corp.xml".into(),
                name: "corp".into(),
                content: "<zone>\n  <!-- password=SENTINEL-FIREWALL -->\n</zone>\n".into(),
                ..Default::default()
            }],
            ..Default::default()
        });

        // Every sentinel must actually be in the snapshot before
        // redaction, or the sweep passes for the wrong reason.
        let before = serde_json::to_string(&snap).unwrap();
        for s in SCRUBBED.iter().chain(EXEMPT) {
            assert!(before.contains(s), "fixture never planted {s}");
        }

        crate::redaction::engine::redact(&mut snap, &Default::default());

        let context = RenderContext { target: None };
        let dir = TempDir::new().unwrap();
        render_all(&snap, &context, dir.path()).unwrap();

        let mut files = Vec::new();
        walk_files(dir.path(), &mut files);
        let mut survived: Vec<String> = Vec::new();
        for s in SCRUBBED.iter().chain(EXEMPT) {
            let found: Vec<String> = files
                .iter()
                .filter(|p| std::fs::read_to_string(p).is_ok_and(|c| c.contains(*s)))
                .map(|p| {
                    p.strip_prefix(dir.path())
                        .unwrap_or(p)
                        .display()
                        .to_string()
                })
                .collect();
            if !found.is_empty() {
                survived.push(format!("{s} in {}", found.join(", ")));
            }
        }

        let leaked: Vec<&String> = survived
            .iter()
            .filter(|line| SCRUBBED.iter().any(|s| line.starts_with(s)))
            .collect();
        assert!(
            leaked.is_empty(),
            "fields declared scrubbed reached the export unredacted:\n  {}",
            leaked
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n  ")
        );

        for s in EXEMPT {
            assert!(
                survived.iter().any(|line| line.starts_with(s)),
                "{s} is declared exempt but no longer reaches the export. \
                 Something now scrubs it: move it to the scrubbed group."
            );
        }
    }

    /// Recursively collect every file written under `dir`.
    fn walk_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk_files(&path, out);
            } else {
                out.push(path);
            }
        }
    }

    /// The export is only as redacted as its leakiest artifact, so this
    /// asserts over every file under the output directory rather than over
    /// one named file. The leak this replaces was a sidecar that scrubbed
    /// correctly next to an `inspection-snapshot.json` that did not.
    #[test]
    fn no_export_artifact_carries_a_manifest_credential() {
        const FIXTURE_CREDENTIAL: &str = "s3cret";

        let mut manifest_files = std::collections::HashMap::new();
        manifest_files.insert(
            "package-lock.json".to_string(),
            format!(
                "{{\n  \"packages\": {{\n    \"node_modules/x\": {{\n      \"resolved\": \"https://deploy:{FIXTURE_CREDENTIAL}@npm.corp.example.com/x.tgz\"\n    }}\n  }}\n}}\n"
            ),
        );
        let mut snap = test_snapshot();
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                path: "/srv/web".into(),
                name: "webapp".into(),
                method: inspectah_core::util::METHOD_NPM_LOCKFILE.into(),
                manifest_files,
                ..Default::default()
            }],
            ..Default::default()
        });

        crate::redaction::engine::redact(&mut snap, &Default::default());

        let context = RenderContext { target: None };
        let dir = TempDir::new().unwrap();
        render_all(&snap, &context, dir.path()).unwrap();

        let mut files = Vec::new();
        walk_files(dir.path(), &mut files);
        assert!(!files.is_empty(), "export produced no artifacts");

        for path in &files {
            let Ok(content) = std::fs::read_to_string(path) else {
                continue; // binary artifact, nothing to match
            };
            assert!(
                !content.contains(FIXTURE_CREDENTIAL),
                "{} carries the fixture credential",
                path.display()
            );
        }
    }

    #[test]
    fn test_render_all_artifacts_non_empty() {
        let snap = test_snapshot();
        let context = RenderContext { target: None };
        let dir = TempDir::new().unwrap();

        render_all(&snap, &context, dir.path()).unwrap();

        let files = &[
            "Containerfile",
            "audit-report.html",
            "audit-report.md",
            "secrets-review.md",
            "README.md",
            "kickstart-suggestion.ks",
            "inspection-snapshot.json",
        ];

        for name in files {
            let content = std::fs::read_to_string(dir.path().join(name)).unwrap();
            assert!(!content.is_empty(), "{} must be non-empty", name);
        }
    }

    /// Verify that every directory under config/ has a matching COPY line
    /// in the Containerfile, and vice versa: no COPY line references a
    /// directory that doesn't exist in config/. This is the core desync
    /// invariant — the Containerfile and config tree describe the same system.
    #[test]
    fn test_containerfile_copy_lines_match_config_tree() {
        let mut snap = test_snapshot();
        snap.config = Some(ConfigSection {
            files: vec![
                ConfigFileEntry {
                    path: "/etc/httpd/conf/httpd.conf".into(),
                    content: "ServerRoot /etc/httpd".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                ConfigFileEntry {
                    path: "/usr/lib/sysctl.d/99-custom.conf".into(),
                    content: "net.ipv4.ip_forward = 1".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
            ],
        });

        let context = RenderContext { target: None };
        let dir = TempDir::new().unwrap();
        render_all(&snap, &context, dir.path()).unwrap();

        // Collect actual top-level dirs under config/
        let config_dir = dir.path().join("config");
        let mut actual_dirs: Vec<String> = std::fs::read_dir(&config_dir)
            .unwrap()
            .filter_map(|e| {
                let e = e.ok()?;
                if e.path().is_dir() {
                    Some(e.file_name().to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect();
        actual_dirs.sort();

        // Parse COPY lines from Containerfile for config/ entries
        let containerfile = std::fs::read_to_string(dir.path().join("Containerfile")).unwrap();
        let mut copy_roots: Vec<String> = Vec::new();
        for line in containerfile.lines() {
            // Match "COPY config/XYZ/ /XYZ/" pattern
            if let Some(rest) = line.strip_prefix("COPY config/")
                && let Some(root) = rest.split('/').next()
                && !root.is_empty()
                && !copy_roots.contains(&root.to_string())
            {
                copy_roots.push(root.to_string());
            }
        }
        copy_roots.sort();

        // Every config dir must have a COPY line
        for dir_name in &actual_dirs {
            assert!(
                copy_roots.contains(dir_name),
                "config/{dir_name}/ exists but has no COPY line in Containerfile"
            );
        }

        // Every COPY line must reference a directory that exists
        for root in &copy_roots {
            assert!(
                actual_dirs.contains(root),
                "Containerfile has COPY config/{root}/ but no such directory exists"
            );
        }

        // Sanity: both should contain etc and usr
        assert!(
            actual_dirs.contains(&"etc".to_string()),
            "etc must be materialized"
        );
        assert!(
            actual_dirs.contains(&"usr".to_string()),
            "usr must be materialized"
        );
    }

    /// Verify that a snapshot with no config files produces no COPY
    /// lines for config/ directories — both the tree and Containerfile
    /// should agree on "nothing to copy".
    #[test]
    fn test_empty_config_no_copy_lines() {
        let snap = test_snapshot(); // has RPM but no config
        let context = RenderContext { target: None };
        let dir = TempDir::new().unwrap();
        render_all(&snap, &context, dir.path()).unwrap();

        let containerfile = std::fs::read_to_string(dir.path().join("Containerfile")).unwrap();
        let config_copy_lines: Vec<&str> = containerfile
            .lines()
            .filter(|l| {
                l.starts_with("COPY config/")
                    && !l.contains("yum.repos.d")
                    && !l.contains("pki/rpm-gpg")
            })
            .collect();
        assert!(
            config_copy_lines.is_empty(),
            "no config files -> no COPY config/ lines, got: {:?}",
            config_copy_lines
        );
    }
}
