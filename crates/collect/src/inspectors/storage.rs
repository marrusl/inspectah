use crate::rpm_ownership::build_rpm_owned_set;
use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::FindingKind;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::finding::AdvisoryType;
use inspectah_core::types::redaction::{Confidence, RedactionHint};
use inspectah_core::types::storage::{
    CredentialRef, FstabEntry, LvmVolume, MountPoint, StorageSection, UnbackedVarAdvisory,
    VarDirBacking,
};
use inspectah_core::types::warnings::{Warning, WarningSeverity};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Inspects storage configuration: fstab entries, active mount points,
/// LVM volumes, and detects credential references in mount options.
pub struct StorageInspector;

impl StorageInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StorageInspector {
    fn default() -> Self {
        Self::new()
    }
}

/// Deserialization target for `findmnt --json` output.
#[derive(Deserialize)]
struct FindmntOutput {
    filesystems: Vec<FindmntEntry>,
}

#[derive(Deserialize)]
struct FindmntEntry {
    target: String,
    source: String,
    fstype: String,
    options: String,
}

/// Deserialization target for `lvs --reportformat json` output.
#[derive(Deserialize)]
struct LvsOutput {
    report: Vec<LvsReport>,
}

#[derive(Deserialize)]
struct LvsReport {
    lv: Vec<LvsEntry>,
}

#[derive(Deserialize)]
struct LvsEntry {
    lv_name: String,
    vg_name: String,
    lv_size: String,
}

impl Inspector for StorageInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Storage
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        _progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        let exec = ctx.executor;

        // 1. Read /etc/fstab — primary source, failure is fatal.
        let fstab_path = Path::new("/etc/fstab");
        let fstab_content = exec
            .read_file(fstab_path)
            .map_err(|e| InspectorError::Failed {
                reason: format!("cannot read /etc/fstab: {e}"),
            })?;

        let (fstab_entries, credential_refs, redaction_hints) = parse_fstab(&fstab_content);

        // 2. Run findmnt --json — degraded if unavailable or malformed.
        let mount_points = match collect_findmnt(exec) {
            Ok(mounts) => mounts,
            Err(reason) => {
                return Err(InspectorError::Degraded {
                    partial: Box::new(InspectorOutput {
                        section: SectionData::Storage(StorageSection {
                            fstab_entries,
                            mount_points: Vec::new(),
                            lvm_info: Vec::new(),
                            var_directories: Vec::new(),
                            credential_refs,
                            unbacked_var_advisory: None,
                        }),
                        warnings: Vec::new(),
                        redaction_hints,
                    }),
                    reason,
                });
            }
        };

        // 3. Run lvs --reportformat json — optional, proceed without.
        let lvm_info = collect_lvs(exec).unwrap_or_default();

        // 4. Discover and classify var directories.
        let (mut var_directories, var_warnings) = discover_var_directories(exec);
        let rpm_owned = build_rpm_owned_set(exec);
        let tmpfiles = load_tmpfiles_index(exec);
        for dir in &mut var_directories {
            dir.backing = Some(detect_var_dir_backing(
                exec, &dir.path, &rpm_owned, &tmpfiles,
            ));
        }

        // Collect unbacked paths for grouped advisory
        let unbacked_paths: Vec<String> = var_directories
            .iter()
            .filter(|d| d.backing == Some(VarDirBacking::Unbacked))
            .map(|d| d.path.clone())
            .collect();

        let unbacked_var_advisory = if unbacked_paths.is_empty() {
            None
        } else {
            Some(UnbackedVarAdvisory {
                disposition: FindingKind::advisory(
                    AdvisoryType::UnbackedVarDir,
                    "These /var directories have no declarative backing (tmpfiles.d, \
                     StateDirectory=, CacheDirectory=, LogsDirectory=). Consider adding \
                     tmpfiles.d entries for a more reproducible, declarative approach.",
                ),
                paths: unbacked_paths,
            })
        };

        Ok(InspectorOutput {
            section: SectionData::Storage(StorageSection {
                fstab_entries,
                mount_points,
                lvm_info,
                var_directories,
                credential_refs,
                unbacked_var_advisory,
            }),
            warnings: var_warnings,
            redaction_hints,
        })
    }
}

/// Discover non-trivial /var directories for backing analysis.
///
/// Scans /var/lib/, /var/log/, /var/cache/ up to depth 2 for non-empty
/// subdirectories, then deduplicates by keeping only leaf directories
/// (removing any path that is a prefix of a deeper discovered path).
fn discover_var_directories(
    exec: &dyn Executor,
) -> (
    Vec<inspectah_core::types::storage::VarDirectory>,
    Vec<Warning>,
) {
    use inspectah_core::types::storage::VarDirectory;

    let mut paths = Vec::new();

    for base in &["/var/lib", "/var/log", "/var/cache"] {
        let result = exec.run(
            "find",
            &[
                base,
                "-mindepth",
                "1",
                "-maxdepth",
                "2",
                "-type",
                "d",
                "!",
                "-empty",
            ],
        );
        if result.exit_code != 0 {
            continue;
        }

        for line in result.stdout.lines() {
            let path = line.trim();
            if path.is_empty() {
                continue;
            }
            paths.push(path.to_string());
        }
    }

    // Deduplicate: remove any path that is a proper prefix of another path.
    // This keeps leaf directories (e.g., /var/lib/pgsql/data) and drops
    // their parents (/var/lib/pgsql) when both are discovered.
    let all_paths = paths.clone();
    paths.retain(|p| {
        let prefix = format!("{}/", p);
        !all_paths.iter().any(|other| other.starts_with(&prefix))
    });

    let mut warnings = Vec::new();
    let dirs = paths
        .into_iter()
        .map(|path| {
            let mut var_dir = VarDirectory {
                path: path.clone(),
                ..Default::default()
            };

            // Capture ownership and mode via stat
            let stat_result = exec.run("stat", &["-c", "%04a %u %g %U %G", &path]);
            if stat_result.exit_code == 0 {
                let parts: Vec<&str> = stat_result.stdout.trim().splitn(5, ' ').collect();
                if parts.len() == 5 {
                    var_dir.mode = Some(parts[0].to_string());
                    var_dir.owner_uid = parts[1].parse().ok();
                    var_dir.owner_gid = parts[2].parse().ok();
                    var_dir.owner_name = Some(parts[3].to_string());
                    var_dir.group_name = Some(parts[4].to_string());
                }
            } else {
                warnings.push(Warning {
                    inspector: "storage".to_string(),
                    message: format!(
                        "Could not read ownership/mode for {}: \
                         tmpfiles.d provisioning unavailable, falling back to mkdir",
                        path
                    ),
                    severity: Some(WarningSeverity::Warning),
                    extra: std::collections::HashMap::new(),
                });
            }

            var_dir
        })
        .collect();
    (dirs, warnings)
}

/// Parse /etc/fstab content into FstabEntry list, credential refs, and redaction hints.
fn parse_fstab(content: &str) -> (Vec<FstabEntry>, Vec<CredentialRef>, Vec<RedactionHint>) {
    let mut entries = Vec::new();
    let mut cred_refs = Vec::new();
    let mut hints = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let device = parts[0].to_string();
        let mount_point = parts[1].to_string();
        let fstype = parts[2].to_string();
        let options = parts[3].to_string();

        // Detect credential references in mount options
        for opt in options.split(',') {
            if let Some(cred_path) = opt.strip_prefix("credentials=") {
                cred_refs.push(CredentialRef {
                    mount_point: mount_point.clone(),
                    credential_path: cred_path.to_string(),
                    source: "fstab".into(),
                });
                hints.push(RedactionHint {
                    path: "/etc/fstab".into(),
                    reason: format!(
                        "credential reference in mount options for {mount_point}: {cred_path}"
                    ),
                    confidence: Some(Confidence::High),
                });
            }
            if opt.starts_with("password=") {
                hints.push(RedactionHint {
                    path: "/etc/fstab".into(),
                    reason: format!("inline password in mount options for {mount_point}"),
                    confidence: Some(Confidence::High),
                });
            }
        }

        entries.push(FstabEntry {
            device,
            mount_point,
            fstype,
            options,
            disposition: FindingKind::included(),
            locked: false,
            acknowledged: false,
            aggregate: None,
            attention_reason: None,
        });
    }

    (entries, cred_refs, hints)
}

/// Collect mount points from `findmnt --json`.
fn collect_findmnt(exec: &dyn Executor) -> Result<Vec<MountPoint>, String> {
    let result = exec.run("findmnt", &["--json"]);

    if !result.success() {
        return Err(format!(
            "findmnt failed with exit code {}",
            result.exit_code
        ));
    }

    let parsed: FindmntOutput = serde_json::from_str(&result.stdout)
        .map_err(|e| format!("failed to parse findmnt JSON: {e}"))?;

    Ok(parsed
        .filesystems
        .into_iter()
        .map(|fs| MountPoint {
            target: fs.target,
            source: fs.source,
            fstype: fs.fstype,
            options: fs.options,
        })
        .collect())
}

/// Collect LVM volumes from `lvs --reportformat json`.
/// Returns Ok(empty) if lvs is not available — LVM is optional.
fn collect_lvs(exec: &dyn Executor) -> Result<Vec<LvmVolume>, String> {
    let result = exec.run("lvs", &["--reportformat", "json"]);

    // lvs not available or failed — not an error, just no LVM data.
    if !result.success() {
        return Ok(Vec::new());
    }

    let parsed: LvsOutput = serde_json::from_str(&result.stdout)
        .map_err(|e| format!("failed to parse lvs JSON: {e}"))?;

    Ok(parsed
        .report
        .into_iter()
        .flat_map(|r| r.lv)
        .map(|lv| LvmVolume {
            lv_name: lv.lv_name,
            vg_name: lv.vg_name,
            lv_size: lv.lv_size,
        })
        .collect())
}

/// tmpfiles.d fragment directories, highest systemd precedence first.
///
/// A basename in an earlier directory masks the same basename in every
/// later one (`tmpfiles.d(5)`).
const TMPFILES_DIRS: [&str; 3] = ["/etc/tmpfiles.d", "/run/tmpfiles.d", "/usr/lib/tmpfiles.d"];

/// The only filename suffix systemd reads in a tmpfiles.d directory.
/// Anything else there is not a fragment and masks nothing.
const TMPFILES_SUFFIX: &str = ".conf";

/// Entry types that create a directory at exactly their own path, and so
/// are the only types that back one.
///
/// Everything else creates something other than a directory (`f`, `F`,
/// `w`, `p`, `c`, `b`, `L`, `C`), adjusts a path that already exists
/// (`z`, `Z`, `e`, `a`/`A`, `h`/`H`, `t`/`T`), or removes and exempts
/// (`r`, `R`, `x`, `X`).
const DIRECTORY_CREATING_TYPES: [char; 5] = ['d', 'D', 'v', 'q', 'Q'];

/// `rpm --query --file` template emitting one bare package name per line.
///
/// Bare `%{NAME}`, deliberately not `name.arch`: the export compares this
/// payload against `rpm.file_ownership[].package_name` and
/// `rpm.baseline_package_names`, both of which are bare by contract.
const OWNING_PACKAGE_QUERY_FORMAT: &str = "%{NAME}\\n";

/// A systemd directory directive: the `/var` prefix it provisions under,
/// the directive name, and the backing a match produces.
type DirectoryDirective = (&'static str, &'static str, fn(String) -> VarDirBacking);

const DIRECTORY_DIRECTIVES: [DirectoryDirective; 3] = [
    ("/var/lib/", "StateDirectory", |unit| {
        VarDirBacking::StateDirectory { unit }
    }),
    ("/var/cache/", "CacheDirectory", |unit| {
        VarDirBacking::CacheDirectory { unit }
    }),
    ("/var/log/", "LogsDirectory", |unit| {
        VarDirBacking::LogsDirectory { unit }
    }),
];

/// A tmpfiles.d entry whose type creates a directory: the fragment that
/// declares it and the line as written.
struct TmpfilesDirEntry {
    conf: String,
    line: String,
}

/// Directory-creating tmpfiles.d entries, keyed by the exact path each
/// one creates.
type TmpfilesIndex = HashMap<String, TmpfilesDirEntry>;

/// Read every tmpfiles.d fragment and index the entries that create a
/// directory.
///
/// Fragments are read in systemd precedence order and the first entry
/// claiming a path wins, both across fragments and within one.
fn load_tmpfiles_index(exec: &dyn Executor) -> TmpfilesIndex {
    let mut index = TmpfilesIndex::new();
    let mut claimed_basenames = HashSet::new();

    for dir in TMPFILES_DIRS {
        let Ok(mut basenames) = exec.read_dir(Path::new(dir)) else {
            continue;
        };
        // read_dir order is unspecified; systemd reads fragments in
        // lexicographic order and the first entry for a path wins.
        basenames.sort();
        for basename in basenames {
            if !basename.ends_with(TMPFILES_SUFFIX) || !claimed_basenames.insert(basename.clone()) {
                continue;
            }
            let conf = format!("{dir}/{basename}");
            let Ok(content) = exec.read_file(Path::new(&conf)) else {
                continue;
            };
            index_tmpfiles_fragment(&conf, &content, &mut index);
        }
    }

    index
}

/// Index one fragment's directory-creating entries into `index`.
fn index_tmpfiles_fragment(conf: &str, content: &str, index: &mut TmpfilesIndex) {
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let (Some(type_token), Some(path)) = (fields.next(), fields.next()) else {
            continue;
        };
        // A `%` specifier is expanded by systemd at runtime against state
        // this parser does not have, so such an entry matches no path.
        if path.contains('%') {
            continue;
        }
        // The first character is the entry type; the `+ ! - = ~ ^`
        // modifiers that may follow it do not change which kind it is.
        let Some(entry_type) = type_token.chars().next() else {
            continue;
        };
        if !DIRECTORY_CREATING_TYPES.contains(&entry_type) {
            continue;
        }
        index
            .entry(path.to_string())
            .or_insert_with(|| TmpfilesDirEntry {
                conf: conf.to_string(),
                line: line.to_string(),
            });
    }
}

/// `grep -l` prints one matching file path per line; the backing records
/// the unit's filename, not the directory it was found in.
fn first_unit_filename(grep_stdout: &str) -> Option<String> {
    let first = grep_stdout.lines().map(str::trim).find(|l| !l.is_empty())?;
    Path::new(first)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

/// Ask rpm which package owns `path`.
fn query_owning_package(exec: &dyn Executor, path: &str) -> Option<String> {
    let result = exec.run(
        "rpm",
        &[
            "--query",
            "--file",
            path,
            "--qf",
            OWNING_PACKAGE_QUERY_FORMAT,
        ],
    );
    if result.exit_code != 0 {
        return None;
    }
    result
        .stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

/// Determine the backing mechanism for a /var directory.
///
/// Checks, in order: a tmpfiles.d entry that creates exactly this path,
/// systemd directory directives (StateDirectory, CacheDirectory,
/// LogsDirectory), RPM ownership. Falls back to `Unbacked` if no
/// backing is found.
///
/// A parent's `d` line backs the parent and nothing below it, so there is
/// no ancestor walk: a directory only an ancestor's entry names is
/// `Unbacked`.
fn detect_var_dir_backing(
    exec: &dyn Executor,
    path: &str,
    rpm_owned: &HashSet<String>,
    tmpfiles: &TmpfilesIndex,
) -> VarDirBacking {
    if let Some(entry) = tmpfiles.get(path) {
        return VarDirBacking::Tmpfiles {
            conf: entry.conf.clone(),
            line: entry.line.clone(),
        };
    }

    // Check systemd directory directives using the relative path
    // under each directive's base prefix.
    for (prefix, directive, backing) in DIRECTORY_DIRECTIVES {
        if let Some(relative) = path.strip_prefix(prefix) {
            if relative.is_empty() {
                continue;
            }
            let pattern = format!(r"{}\s*=\s*{}", directive, relative);
            let grep = exec.run(
                "grep",
                &[
                    "-rE",
                    "--include=*.service",
                    "--include=*.socket",
                    "-l",
                    &pattern,
                    "/usr/lib/systemd/system/",
                    "/etc/systemd/system/",
                ],
            );
            if grep.exit_code == 0
                && let Some(unit) = first_unit_filename(&grep.stdout)
            {
                return backing(unit);
            }
        }
    }

    // Check RPM ownership. A path rpm owns but will not name the owner of
    // falls through to Unbacked rather than carrying an empty package:
    // over-provisioning is the safe direction here, and it is what a
    // failed rpm query already produced through an empty owned set.
    //
    // "Here" is load-bearing, because the fleet case inverts it. This is
    // one host's verdict, where losing the proof costs a `d` line that is
    // free if the directory already exists and necessary if it does not.
    // One image serves a whole fleet, so any host's carried fragment
    // creates the directory on every host: once the aggregate merges, a
    // lost proof means a duplicate `d` line with order-dependent mode
    // resolution, and preferring the backed verdict is the safe
    // direction. See the var_directories dedup in `aggregate::merge`.
    if rpm_owned.contains(path)
        && let Some(package) = query_owning_package(exec, path)
    {
        return VarDirBacking::RpmOwned { package };
    }

    VarDirBacking::Unbacked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;

    fn grep_success(stdout: &str) -> ExecResult {
        ExecResult {
            stdout: stdout.to_string(),
            stderr: String::new(),
            exit_code: 0,
        }
    }

    fn owning_package_key(path: &str) -> String {
        format!("rpm --query --file {path} --qf {OWNING_PACKAGE_QUERY_FORMAT}")
    }

    /// A tmpfiles.d tree in the shape § 3.4's worked example describes:
    /// systemd's own `var.conf`, a host fragment under `/etc`, a runtime
    /// fragment under `/run`, label-only and non-directory entries, a
    /// basename present in both `/etc` and `/usr/lib`, and a file in a
    /// fragment directory that is not a fragment.
    fn tmpfiles_host() -> MockExecutor {
        MockExecutor::new()
            .with_dir("/etc/tmpfiles.d", vec!["appone.conf", "masked.conf"])
            .with_dir("/run/tmpfiles.d", vec!["runtime.conf"])
            .with_dir(
                "/usr/lib/tmpfiles.d",
                vec![
                    "var.conf",
                    "systemd.conf",
                    "labels.conf",
                    "cleanup.conf",
                    "masked.conf",
                    "notes.txt",
                ],
            )
            .with_file(
                "/etc/tmpfiles.d/appone.conf",
                "# application state\n\nd /var/lib/appone/cache 0750 appone appone -\n",
            )
            .with_file(
                "/etc/tmpfiles.d/masked.conf",
                "d /var/lib/from-etc 0755 root root -\n",
            )
            .with_file(
                "/run/tmpfiles.d/runtime.conf",
                "d /var/lib/rt 0755 root root -\n",
            )
            .with_file(
                "/usr/lib/tmpfiles.d/var.conf",
                "d /var/lib 0755 root root -\n\
                 d /var/log 0755 root root -\n\
                 d /var/cache 0755 root root -\n\
                 d /var/log/journal/%m 2755 root systemd-journal -\n",
            )
            .with_file(
                "/usr/lib/tmpfiles.d/systemd.conf",
                "d /var/lib/systemd/coredump 0755 root root 3d\n",
            )
            .with_file(
                "/usr/lib/tmpfiles.d/labels.conf",
                "z /var/lib/zonly 0755 root root -\n\
                 Z /var/lib/zrec - - - -\n\
                 e /var/lib/eonly - - - 30d\n\
                 f /var/lib/afile 0644 root root -\n",
            )
            .with_file(
                "/usr/lib/tmpfiles.d/cleanup.conf",
                "D /run/myapp 0755 root root -\n",
            )
            .with_file(
                "/usr/lib/tmpfiles.d/masked.conf",
                "d /var/lib/from-usr 0755 root root -\n",
            )
            .with_file(
                "/usr/lib/tmpfiles.d/notes.txt",
                "d /var/lib/not-a-fragment 0755 root root -\n",
            )
    }

    #[test]
    fn only_an_exact_directory_creator_backs_a_directory() {
        let exec = tmpfiles_host();
        let index = load_tmpfiles_index(&exec);
        let rpm_owned = HashSet::new();
        let backing = |path: &str| detect_var_dir_backing(&exec, path, &rpm_owned, &index);

        // --- backed: an entry whose type creates exactly this path ---
        assert_eq!(
            backing("/var/lib/appone/cache"),
            VarDirBacking::Tmpfiles {
                conf: "/etc/tmpfiles.d/appone.conf".into(),
                line: "d /var/lib/appone/cache 0750 appone appone -".into(),
            }
        );
        assert_eq!(
            backing("/var/lib/rt"),
            VarDirBacking::Tmpfiles {
                conf: "/run/tmpfiles.d/runtime.conf".into(),
                line: "d /var/lib/rt 0755 root root -".into(),
            },
            "a /run fragment backs its own path and is recorded as such"
        );
        assert_eq!(
            backing("/var/lib"),
            VarDirBacking::Tmpfiles {
                conf: "/usr/lib/tmpfiles.d/var.conf".into(),
                line: "d /var/lib 0755 root root -".into(),
            },
            "/usr/lib fragments are read"
        );
        assert_eq!(
            backing("/var/lib/systemd/coredump"),
            VarDirBacking::Tmpfiles {
                conf: "/usr/lib/tmpfiles.d/systemd.conf".into(),
                line: "d /var/lib/systemd/coredump 0755 root root 3d".into(),
            },
            "a /usr/lib fragment backs a directory the discovery walk reaches"
        );
        assert_eq!(
            backing("/var/lib/from-etc"),
            VarDirBacking::Tmpfiles {
                conf: "/etc/tmpfiles.d/masked.conf".into(),
                line: "d /var/lib/from-etc 0755 root root -".into(),
            },
            "the /etc copy of a masked basename is the one that is read"
        );

        // --- unbacked: everything else ---
        for path in [
            // Nothing names it. The ancestor walk called this Tmpfiles
            // because systemd's `d /var/lib` contains the string.
            "/var/lib/pgsql",
            // A parent's `d` line creates the parent and nothing below it.
            "/var/lib/appone/cache/sub",
            // z, Z, e adjust a path that already exists; f creates a file.
            "/var/lib/zonly",
            "/var/lib/zrec",
            "/var/lib/eonly",
            "/var/lib/afile",
            // No entry anywhere names it.
            "/var/lib/foo",
            // Its basename is claimed by the /etc copy of masked.conf.
            "/var/lib/from-usr",
            // A % specifier matches neither itself nor any expansion.
            "/var/log/journal/%m",
            "/var/log/journal/3f2a1c",
            // cleanup.conf's D line creates /run/myapp, not this.
            "/var/myapp",
            // notes.txt is not a fragment, so its `d` line is not read.
            "/var/lib/not-a-fragment",
        ] {
            assert_eq!(
                backing(path),
                VarDirBacking::Unbacked,
                "{path} should have no tmpfiles backing"
            );
        }
    }

    /// Only `d`, `D`, `v`, `q`, `Q` create a directory at their own path.
    /// Modifiers after the type character do not change the kind.
    #[test]
    fn entry_type_decides_whether_a_line_backs_its_path() {
        let cases = [
            ("d", true),
            ("D", true),
            ("v", true),
            ("q", true),
            ("Q", true),
            ("d+", true),
            ("d!", true),
            ("d-", true),
            ("v=", true),
            ("q~", true),
            ("Q^", true),
            ("f", false),
            ("F", false),
            ("w", false),
            ("p", false),
            ("c", false),
            ("b", false),
            ("L", false),
            ("C", false),
            ("z", false),
            ("Z", false),
            ("e", false),
            ("a", false),
            ("A", false),
            ("h", false),
            ("H", false),
            ("t", false),
            ("T", false),
            ("r", false),
            ("R", false),
            ("x", false),
            ("X", false),
        ];
        let path = "/var/lib/typed";
        for (type_token, creates_directory) in cases {
            let mut index = TmpfilesIndex::new();
            index_tmpfiles_fragment(
                "/etc/tmpfiles.d/types.conf",
                &format!("{type_token} {path} 0755 root root -\n"),
                &mut index,
            );
            assert_eq!(
                index.contains_key(path),
                creates_directory,
                "type token {type_token:?}"
            );
        }
    }

    #[test]
    fn first_entry_for_a_path_wins() {
        let mut index = TmpfilesIndex::new();
        index_tmpfiles_fragment(
            "/etc/tmpfiles.d/dup.conf",
            "d /var/lib/dup 0700 appone appone -\nd /var/lib/dup 0755 root root -\n",
            &mut index,
        );
        assert_eq!(
            index.get("/var/lib/dup").map(|e| e.line.as_str()),
            Some("d /var/lib/dup 0700 appone appone -")
        );
    }

    #[test]
    fn state_directory_uses_relative_path_and_records_the_unit() {
        // /var/lib/postgresql/data searches for StateDirectory=postgresql/data,
        // not the bare leaf "data" the old rsplit('/').next() code produced.
        let exec = MockExecutor::new().with_command(
            r"grep -rE --include=*.service --include=*.socket -l StateDirectory\s*=\s*postgresql/data /usr/lib/systemd/system/ /etc/systemd/system/",
            grep_success("/usr/lib/systemd/system/postgresql.service\n"),
        );
        let result = detect_var_dir_backing(
            &exec,
            "/var/lib/postgresql/data",
            &HashSet::new(),
            &TmpfilesIndex::new(),
        );
        assert_eq!(
            result,
            VarDirBacking::StateDirectory {
                unit: "postgresql.service".into()
            }
        );
    }

    #[test]
    fn state_directory_ignores_a_bare_leaf_match() {
        // The negative half of the test above: a unit declaring
        // StateDirectory=data does not back /var/lib/postgresql/data.
        let exec = MockExecutor::new().with_command(
            r"grep -rE --include=*.service --include=*.socket -l StateDirectory\s*=\s*data /usr/lib/systemd/system/ /etc/systemd/system/",
            grep_success("/usr/lib/systemd/system/other.service\n"),
        );
        let result = detect_var_dir_backing(
            &exec,
            "/var/lib/postgresql/data",
            &HashSet::new(),
            &TmpfilesIndex::new(),
        );
        assert_eq!(result, VarDirBacking::Unbacked);
    }

    #[test]
    fn cache_directory_detected() {
        let exec = MockExecutor::new().with_command(
            r"grep -rE --include=*.service --include=*.socket -l CacheDirectory\s*=\s*httpd /usr/lib/systemd/system/ /etc/systemd/system/",
            grep_success("/usr/lib/systemd/system/httpd.service\n"),
        );
        let result = detect_var_dir_backing(
            &exec,
            "/var/cache/httpd",
            &HashSet::new(),
            &TmpfilesIndex::new(),
        );
        assert_eq!(
            result,
            VarDirBacking::CacheDirectory {
                unit: "httpd.service".into()
            }
        );
    }

    #[test]
    fn logs_directory_detected() {
        let exec = MockExecutor::new().with_command(
            r"grep -rE --include=*.service --include=*.socket -l LogsDirectory\s*=\s*nginx /usr/lib/systemd/system/ /etc/systemd/system/",
            grep_success("/usr/lib/systemd/system/nginx.service\n"),
        );
        let result = detect_var_dir_backing(
            &exec,
            "/var/log/nginx",
            &HashSet::new(),
            &TmpfilesIndex::new(),
        );
        assert_eq!(
            result,
            VarDirBacking::LogsDirectory {
                unit: "nginx.service".into()
            }
        );
    }

    #[test]
    fn rpm_owned_records_the_owning_package() {
        let exec = MockExecutor::new()
            .with_command(&owning_package_key("/var/lib/rpm"), grep_success("rpm\n"));
        let rpm_owned = HashSet::from(["/var/lib/rpm".to_string()]);
        let result =
            detect_var_dir_backing(&exec, "/var/lib/rpm", &rpm_owned, &TmpfilesIndex::new());
        assert_eq!(
            result,
            VarDirBacking::RpmOwned {
                package: "rpm".into()
            }
        );
    }

    /// A directory can be claimed by more than one mechanism at once —
    /// a `d` line and a `StateDirectory=` for the same path is ordinary.
    /// The order the three checks run in picks which payload the
    /// snapshot carries, and Task 19 branches on that payload, so the
    /// order is part of the contract rather than an implementation
    /// detail. Each case leaves the earlier mechanisms live, so the
    /// assertion fails if any of the three moves.
    #[test]
    fn a_tmpfiles_entry_outranks_both_a_directive_and_rpm_ownership() {
        let path = "/var/lib/myapp";
        let exec = MockExecutor::new()
            .with_command(
                r"grep -rE --include=*.service --include=*.socket -l StateDirectory\s*=\s*myapp /usr/lib/systemd/system/ /etc/systemd/system/",
                grep_success("/usr/lib/systemd/system/myapp.service\n"),
            )
            .with_command(&owning_package_key(path), grep_success("myapp\n"));
        let rpm_owned = HashSet::from([path.to_string()]);
        let mut tmpfiles = TmpfilesIndex::new();
        index_tmpfiles_fragment(
            "/etc/tmpfiles.d/myapp.conf",
            "d /var/lib/myapp 0750 myapp myapp -\n",
            &mut tmpfiles,
        );

        assert_eq!(
            detect_var_dir_backing(&exec, path, &rpm_owned, &tmpfiles),
            VarDirBacking::Tmpfiles {
                conf: "/etc/tmpfiles.d/myapp.conf".into(),
                line: "d /var/lib/myapp 0750 myapp myapp -".into(),
            }
        );
    }

    #[test]
    fn a_directive_outranks_rpm_ownership() {
        let path = "/var/lib/myapp";
        let exec = MockExecutor::new()
            .with_command(
                r"grep -rE --include=*.service --include=*.socket -l StateDirectory\s*=\s*myapp /usr/lib/systemd/system/ /etc/systemd/system/",
                grep_success("/usr/lib/systemd/system/myapp.service\n"),
            )
            .with_command(&owning_package_key(path), grep_success("myapp\n"));
        let rpm_owned = HashSet::from([path.to_string()]);

        // Empty by construction, so the tmpfiles branch misses for a
        // reason a reader can see rather than by a fixture that happens
        // not to name this path.
        assert_eq!(
            detect_var_dir_backing(&exec, path, &rpm_owned, &TmpfilesIndex::new()),
            VarDirBacking::StateDirectory {
                unit: "myapp.service".into()
            }
        );
    }

    #[test]
    fn rpm_owned_without_a_named_owner_is_unbacked() {
        // The negative half: in the owned set, but rpm will not name the
        // owner. An RpmOwned payload with no package is not a verdict.
        let exec = MockExecutor::new();
        let rpm_owned = HashSet::from(["/var/lib/rpm".to_string()]);
        let result =
            detect_var_dir_backing(&exec, "/var/lib/rpm", &rpm_owned, &TmpfilesIndex::new());
        assert_eq!(result, VarDirBacking::Unbacked);
    }

    #[test]
    fn unbacked_fallback() {
        let exec = MockExecutor::new();
        let result = detect_var_dir_backing(
            &exec,
            "/var/lib/custom-app/data",
            &HashSet::new(),
            &TmpfilesIndex::new(),
        );
        assert_eq!(result, VarDirBacking::Unbacked);
    }

    #[test]
    fn discover_var_dirs_depth2_with_dedup() {
        // find under /var/lib returns both depth-1 and depth-2 entries;
        // the parent should be deduplicated away when a child exists.
        let exec = MockExecutor::new()
            .with_command(
                "find /var/lib -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: "/var/lib/pgsql\n/var/lib/pgsql/data\n/var/lib/rpm\n".to_string(),
                    stderr: String::new(),
                    exit_code: 0,
                },
            )
            .with_command(
                "find /var/log -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: "/var/log/myapp\n".to_string(),
                    stderr: String::new(),
                    exit_code: 0,
                },
            )
            .with_command(
                "find /var/cache -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 1,
                },
            );

        let (dirs, _warnings) = discover_var_directories(&exec);
        let paths: Vec<&str> = dirs.iter().map(|d| d.path.as_str()).collect();

        assert!(
            !paths.contains(&"/var/lib/pgsql"),
            "parent should be deduplicated when child exists"
        );
        assert!(
            paths.contains(&"/var/lib/pgsql/data"),
            "depth-2 leaf should be kept"
        );
        assert!(
            paths.contains(&"/var/lib/rpm"),
            "depth-1 leaf with no children should be kept"
        );
        assert!(
            paths.contains(&"/var/log/myapp"),
            "depth-1 entry from another base should be kept"
        );
    }

    #[test]
    fn discover_var_directories_captures_ownership_root() {
        let exec = MockExecutor::new()
            .with_command(
                "find /var/lib -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: "/var/lib/rpm\n".to_string(),
                    stderr: String::new(),
                    exit_code: 0,
                },
            )
            .with_command(
                "find /var/log -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 1,
                },
            )
            .with_command(
                "find /var/cache -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 1,
                },
            )
            .with_command(
                "stat -c %04a %u %g %U %G /var/lib/rpm",
                ExecResult {
                    stdout: "0755 0 0 root root\n".to_string(),
                    stderr: String::new(),
                    exit_code: 0,
                },
            );

        let (dirs, warnings) = discover_var_directories(&exec);
        assert!(warnings.is_empty());
        assert_eq!(dirs.len(), 1);
        let dir = &dirs[0];
        assert_eq!(dir.path, "/var/lib/rpm");
        assert_eq!(dir.mode, Some("0755".to_string()));
        assert_eq!(dir.owner_uid, Some(0));
        assert_eq!(dir.owner_gid, Some(0));
        assert_eq!(dir.owner_name, Some("root".to_string()));
        assert_eq!(dir.group_name, Some("root".to_string()));
    }

    #[test]
    fn discover_var_directories_captures_ownership_service_account() {
        let exec = MockExecutor::new()
            .with_command(
                "find /var/lib -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: "/var/lib/pgsql/data\n".to_string(),
                    stderr: String::new(),
                    exit_code: 0,
                },
            )
            .with_command(
                "find /var/log -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 1,
                },
            )
            .with_command(
                "find /var/cache -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 1,
                },
            )
            .with_command(
                "stat -c %04a %u %g %U %G /var/lib/pgsql/data",
                ExecResult {
                    stdout: "0750 26 26 postgres postgres\n".to_string(),
                    stderr: String::new(),
                    exit_code: 0,
                },
            );

        let (dirs, warnings) = discover_var_directories(&exec);
        assert!(warnings.is_empty());
        assert_eq!(dirs.len(), 1);
        let dir = &dirs[0];
        assert_eq!(dir.path, "/var/lib/pgsql/data");
        assert_eq!(dir.mode, Some("0750".to_string()));
        assert_eq!(dir.owner_uid, Some(26));
        assert_eq!(dir.owner_gid, Some(26));
        assert_eq!(dir.owner_name, Some("postgres".to_string()));
        assert_eq!(dir.group_name, Some("postgres".to_string()));
    }

    #[test]
    fn discover_var_directories_captures_non_default_mode() {
        let exec = MockExecutor::new()
            .with_command(
                "find /var/lib -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: "/var/lib/shared\n".to_string(),
                    stderr: String::new(),
                    exit_code: 0,
                },
            )
            .with_command(
                "find /var/log -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 1,
                },
            )
            .with_command(
                "find /var/cache -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 1,
                },
            )
            .with_command(
                "stat -c %04a %u %g %U %G /var/lib/shared",
                ExecResult {
                    stdout: "2770 1000 100 webadmin webgroup\n".to_string(),
                    stderr: String::new(),
                    exit_code: 0,
                },
            );

        let (dirs, warnings) = discover_var_directories(&exec);
        assert!(warnings.is_empty());
        assert_eq!(dirs.len(), 1);
        let dir = &dirs[0];
        assert_eq!(dir.path, "/var/lib/shared");
        assert_eq!(dir.mode, Some("2770".to_string()));
        assert_eq!(dir.owner_uid, Some(1000));
        assert_eq!(dir.owner_gid, Some(100));
        assert_eq!(dir.owner_name, Some("webadmin".to_string()));
        assert_eq!(dir.group_name, Some("webgroup".to_string()));
    }

    #[test]
    fn discover_var_directories_stat_failure_degradation() {
        let exec = MockExecutor::new()
            .with_command(
                "find /var/lib -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: "/var/lib/missing\n".to_string(),
                    stderr: String::new(),
                    exit_code: 0,
                },
            )
            .with_command(
                "find /var/log -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 1,
                },
            )
            .with_command(
                "find /var/cache -mindepth 1 -maxdepth 2 -type d ! -empty",
                ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 1,
                },
            )
            .with_command(
                "stat -c %04a %u %g %U %G /var/lib/missing",
                ExecResult {
                    stdout: String::new(),
                    stderr: "stat: cannot stat '/var/lib/missing': No such file or directory\n"
                        .to_string(),
                    exit_code: 1,
                },
            );

        let (dirs, warnings) = discover_var_directories(&exec);
        assert_eq!(warnings.len(), 1, "stat failure should emit a warning");
        assert!(warnings[0].message.contains("/var/lib/missing"));
        assert!(
            warnings[0]
                .message
                .contains("tmpfiles.d provisioning unavailable")
        );
        assert_eq!(dirs.len(), 1);
        let dir = &dirs[0];
        assert_eq!(dir.path, "/var/lib/missing");
        // All ownership/mode fields should be None on stat failure
        assert_eq!(dir.mode, None);
        assert_eq!(dir.owner_uid, None);
        assert_eq!(dir.owner_gid, None);
        assert_eq!(dir.owner_name, None);
        assert_eq!(dir.group_name, None);
    }
}
