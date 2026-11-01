//! Effective `/var` backing at export (spec § 3.4).
//!
//! The snapshot verdict says what created a directory on the source
//! host. This module answers the different question the export has to
//! answer: does that creator reach the image? A creator "reaches the
//! image" only on proof; where the export has no proof either way the
//! outcome says so rather than guessing.
//!
//! The snapshot verdict is never rewritten — [`VarDirOutcome`] carries
//! it beside the outcome so the audit report and the view can show both.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::storage::{
    ETC_TMPFILES_PREFIX, RUN_TMPFILES_PREFIX, USR_TMPFILES_PREFIX, VarDirBacking,
    declaring_unit_label,
};

use super::service_intent::is_package_installable;

/// The directories a unit file can sit in and still be a unit file this
/// export can reason about.
///
/// The collector's grep is recursive over these two roots, so a file
/// matching `*.service` inside `multi-user.target.wants/`, a
/// `<unit>.service.d/` drop-in directory, or any other nested directory
/// reaches `declared_by` even though the system manager would never run
/// it as a unit. Matched against a path's parent exactly, because a
/// substring test does not work here and three rounds of narrowing one
/// proved it: `/systemd/` admitted user units, `/systemd/system/`
/// admitted `multi-user.target.wants/` links, and excluding those would
/// have admitted `.requires/` and drop-in `.d/` directories next. A unit
/// file lives at exactly `<unit-dir>/<name>` and is never nested, so an
/// exact parent match rules out every nested shape at once rather than
/// one per round.
const SYSTEM_UNIT_DIRS: [&str; 2] = ["/etc/systemd/system", "/usr/lib/systemd/system"];

/// The system unit directory whose files the export writes itself,
/// rather than carrying inside a package. Which of the two roots a
/// declaring path sits in decides whether an operator's include
/// decision or a package's carriage settles it.
const ETC_UNIT_DIR: &str = "/etc/systemd/system";

/// What the export can say about one `/var` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectiveBacking {
    /// A creator the export carries already makes this directory.
    Carried,
    /// The export must make the directory itself. `lost` names the
    /// creator the source host had and this export does not carry, when
    /// the verdict named one.
    Provision { lost: Option<String> },
    /// The export can neither place the creator nor rule it out.
    Unresolved {
        creator: String,
        reason: UnresolvedReason,
    },
}

/// Why an outcome is [`EffectiveBacking::Unresolved`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedReason {
    /// No `rpm.file_ownership` entry names the tmpfiles.d fragment.
    UnownedFragment,
    /// The unit is neither a `config.files` entry nor RPM-owned.
    UnownedUnit,
    /// No baseline is known, so the target base package set is unknown.
    BaseUnknown,
}

impl UnresolvedReason {
    /// The clause the FIXME, the audit report, and the view's rationale
    /// all use, so one wording serves the three surfaces.
    pub fn reason_text(&self) -> &'static str {
        match self {
            Self::UnownedFragment => "no package owns that file",
            Self::UnownedUnit => "no package owns that unit",
            Self::BaseUnknown => "the target base package set is unknown",
        }
    }
}

/// One directory's source verdict beside the outcome the export reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarDirOutcome {
    pub path: String,
    pub source: Option<VarDirBacking>,
    pub outcome: EffectiveBacking,
}

/// Reconcile every `/var` directory's verdict against what the export
/// carries, in `storage.var_directories` order.
pub fn effective_var_backing(snap: &InspectionSnapshot) -> Vec<VarDirOutcome> {
    let Some(storage) = &snap.storage else {
        return Vec::new();
    };
    let evidence = Evidence::from_snapshot(snap);
    storage
        .var_directories
        .iter()
        .map(|d| VarDirOutcome {
            path: d.path.clone(),
            source: d.backing.clone(),
            outcome: classify(d.backing.as_ref(), &evidence),
        })
        .collect()
}

/// Whether the export carries the package a creator belongs to.
enum Carriage {
    Carried,
    Lost,
    Unknown,
}

/// Whether one unit file reaches the image, carrying the reason when
/// the export cannot tell.
enum UnitReach {
    Carried,
    Lost,
    Unresolved(UnresolvedReason),
}

/// The two sets the proof is made from (§ 3.4), plus the lookups that
/// place a creator in them.
struct Evidence<'a> {
    /// Every `config.files` entry's path and whether it is included.
    config_files: HashMap<&'a str, bool>,
    /// The live include decision for each `/etc` unit file, by full
    /// path.
    ///
    /// Not `config_files`, because a unit file the services inspector
    /// also records carries an `Inventory` `config.files` row whose
    /// `is_included()` is false while the operator's actual decision,
    /// in `services.drop_ins`, may well be to carry it. Reading the
    /// config row alone would call every full shadow excluded.
    etc_unit_includes: HashMap<&'a str, bool>,
    /// Every path `rpm.file_ownership` names, to its owning package.
    file_owners: HashMap<&'a str, &'a str>,
    /// The included `packages_added` entries the export installs.
    ///
    /// Deliberately wider than the Containerfile's own install loop,
    /// which also applies the leaf filter, `baseline_suppressed`, and
    /// group-member suppression. Each of those three drops a package
    /// that is in the built image anyway — a transitive dependency dnf
    /// pulls in, one the base already has, one its group installs — so
    /// counting them as carried is the correct answer, and the loop's
    /// set would read a directory backed by a transitively installed
    /// package as `Provision`.
    ///
    /// The repo-enabling demotion is the one filter that removes a
    /// package the image will not have, so it is applied here.
    install_set: HashSet<&'a str>,
    /// `None` means no base set is known, which is not the same as an
    /// empty one: membership cannot be ruled out either way.
    baseline: Option<HashSet<&'a str>>,
}

impl<'a> Evidence<'a> {
    fn from_snapshot(snap: &'a InspectionSnapshot) -> Self {
        let mut config_files = HashMap::new();
        let mut etc_unit_includes = HashMap::new();
        if let Some(config) = &snap.config {
            for entry in &config.files {
                let included = entry.disposition.is_included();
                config_files.insert(entry.path.as_str(), included);
                // An inventory row is a shadow of a decision taken
                // elsewhere, so it is not the answer for this path.
                if parent_dir(&entry.path) == Some(ETC_UNIT_DIR)
                    && !entry.disposition.is_inventory()
                {
                    etc_unit_includes.insert(entry.path.as_str(), included);
                }
            }
        }
        if let Some(services) = &snap.services {
            for dropin in &services.drop_ins {
                if parent_dir(&dropin.path) == Some(ETC_UNIT_DIR) {
                    etc_unit_includes
                        .insert(dropin.path.as_str(), dropin.disposition.is_included());
                }
            }
        }

        let mut file_owners = HashMap::new();
        let mut install_set = HashSet::new();
        let mut baseline = None;
        if let Some(rpm) = &snap.rpm {
            for entry in &rpm.file_ownership {
                for path in &entry.paths {
                    file_owners
                        .entry(path.as_str())
                        .or_insert(entry.package_name.as_str());
                }
            }
            install_set = rpm
                .packages_added
                .iter()
                .filter(|pkg| pkg.disposition.is_included())
                // A repo-less entry whose RPM the export carries counts
                // too: a staged upload or a cached RPM the payload
                // extraction carries.
                .filter(|pkg| is_package_installable(pkg) || pkg.uploaded || pkg.repoless_cached)
                // A repo-enabling package whose serving repo the export
                // dropped is demoted to a FIXME and never installed.
                .filter(|pkg| {
                    !matches!(
                        super::repo_enabling::repo_enabling_export(rpm, pkg),
                        super::repo_enabling::RepoEnablingExport::Lost(_)
                    )
                })
                .map(|pkg| pkg.name.as_str())
                .collect();
            baseline = rpm
                .baseline_package_names
                .as_ref()
                .map(|names| names.iter().map(String::as_str).collect());
        }

        Self {
            config_files,
            etc_unit_includes,
            file_owners,
            install_set,
            baseline,
        }
    }

    /// Whether the unit file at `path` reaches the image.
    ///
    /// An `/etc` unit file is carried on the operator's decision alone:
    /// the export writes the file itself. A vendor unit file is carried
    /// with its package.
    fn unit_file_reach(&self, path: &str) -> UnitReach {
        let Some(parent) = parent_dir(path) else {
            return UnitReach::Lost;
        };
        if !SYSTEM_UNIT_DIRS.contains(&parent) {
            // Nested under a unit directory rather than in one: an
            // enablement link, a drop-in, or a file that merely ends in
            // `.service`. The system manager runs none of them, so a
            // directive in one creates nothing.
            return UnitReach::Lost;
        }
        if parent == ETC_UNIT_DIR {
            // The config walk records every admin-written unit file in
            // this directory, so a path neither it nor the services
            // inspector names is one no decision carries.
            return match self.etc_unit_includes.get(path) {
                Some(true) => UnitReach::Carried,
                _ => UnitReach::Lost,
            };
        }
        match self.file_owners.get(path) {
            Some(package) => match self.carriage(package) {
                Carriage::Carried => UnitReach::Carried,
                Carriage::Lost => UnitReach::Lost,
                Carriage::Unknown => UnitReach::Unresolved(UnresolvedReason::BaseUnknown),
            },
            None => UnitReach::Unresolved(UnresolvedReason::UnownedUnit),
        }
    }

    /// A provider package is carried when it is in the exported install
    /// set or in the known target base, lost when a baseline is known
    /// and it is in neither, unknown when no baseline is known.
    fn carriage(&self, package: &str) -> Carriage {
        if self.install_set.contains(package) {
            return Carriage::Carried;
        }
        match &self.baseline {
            Some(baseline) if baseline.contains(package) => Carriage::Carried,
            Some(_) => Carriage::Lost,
            None => Carriage::Unknown,
        }
    }
}

/// The directory a path sits in directly, as a string.
fn parent_dir(path: &str) -> Option<&str> {
    Path::new(path).parent()?.to_str()
}

/// The § 3.4 table, one arm per source backing.
fn classify(backing: Option<&VarDirBacking>, evidence: &Evidence) -> EffectiveBacking {
    // A directory with no verdict carries no creator to place, which is
    // what `Unbacked` means for the export.
    let Some(backing) = backing else {
        return EffectiveBacking::Provision { lost: None };
    };

    match backing {
        VarDirBacking::Unbacked => EffectiveBacking::Provision { lost: None },

        VarDirBacking::Tmpfiles { conf, .. } => {
            if conf.starts_with(RUN_TMPFILES_PREFIX) {
                // A runtime fragment is not image content, so the
                // directory is always the export's to make.
                EffectiveBacking::Provision {
                    lost: Some(conf.clone()),
                }
            } else if conf.starts_with(USR_TMPFILES_PREFIX) {
                match evidence.file_owners.get(conf.as_str()) {
                    Some(package) => from_carriage(evidence.carriage(package), conf),
                    None => EffectiveBacking::Unresolved {
                        creator: conf.clone(),
                        reason: UnresolvedReason::UnownedFragment,
                    },
                }
            } else {
                // An `/etc` fragment follows the config decision and
                // never the provider: the export carries the file
                // itself. The collector reads no directory other than
                // the three, so this arm is the `/etc` one.
                debug_assert!(conf.starts_with(ETC_TMPFILES_PREFIX));
                match evidence.config_files.get(conf.as_str()) {
                    Some(true) => EffectiveBacking::Carried,
                    _ => EffectiveBacking::Provision {
                        lost: Some(conf.clone()),
                    },
                }
            }
        }

        VarDirBacking::StateDirectory { declared_by }
        | VarDirBacking::CacheDirectory { declared_by }
        | VarDirBacking::LogsDirectory { declared_by } => classify_declarers(declared_by, evidence),

        VarDirBacking::RpmOwned { package } => from_carriage(evidence.carriage(package), package),
    }
}

/// Reconcile the unit files that declare a directory against what the
/// export carries.
///
/// The question is whether a unit file *that declares this directory*
/// reaches the image, which is not the question "is a unit of that
/// filename carried". A vendor unit of the same name as an excluded
/// `/etc` shadow is routinely carried while declaring nothing, and
/// reading its installation as proof is what called an excluded
/// full shadow's directory `Carried`.
fn classify_declarers(declared_by: &[String], evidence: &Evidence) -> EffectiveBacking {
    let mut unresolved: Option<UnresolvedReason> = None;
    for path in declared_by {
        match evidence.unit_file_reach(path) {
            UnitReach::Carried => return EffectiveBacking::Carried,
            // One declarer the export cannot place leaves the whole
            // verdict unplaceable, however many others it ruled out.
            UnitReach::Unresolved(reason) => unresolved = unresolved.or(Some(reason)),
            UnitReach::Lost => {}
        }
    }
    let label = declaring_unit_label(declared_by);
    match unresolved {
        Some(reason) => EffectiveBacking::Unresolved {
            creator: label,
            reason,
        },
        // Every declarer is accounted for and none reaches the image.
        // An empty `declared_by` lands here too, with no creator to name.
        None if declared_by.is_empty() => EffectiveBacking::Provision { lost: None },
        None => EffectiveBacking::Provision { lost: Some(label) },
    }
}

fn from_carriage(carriage: Carriage, creator: &str) -> EffectiveBacking {
    match carriage {
        Carriage::Carried => EffectiveBacking::Carried,
        Carriage::Lost => EffectiveBacking::Provision {
            lost: Some(creator.to_string()),
        },
        Carriage::Unknown => EffectiveBacking::Unresolved {
            creator: creator.to_string(),
            reason: UnresolvedReason::BaseUnknown,
        },
    }
}
