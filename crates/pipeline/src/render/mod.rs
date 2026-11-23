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
pub mod authselect;
pub mod comment;
pub mod configtree;
pub mod containerfile;
pub mod kickstart;
pub mod machine_state;
pub mod md_literal;
pub mod readme;
pub mod report;
pub mod safety;
pub mod secrets;
pub mod selinux;
pub mod sysctl_report;
pub mod tarball;
pub mod tmpfiles_accounts;
pub mod var_dirs;

pub mod users;

pub mod baseline_fmt;
pub mod report_data;

pub mod language_packages;
pub mod not_carried;
pub mod unit_plan;

pub mod repo_enabling;
pub mod repo_owned;
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
    // 0. Machine-state validation before anything is written: a snapshot
    //    holding a credential, or a file row at a class path, is a defect
    //    and no export is produced. The string is the snapshot step 7
    //    writes.
    let snapshot_json = snap
        .to_validated_json()
        .map_err(|e| RenderError::Failed(e.to_string()))?;

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

    // 7. inspection-snapshot.json, validated at step 0
    std::fs::write(output_dir.join("inspection-snapshot.json"), snapshot_json)?;

    Ok(())
}

/// A sandbox for tests of export writers that take names from the
/// snapshot: `export/` is the root a writer is given, and
/// `victim/authorized_keys` is what an escape would hit.
#[cfg(test)]
pub(crate) mod victim_sandbox {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// The victim's bytes before any writer runs.
    pub(crate) const ORIGINAL: &[u8] = b"ORIGINAL";

    pub(crate) struct VictimSandbox {
        dir: tempfile::TempDir,
    }

    impl VictimSandbox {
        pub(crate) fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            std::fs::create_dir(dir.path().join("export")).expect("export dir");
            std::fs::create_dir(dir.path().join("victim")).expect("victim dir");
            std::fs::write(dir.path().join("victim/authorized_keys"), ORIGINAL)
                .expect("victim file");
            Self { dir }
        }

        pub(crate) fn root(&self) -> &Path {
            self.dir.path()
        }

        pub(crate) fn out(&self) -> PathBuf {
            self.root().join("export")
        }

        pub(crate) fn victim(&self) -> PathBuf {
            self.root().join("victim/authorized_keys")
        }

        /// Every path under the sandbox outside `export/`, without
        /// following links.
        pub(crate) fn outside(&self) -> BTreeSet<PathBuf> {
            walkdir::WalkDir::new(self.root())
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| e.path() != self.out())
                .map(|e| e.expect("walk").path().to_path_buf())
                .collect()
        }

        /// The victim still holds its original bytes and nothing appeared
        /// outside the export root since `before`.
        pub(crate) fn assert_contained(&self, before: &BTreeSet<PathBuf>, what: &str) {
            assert_eq!(
                std::fs::read(self.victim()).expect("read victim"),
                ORIGINAL,
                "{what}: the victim was written"
            );
            assert_eq!(
                &self.outside(),
                before,
                "{what}: something appeared outside the export root"
            );
        }
    }
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
        use inspectah_core::types::config::ConfigSection;
        use inspectah_core::types::containers::{
            ComposeFile, ContainerMount, ContainerSection, FlatpakApp, QuadletUnit,
            RunningContainer,
        };
        use inspectah_core::types::kernelboot::KernelBootSection;
        use inspectah_core::types::network::{
            FirewallDirectRule, FirewallZone, NetworkSection, ProxyEntry,
        };
        use inspectah_core::types::nonrpm::{
            LanguagePackage, NonRpmSoftwareSection, UnmanagedFile, UnmanagedFileSection,
        };
        use inspectah_core::types::rpm::{RepoFile, RpmSection};
        use inspectah_core::types::scheduled::ScheduledTaskSection;
        use inspectah_core::types::scheduled::{AtJob, GeneratedTimerUnit, SystemdTimer};
        use inspectah_core::types::selinux::{FcontextRule, SelinuxPortLabel, SelinuxSection};
        use inspectah_core::types::storage::StorageSection;
        use inspectah_core::types::storage::{FstabEntry, MountPoint};
        use inspectah_core::types::sysctl::{SysctlFile, SysctlFileBody, SysctlSection};
        use inspectah_core::types::units::UnitSection;
        use inspectah_core::types::units::{
            AlsoMember, DefaultTarget, ManualLink, MountUnitFinding, SystemdDropIn,
            UndeterminedInstall, UnitInventoryEntry,
        };
        use inspectah_core::types::users::UserGroupSection;

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
            gpgkey_references: _,   // identifier: the literal spelling of `path`
            shipped_by_base: _,     // identifier: an rpm package name
            verified_base_owner: _, // identifier: an rpm package name
            redaction_changed: _,   // identifier: section ids the scrubbed content keeps
            rejected: _,            // classification: a fixed enum
        } = RepoFile::default();

        // `units.drop_ins[]`.
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

        // `units.state_changes[]`: `UnitStateChange` keeps its unit and
        // state private, so this pattern cannot name them here. Its guard
        // is in the type's own module, `crates/core/src/types/units.rs`
        // (`every_field_of_a_unit_state_change_is_classified`), with the
        // same rule: no `..`, every field classified. The unit types nested
        // in it and in `MountUnitFinding` are classified field by field
        // there too (`every_field_of_a_nested_unit_type_is_classified`).

        // `units.state_changes[].also[]`: names and what the links say.
        fn _classify_also_member(member: AlsoMember) {
            let AlsoMember {
                unit: _,            // exempt: a proved unit name
                links: _,           // exempt: installed, unlinked, inert or other
                vendor_drop_ins: _, // identifiers: file names, no content
            } = member;
        }

        // `units.inventory[]`. No `Default`, so the pattern sits in a
        // function that is never called, as the sysctl ones below.
        fn _classify_unit_inventory_entry(entry: UnitInventoryEntry) {
            let UnitInventoryEntry {
                unit: _,   // exempt: a proved unit name
                state: _,  // exempt: enabled, disabled or masked
                preset: _, // exempt: enable or disable
            } = entry;
        }

        // `units.mount_findings[]`. There is no field for the mount source
        // (`What=`): it can hold a credential, and a copy here is one the
        // config pass never visits. Refine links the row to the unit
        // file's config entry, which the config pass scrubs.
        fn _classify_mount_unit_finding(finding: MountUnitFinding) {
            let MountUnitFinding {
                // exempt: a proved unit name, and the owning package's name.
                // Every field inside is classified in `units.rs`
                // (`every_field_of_a_nested_unit_type_is_classified`).
                unit: _,
                state_change: _, // exempt: a state and a preset
                // EXEMPT, pinned by SENTINEL-MOUNT-POINT in the sweep below:
                // an identifier, an absolute path from `Where=`.
                mount_point: _,
                nofail: _,    // exempt: an optional boolean from `Options=`
                aggregate: _, // fleet counts
            } = finding;
        }

        // `units.manual_links[]`. Names and a cause only: no content is
        // read into it, and its advisory is built from these fields and
        // fixed text (§ 6.7).
        fn _classify_manual_link(link: ManualLink) {
            let ManualLink {
                relation: _, // exempt: wants, requires, upholds, alias or also
                parent: _,   // exempt: a proved unit name
                unit: _,     // exempt: a proved unit name
                cause: _,    // exempt: a cause and, for one, a reason
                // identifiers: paths under /etc/systemd/system from a
                // directory listing, no content
                install_files: _,
                aggregate: _, // fleet counts
            } = link;
        }

        // `units.undetermined_installs[]`, the same: names, a reason and
        // paths.
        fn _classify_undetermined_install(record: UndeterminedInstall) {
            let UndeterminedInstall {
                unit: _,          // exempt: a proved unit name
                reason: _,        // exempt: why the section cannot be relied on
                install_files: _, // identifiers: paths from a directory listing
                aggregate: _,     // fleet counts
            } = record;
        }

        // `units.default_target`: a proved target and its toggle.
        fn _classify_default_target(target: DefaultTarget) {
            let DefaultTarget {
                target: _,      // exempt: a proved target name and its fragment
                disposition: _, // exempt: the toggle
                locked: _,      // exempt: a flag
                aggregate: _,   // fleet counts
            } = target;
        }

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
        // A description only (§ 8): the files are config entries, which
        // the config pass scrubs.
        let SystemdTimer {
            exec_start: _, // scrubbed
            name: _,
            on_calendar: _, // schedule expression
            description: _, // scrubbed: the one free-text line a .timer has
            source: _,
            path: _,
            trigger: _,          // identifier: a proved UnitName or `unresolved`
            trigger_drop_ins: _, // identifiers: drop-in paths, no content
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
            // scrubbed: the content pass, under the zone's path, as its
            // config entry is. A `<description>` and a comment are free
            // text, so "zone XML holds no credential" did not hold.
            content: _,
            path: _,
            name: _,
            services: _, // service names
            ports: _,    // port/protocol pairs
            // scrubbed: the URL pass; a `log prefix` is free text
            rich_rules: _,
            disposition: _,
            locked: _,
            aggregate: _,
        } = FirewallZone::default();

        // `network`: the rest of the section.
        let NetworkSection {
            connections: _,    // identifiers: path, id, method, type
            firewall_zones: _, // classified above
            // classified below
            firewall_direct_rules: _,
            static_routes: _,     // identifiers: path and name, no content
            ip_routes: _,         // `ip route` output: addresses and devices
            ip_rules: _,          // `ip rule` output: selectors and tables
            resolv_provenance: _, // classification
            hosts_additions: _,   // scrubbed: the URL pass; a line can end in a comment
            proxy: _,             // classified below
        } = NetworkSection::default();
        let FirewallDirectRule {
            ipv: _,
            table: _,
            chain: _,
            priority: _,
            args: _, // scrubbed: the URL pass; `--comment` is free text
            disposition: _,
            locked: _,
        } = FirewallDirectRule::default();
        let ProxyEntry {
            source: _, // identifier: the file path
            line: _,   // scrubbed: the URL pass, passwords and token user names
        } = ProxyEntry::default();

        // `storage`.
        let FstabEntry {
            device: _,      // scrubbed: the URL pass; a source can be a URL
            options: _,     // scrubbed: `password=` rule and the URL pass
            mount_point: _, // identifier: a path
            fstype: _,
            disposition: _,
            locked: _,
            acknowledged: _,
            aggregate: _,
            attention_reason: _, // generated prose
        } = FstabEntry::default();
        let MountPoint {
            source: _,  // scrubbed: the URL pass
            options: _, // scrubbed: `password=` rule and the URL pass
            target: _,  // identifier: a path
            fstype: _,
        } = MountPoint::default();

        // `containers.flatpak_apps[]`.
        let FlatpakApp {
            remote_url: _, // scrubbed: the URL pass; reaches the provisioning service
            app_id: _,
            origin: _,
            branch: _,
            remote: _,
            disposition: _,
            locked: _,
            aggregate: _,
        } = FlatpakApp::default();

        // `kernel_boot`.
        let KernelBootSection {
            // scrubbed: patterns and URL credentials, argument by argument
            cmdline: _,
            // scrubbed: the content pass; no collector writes it today
            grub_defaults: _,
            sysctl: _,         // classified below
            loaded_modules: _, // `lsmod`: names, sizes, users
            tuned_active: _,   // a profile name
            tuned_disposition: _,
            locale: _,       // a locale name
            timezone: _,     // a zone name
            alternatives: _, // `alternatives`: names and paths
        } = KernelBootSection::default();

        // `users_groups`.
        let UserGroupSection {
            users: _,                    // name, ids, shell, home; ssh keys only when preserved
            groups: _,                   // names, ids, members
            sudoers_rules: _,            // scrubbed: patterns, then the URL pass
            ssh_authorized_keys_refs: _, // user, key count, path
            // a passwd line: GECOS cannot hold `:`, so no URL credential
            passwd_entries: _,
            shadow_entries: _,  // scrubbed: the shadow pass, unless preserved
            group_entries: _,   // name, id, members
            gshadow_entries: _, // written with `!` for the password
            subuid_entries: _,  // name and id ranges
            subgid_entries: _,  // name and id ranges
        } = UserGroupSection::default();

        // `selinux`.
        let SelinuxSection {
            mode: _,              // exempt: enforcing, permissive or disabled
            custom_modules: _,    // exempt: module names from the store listing
            disabled_modules: _,  // exempt: module names and an origin
            boolean_overrides: _, // exempt: boolean names and values
            boolean_conflicts: _, // exempt: boolean names and host names
            fcontext_rules: _,    // each variant classified below
            fcontext_source: _,   // classification
            fips_mode: _,         // a flag
            port_labels: _,       // classified below
        } = SelinuxSection::default();
        fn _classify_fcontext_rule(rule: FcontextRule) {
            match rule {
                FcontextRule::Label {
                    spec: _, // scrubbed: the URL pass; a path expression
                    file_type: _,
                    context: _, // exempt: user, role, type and level names
                    disposition: _,
                    aggregate: _,
                } => {}
                FcontextRule::Unlabel {
                    spec: _, // scrubbed: the URL pass; a path expression
                    file_type: _,
                    disposition: _,
                    aggregate: _,
                } => {}
                FcontextRule::Equivalence {
                    subject: _,      // scrubbed: the URL pass; a path
                    labeled_like: _, // scrubbed: the URL pass; a path
                    disposition: _,
                    aggregate: _,
                } => {}
                // scrubbed: the content pass. A line the parser could not
                // place, which the Containerfile's FIXME prints.
                FcontextRule::Unparsed { raw: _ } => {}
            }
        }
        let SelinuxPortLabel {
            protocol: _,   // exempt: tcp, udp, dccp or sctp
            port: _,       // exempt: a port or range
            label_type: _, // exempt: a type name
            disposition: _,
            locked: _,
            aggregate: _,
        } = SelinuxPortLabel::default();

        // `non_rpm_software.items[]`.
        let NonRpmItem {
            manifest_files: _, // scrubbed: key scrubbers, then the URL pass
            git_remote: _,     // scrubbed
            // scrubbed: the content pass. No collector writes it today;
            // when one does, it holds file content.
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
            // each `version` scrubbed: the URL pass; a dependency spec
            // can be a URL
            packages: _,
            has_c_extensions: _,
            git_commit: _,
            git_branch: _,
            rpm_filtered: _,
            files: _,
            aggregate: _,
            review_status: _,
        } = NonRpmItem::default();

        // The snapshot itself and every section: a field added to any of
        // them stops the build here too, so a new list of host text is
        // classified the day it lands, not when a review finds it.
        fn _classify_snapshot(snap: InspectionSnapshot) {
            let InspectionSnapshot {
                schema_version: _,        // a number
                meta: _,                  // scan roots, home users and extra paths
                os_release: _,            // `os-release` identity fields
                system_type: _,           // classification
                rpm: _,                   // classified below
                config: _,                // classified below
                units: _,                 // classified below
                network: _,               // classified above
                storage: _,               // classified below
                scheduled_tasks: _,       // classified below
                containers: _,            // classified below
                non_rpm_software: _,      // classified below
                unmanaged_files: _,       // classified below
                kernel_boot: _,           // classified above
                selinux: _,               // classified above
                users_groups: _,          // classified above
                preflight: _,             // checks and their results
                warnings: _,              // diagnostics, named from the raw value at collection
                redactions: _,            // findings: path, pattern and placeholder
                redaction_hints: _,       // collector hints: path and reason
                redaction_state: _,       // classification
                completeness: _,          // section states and degraded reasons
                target_image: _,          // an image reference and digest
                baseline: _,              // the target image's package and unit data
                sensitive_snapshot: _,    // a flag
                preserved_credentials: _, // a flag
                preserved_ssh_keys: _,    // a flag
                // exempt: collected only when the operator preserves it,
                // which marks the snapshot sensitive
                subscription: _,
                preserved_subscription: _, // a flag
                redaction_skipped: _,      // a flag
                aggregate_meta: _,         // fleet host names and counts
                rpm_repo_conflicts: _,     // repo ids and package names
            } = snap;
        }
        let RpmSection {
            packages_added: _,          // package identities
            base_image_only: _,         // package identities
            rpm_va: _,                  // paths, verify flags and package names
            rpm_va_complete: _,         // a flag
            repo_files: _,              // classified above
            gpg_keys: _,                // classified above, as `RepoFile`
            dnf_history_removed: _,     // package names
            version_changes: _,         // package names and versions
            leaf_packages: _,           // package identities
            auto_packages: _,           // package identities
            leaf_dep_tree: _,           // package identities
            dep_closures: _,            // package identities
            dep_alternatives: _,        // package identities
            unresolved_requirements: _, // capability names
            module_streams: _,          // module, stream and profile names
            // package specs from the versionlock list, and their parts
            version_locks: _,
            module_stream_conflicts: _,    // module names
            baseline_module_streams: _,    // module and stream names
            versionlock_command_output: _, // scrubbed: the content pass
            multiarch_packages: _,         // package names
            duplicate_packages: _,         // package names
            ostree_overrides: _,           // package names and NEVRAs
            ostree_removals: _,            // package names
            base_image: _,                 // an image reference
            baseline_suppressed: _,        // package names
            baseline_package_names: _,     // package names
            leaf_authority_hosts: _,       // a count
            leaf_total_hosts: _,           // a count
            file_ownership: _,             // package names and paths
            installed_groups: _,           // group and package names
        } = RpmSection::default();
        let ConfigSection {
            files: _,    // classified above
            withheld: _, // machine-state paths and members, no content
        } = ConfigSection::default();
        let UnitSection {
            state_changes: _,         // classified in `units.rs`
            inventory: _,             // classified above
            drop_ins: _,              // classified above
            mount_findings: _,        // classified above
            manual_links: _,          // classified above
            undetermined_installs: _, // classified above
            default_target: _,        // classified above
            enabled_on_any_host: _,   // proved unit names
        } = UnitSection::default();
        let StorageSection {
            fstab_entries: _,         // classified above
            mount_points: _,          // classified above
            lvm_info: _,              // `lvs` output: volume names and sizes
            var_directories: _,       // paths and sizes
            credential_refs: _,       // mount points and credential file paths
            unbacked_var_advisory: _, // paths
        } = StorageSection::default();
        let ScheduledTaskSection {
            cron_jobs: _,             // paths: the files are config entries
            systemd_timers: _,        // classified above
            at_jobs: _,               // classified above
            generated_timer_units: _, // classified above
        } = ScheduledTaskSection::default();
        let ContainerSection {
            quadlet_units: _,      // classified above
            compose_files: _,      // classified above
            running_containers: _, // classified below
            flatpak_apps: _,       // classified above
        } = ContainerSection::default();
        let RunningContainer {
            env: _, // scrubbed: the content pass, entry by entry
            id: _,
            name: _,
            image: _, // an image reference; a registry credential is in
            // the pull secret, not here
            image_id: _,
            status: _,
            restart_policy: _,
            mounts: _,   // classified below
            networks: _, // `podman inspect` network names and addresses
            ports: _,    // `podman inspect` port bindings
            inspect_data: _,
            disposition: _,
            locked: _,
            acknowledged: _,
            aggregate: _,
        } = RunningContainer::default();
        let ContainerMount {
            mount_type: _,
            source: _,      // identifier: a host path or volume name
            destination: _, // identifier: a container path
            mode: _,
            rw: _,
        } = ContainerMount::default();
        let NonRpmSoftwareSection {
            items: _,     // classified above
            env_files: _, // classified above, as `ConfigFileEntry`
        } = NonRpmSoftwareSection::default();
        let LanguagePackage {
            name: _,    // a package name
            version: _, // scrubbed: the URL pass; a spec can be a URL
            pinned: _,
        } = LanguagePackage::default();
        let UnmanagedFileSection {
            items: _,       // classified below
            usr_entries: _, // paths, counts and sizes
            total_size: _,
            total_count: _,
            withheld: _, // machine-state paths and members, no content
        } = UnmanagedFileSection::default();
        let UnmanagedFile {
            path: _, // identifier: a path
            size: _,
            file_type: _,
            provenance: _, // owner, mode and times
            disposition: _,
            locked: _,
            acknowledged: _,
            under_var: _,
            // scrubbed: the content pass, after the unmanaged walk, which
            // runs after `redact()`; a credentialed link is withheld
            // (`redact_unmanaged_link_targets`)
            link_target: _,
            aggregate: _,
            content_hash: _,
            variant_selection: _,
            payload_redaction: _, // classification
        } = UnmanagedFile::default();

        // `kernel_boot.sysctl`. No sysctl type has a `Default`, so each
        // pattern sits in a function that is never called: the compiler
        // still checks every pattern for completeness.
        fn _classify_sysctl_section(section: SysctlSection) {
            let SysctlSection {
                files: _,            // each classified below
                runtime: _,          // scrubbed: value removed on any match
                redacted_runtime: _, // exempt: kernel key names
            } = section;
        }
        fn _classify_sysctl_file(file: SysctlFile) {
            let SysctlFile {
                dir: _,  // exempt: one of four directories
                name: _, // exempt: file name, as `ConfigFileEntry.path`
                body: _, // each variant classified below
            } = file;
        }
        fn _classify_sysctl_file_body(body: SysctlFileBody) {
            match body {
                SysctlFileBody::Mask => {} // no fields
                SysctlFileBody::Withheld {
                    member: _, // exempt: an enum name
                } => {}
                SysctlFileBody::OutsideLink {
                    target: _, // scrubbed: one line under the entry's path
                } => {}
                SysctlFileBody::Read {
                    link_target: _, // scrubbed: one line under the entry's path
                    content: _,     // scrubbed: sysctl pass, under the file's path
                    redacted: _,    // exempt: a flag the pass writes
                } => {}
            }
        }
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
        use inspectah_core::types::kernelboot::KernelBootSection;
        use inspectah_core::types::scheduled::{
            AtJob, GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer,
        };
        use inspectah_core::types::sysctl::{
            SysctlDir, SysctlFile, SysctlFileBody, SysctlKey, SysctlSection,
        };
        use inspectah_core::types::units::{
            MountUnitFinding, SystemdDropIn, UnitFragment, UnitName, UnitSection, UnitSubject,
        };

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
            "SENTINEL-SYSTIMER-EXEC",
            "SENTINEL-SYSTIMER-DESC",
            "SENTINEL-ATJOB",
            "SENTINEL-QUADLET",
            "SENTINEL-COMPOSE",
            "SENTINEL-MANIFEST",
            "SENTINEL-GITREMOTE",
            "SENTINEL-SYSCTL-CONTENT",
            "SENTINEL-SYSCTL-LINK",
            "SENTINEL-SYSCTL-OUTSIDE",
            "SENTINEL-SYSCTL-RUNTIME",
            "SENTINEL-MOUNT-WHAT",
            // A URL password with no keyword near it: no pattern reads
            // it, only the URL credential pass, which every field that
            // holds content takes.
            "SENTINEL-URL-CONFIG",
            "SENTINEL-URL-GPGKEY",
            "SENTINEL-URL-DROPIN",
            "SENTINEL-URL-CONTAINERENV",
            "SENTINEL-URL-QUADLET",
            "SENTINEL-URL-GENTIMER-SERVICE",
            "SENTINEL-URL-GENTIMER-TIMER",
            "SENTINEL-URL-GENTIMER-COMMAND",
            "SENTINEL-URL-SYSTIMER-EXEC",
            "SENTINEL-URL-SYSTIMER-DESC",
            "SENTINEL-URL-ATJOB",
            "SENTINEL-URL-SYSCTL-CONTENT",
            "SENTINEL-URL-SYSCTL-RUNTIME",
            "SENTINEL-URL-REPOFILE",
            "SENTINEL-URL-ENVFILE",
            "SENTINEL-URL-COMPOSE",
            "SENTINEL-URL-CMDLINE",
            "SENTINEL-URL-GRUB",
            "SENTINEL-URL-FSTAB-DEVICE",
            "SENTINEL-URL-FSTAB-OPTIONS",
            "SENTINEL-URL-MOUNT-SOURCE",
            "SENTINEL-URL-MOUNT-OPTIONS",
            "SENTINEL-URL-FLATPAK",
            // A token user name before a password: the token is the
            // credential, so it is its own sentinel.
            "SENTINEL-URL-PROXYTOKEN",
            "SENTINEL-URL-ZONE",
            "SENTINEL-URL-RICHRULE",
            "SENTINEL-URL-DIRECTRULE",
            "SENTINEL-URL-HOSTS",
            "SENTINEL-URL-REQUIREMENT",
            "SENTINEL-URL-JSONDEP",
            "SENTINEL-URL-PKGVERSION",
            "SENTINEL-URL-NRSCONTENT",
            "SENTINEL-URL-SUDOERS",
            "SENTINEL-URL-VERSIONLOCK",
            "SENTINEL-URL-FCSPEC",
            "SENTINEL-URL-FCEQUIV",
            "SENTINEL-URL-SELINUX",
            "SENTINEL-SELINUX",
            // Moved from the exempt group: the zone XML and
            // `NonRpmItem.content` now take the content pass.
            "SENTINEL-FIREWALL",
            "SENTINEL-NRSCONTENT",
        ];

        // Declared exempt, and planted anyway so the exemption is a
        // measured decision rather than an untested assumption. Each
        // one's reason is recorded beside its field in
        // `every_field_of_a_content_bearing_type_is_classified`.
        const EXEMPT: &[&str] = &[
            "SENTINEL-MOUNT-POINT", // MountUnitFinding.mount_point is a path
        ];

        // The sweep matches by substring, so no sentinel may hold another.
        for a in SCRUBBED.iter().chain(EXEMPT) {
            for b in SCRUBBED.iter().chain(EXEMPT) {
                assert!(a == b || !b.contains(a), "{a} is inside {b}");
            }
        }

        let mut snap = test_snapshot();

        // A local mount unit's file is a config entry, so its `What=` is
        // scrubbed by the config pass; the finding below has no copy.
        let mount_unit = "/etc/systemd/system/srv-data.mount";
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![
                ConfigFileEntry {
                    path: "/etc/myapp/app.conf".into(),
                    kind: ConfigFileKind::Unowned,
                    content: "password=SENTINEL-CONFIG\n\
                              HTTPS_PROXY=https://u:SENTINEL-URL-CONFIG@proxy.example:3128\n"
                        .into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                ConfigFileEntry {
                    path: mount_unit.into(),
                    kind: ConfigFileKind::Unowned,
                    content:
                        "[Mount]\nWhat=https://files.example/volume?token=SENTINEL-MOUNT-WHAT\n\
                              Where=/srv/data\n"
                            .into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
            ],
        });

        if let Some(ref mut rpm) = snap.rpm {
            rpm.repo_files = vec![inspectah_core::types::rpm::RepoFile {
                path: "/etc/yum.repos.d/corp.repo".into(),
                content: "[corp]\npassword=SENTINEL-REPOFILE\n\
                          baseurl=https://u:SENTINEL-URL-REPOFILE@repo.example/\n"
                    .into(),
                ..Default::default()
            }];
            rpm.versionlock_command_output =
                Some("mirror https://u:SENTINEL-URL-VERSIONLOCK@m.example/\n".into());
            rpm.gpg_keys = vec![inspectah_core::types::rpm::RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-corp".into(),
                content: "password=SENTINEL-GPGKEY\n\
                          Comment: https://u:SENTINEL-URL-GPGKEY@keys.example/\n"
                    .into(),
                ..Default::default()
            }];
        }

        let srv_data = UnitName::try_from("srv-data.mount".to_owned()).unwrap();
        let advisory = UnitSubject::new(srv_data.clone(), UnitFragment::Local { file: srv_data })
            .unwrap()
            .try_into()
            .unwrap();
        snap.units = Some(UnitSection {
            drop_ins: vec![SystemdDropIn {
                unit: "app.service".into(),
                path: "/etc/systemd/system/app.service.d/override.conf".into(),
                content: "[Service]\nEnvironment=DB_PASSWORD=SENTINEL-DROPIN\n\
                          Environment=HTTPS_PROXY=https://u:SENTINEL-URL-DROPIN@p.example\n"
                    .into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            mount_findings: vec![MountUnitFinding {
                unit: advisory,
                state_change: None,
                // Behind a keyword, as every sentinel is: a visited field
                // would lose it.
                mount_point: Some("/srv/token=SENTINEL-MOUNT-POINT".into()),
                nofail: Some(false),
                aggregate: None,
            }],
            ..Default::default()
        });

        snap.scheduled_tasks = Some(ScheduledTaskSection {
            generated_timer_units: vec![GeneratedTimerUnit {
                name: "backup".into(),
                source_path: "/etc/cron.d/backup".into(),
                command: "/usr/bin/backup --password=SENTINEL-GENTIMER-COMMAND \
                          --mirror https://u:SENTINEL-URL-GENTIMER-COMMAND@m.example"
                    .into(),
                service_content: "[Service]\nEnvironment=DB_PASSWORD=SENTINEL-GENTIMER-SERVICE\n\
                                  Environment=HTTPS_PROXY=https://u:SENTINEL-URL-GENTIMER-SERVICE@p\n"
                    .into(),
                // A `.timer` has no ExecStart= or Environment=, so a
                // secret in one realistically rides on Description=.
                // Not a comment line: comment lines are exempt from
                // scanning on every field, by design.
                timer_content:
                    "[Timer]\nOnCalendar=daily\nDescription=sync api_key=SENTINEL-GENTIMER-TIMER \
                     from https://u:SENTINEL-URL-GENTIMER-TIMER@m.example\n"
                        .into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            systemd_timers: vec![SystemdTimer {
                name: "report".into(),
                description: "report job api_key=SENTINEL-SYSTIMER-DESC \
                              to https://u:SENTINEL-URL-SYSTIMER-DESC@r.example"
                    .into(),
                source: "local".into(),
                path: "/etc/systemd/system/report.timer".into(),
                exec_start: "/usr/bin/report --password=SENTINEL-SYSTIMER-EXEC \
                             https://u:SENTINEL-URL-SYSTIMER-EXEC@r.example"
                    .into(),
                ..Default::default()
            }],
            at_jobs: vec![AtJob {
                file: "/var/spool/at/a0001".into(),
                command: "/usr/bin/sync --password=SENTINEL-ATJOB \
                          https://u:SENTINEL-URL-ATJOB@s.example"
                    .into(),
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
                          Environment=DB_PASSWORD=SENTINEL-QUADLET\n\
                          Environment=HTTPS_PROXY=https://u:SENTINEL-URL-QUADLET@p.example\n"
                    .into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            running_containers: vec![inspectah_core::types::containers::RunningContainer {
                name: "web".into(),
                env: vec!["HTTPS_PROXY=https://u:SENTINEL-URL-CONTAINERENV@p.example".into()],
                ..Default::default()
            }],
            compose_files: vec![ComposeFile {
                path: "opt/myapp/docker-compose.yml".into(),
                raw_content: Some(
                    "services:\n  db:\n    environment:\n      \
                     POSTGRES_PASSWORD: SENTINEL-COMPOSE\n      \
                     MIRROR: https://u:SENTINEL-URL-COMPOSE@m.example\n"
                        .into(),
                ),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            flatpak_apps: vec![inspectah_core::types::containers::FlatpakApp {
                app_id: "org.example.App".into(),
                origin: "corp".into(),
                branch: "stable".into(),
                remote: "corp".into(),
                remote_url: "https://u:SENTINEL-URL-FLATPAK@flatpak.example/repo/".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
        });

        let mut manifest_files = std::collections::HashMap::new();
        manifest_files.insert(
            "package-lock.json".to_string(),
            "{\n  \"resolved\": \"https://u:SENTINEL-MANIFEST@npm.corp.example.com/x.tgz\"\n}\n"
                .to_string(),
        );
        // Direct dependency URLs, under no key the manifest scrubbers read.
        manifest_files.insert(
            "requirements.txt".to_string(),
            "widget @ https://u:SENTINEL-URL-REQUIREMENT@pypi.example/widget.whl\n".to_string(),
        );
        manifest_files.insert(
            "package.json".to_string(),
            "{\"dependencies\":{\"widget\":\"git+https://u:SENTINEL-URL-JSONDEP@git.example/w.git\"}}\n"
                .to_string(),
        );
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                path: "opt/webapp".into(),
                name: "webapp".into(),
                content: "password=SENTINEL-NRSCONTENT\n\
                          url=https://u:SENTINEL-URL-NRSCONTENT@h.example\n"
                    .into(),
                packages: vec![inspectah_core::types::nonrpm::LanguagePackage {
                    name: "widget".into(),
                    version: "git+https://u:SENTINEL-URL-PKGVERSION@git.example/w.git".into(),
                    pinned: false,
                }],
                method: inspectah_core::util::METHOD_NPM_LOCKFILE.into(),
                git_remote: "https://u:SENTINEL-GITREMOTE@git.corp.example.com/webapp.git".into(),
                manifest_files,
                ..Default::default()
            }],
            env_files: vec![ConfigFileEntry {
                path: "opt/webapp/.env".into(),
                content: "DB_PASSWORD=SENTINEL-ENVFILE\n\
                          MIRROR=https://u:SENTINEL-URL-ENVFILE@m.example\n"
                    .into(),
                ..Default::default()
            }],
        });

        snap.network = Some(inspectah_core::types::network::NetworkSection {
            firewall_zones: vec![inspectah_core::types::network::FirewallZone {
                path: "/etc/firewalld/zones/corp.xml".into(),
                name: "corp".into(),
                content: "<zone>\n  <!-- password=SENTINEL-FIREWALL -->\n  \
                          <description>https://u:SENTINEL-URL-ZONE@h.example/</description>\n\
                          </zone>\n"
                    .into(),
                rich_rules: vec![
                    "rule family=\"ipv4\" log prefix=\"https://u:SENTINEL-URL-RICHRULE@h\" accept"
                        .into(),
                ],
                ..Default::default()
            }],
            firewall_direct_rules: vec![inspectah_core::types::network::FirewallDirectRule {
                ipv: "ipv4".into(),
                table: "filter".into(),
                chain: "INPUT".into(),
                priority: "0".into(),
                args: "-p tcp -m comment --comment https://u:SENTINEL-URL-DIRECTRULE@h -j ACCEPT"
                    .into(),
                ..Default::default()
            }],
            hosts_additions: vec!["10.0.0.5 db.internal # https://u:SENTINEL-URL-HOSTS@h".into()],
            proxy: vec![inspectah_core::types::network::ProxyEntry {
                source: "etc/environment".into(),
                line: "https_proxy=https://SENTINEL-URL-PROXYTOKEN:x-oauth-basic@p.example".into(),
            }],
            ..Default::default()
        });

        snap.storage = Some(inspectah_core::types::storage::StorageSection {
            fstab_entries: vec![inspectah_core::types::storage::FstabEntry {
                device: "https://u:SENTINEL-URL-FSTAB-DEVICE@files.example/share".into(),
                mount_point: "/srv/webdav".into(),
                fstype: "davfs".into(),
                options: "_netdev,proxy=https://u:SENTINEL-URL-FSTAB-OPTIONS@p.example".into(),
                ..Default::default()
            }],
            mount_points: vec![inspectah_core::types::storage::MountPoint {
                target: "/srv/webdav".into(),
                source: "https://u:SENTINEL-URL-MOUNT-SOURCE@files.example/share".into(),
                fstype: "fuse".into(),
                options: "rw,proxy=https://u:SENTINEL-URL-MOUNT-OPTIONS@p.example".into(),
            }],
            ..Default::default()
        });

        snap.users_groups = Some(inspectah_core::types::users::UserGroupSection {
            sudoers_rules: vec![
                "alice ALL=(ALL) /usr/bin/curl https://u:SENTINEL-URL-SUDOERS@h.example/x".into(),
            ],
            ..Default::default()
        });

        // A line `file_contexts.local` holds that the parser could not
        // place keeps its raw text, and a rule's operands are paths.
        {
            use inspectah_core::types::selinux::{
                FcontextFileType, FcontextRule, SelinuxContext, SelinuxSection,
            };
            snap.selinux = Some(SelinuxSection {
                fcontext_rules: vec![
                    FcontextRule::Label {
                        spec: "https://u:SENTINEL-URL-FCSPEC@h.example/".into(),
                        file_type: FcontextFileType::All,
                        context: SelinuxContext {
                            user: "system_u".into(),
                            role: "object_r".into(),
                            type_: "httpd_sys_content_t".into(),
                            level: Some("s0".into()),
                        },
                        disposition: FindingKind::included(),
                        aggregate: None,
                    },
                    FcontextRule::Equivalence {
                        subject: "https://u:SENTINEL-URL-FCEQUIV@h.example/".into(),
                        labeled_like: "/var/www".into(),
                        disposition: FindingKind::included(),
                        aggregate: None,
                    },
                    FcontextRule::Unparsed {
                        raw: "https://u:SENTINEL-URL-SELINUX@repo.example/ \
                              password=SENTINEL-SELINUX"
                            .into(),
                    },
                ],
                ..Default::default()
            });
        }

        // Planted field by field, not a host layout: the collector-driven
        // cases are in `crates/cli/tests/sysctl_scan_test.rs`.
        snap.kernel_boot = Some(KernelBootSection {
            cmdline: "root=/dev/vda1 ro inst.repo=https://u:SENTINEL-URL-CMDLINE@repo.example/os/"
                .into(),
            grub_defaults:
                "GRUB_CMDLINE_LINUX=\"inst.repo=https://u:SENTINEL-URL-GRUB@repo.example/\"\n"
                    .into(),
            sysctl: Some(SysctlSection {
                files: vec![
                    SysctlFile {
                        dir: SysctlDir::Etc,
                        name: "80-app.conf".into(),
                        body: SysctlFileBody::Read {
                            link_target: None,
                            content: "password=SENTINEL-SYSCTL-CONTENT\n\
                                      x = https://u:SENTINEL-URL-SYSCTL-CONTENT@h\n"
                                .into(),
                            redacted: false,
                        },
                    },
                    SysctlFile {
                        dir: SysctlDir::Etc,
                        name: "85-link.conf".into(),
                        body: SysctlFileBody::Read {
                            link_target: Some("/etc/app/password=SENTINEL-SYSCTL-LINK".into()),
                            content: "vm.swappiness = 10\n".into(),
                            redacted: false,
                        },
                    },
                    SysctlFile {
                        dir: SysctlDir::Etc,
                        name: "90-outside.conf".into(),
                        body: SysctlFileBody::OutsideLink {
                            target: "/var/lib/app/password=SENTINEL-SYSCTL-OUTSIDE".into(),
                        },
                    },
                ],
                runtime: [
                    (
                        SysctlKey::normalize("kernel.core_pattern"),
                        "password=SENTINEL-SYSCTL-RUNTIME".to_string(),
                    ),
                    (
                        SysctlKey::normalize("kernel.hostname"),
                        "https://u:SENTINEL-URL-SYSCTL-RUNTIME@h".to_string(),
                    ),
                ]
                .into(),
                redacted_runtime: Vec::new(),
            }),
            ..Default::default()
        });

        // Every sentinel must actually be in the snapshot before
        // redaction, or the sweep passes for the wrong reason.
        let before = serde_json::to_string(&snap).unwrap();
        for s in SCRUBBED.iter().chain(EXEMPT) {
            assert!(before.contains(s), "fixture never planted {s}");
        }

        crate::redaction::engine::redact(&mut snap, &Default::default());

        // The snapshot object itself, serialized: what both export paths
        // and `aggregate` start from.
        let json = serde_json::to_string(&snap).unwrap();
        let in_json: Vec<&&str> = SCRUBBED.iter().filter(|s| json.contains(**s)).collect();
        assert!(
            in_json.is_empty(),
            "fields declared scrubbed are in the redacted snapshot: {in_json:?}"
        );

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
            withheld: Vec::new(),
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
