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
};

use super::service_intent::is_package_installable;

/// A backing names a unit by filename, so the paths that could be that
/// unit are narrowed before matching.
///
/// Narrowed to the *system* unit directories, not to every directory
/// with `systemd` in it. A `StateDirectory=` verdict came from a unit
/// the system manager runs, and systemd searches `.../systemd/user/`
/// only for units a per-user manager runs: the two have separate search
/// paths, so a user unit sharing the filename shares nothing else and
/// cannot take over when the system unit is dropped. Treating it as
/// evidence is the same mistake as matching on the filename alone, one
/// level in.
const SYSTEM_UNIT_DIR_MARKER: &str = "/systemd/system/";

/// The system unit directory that takes precedence over the vendor copy
/// of the same filename.
const ETC_UNIT_PREFIX: &str = "/etc/systemd/system/";

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

/// The two sets the proof is made from (§ 3.4), plus the lookups that
/// place a creator in them.
struct Evidence<'a> {
    /// Every `config.files` entry's path and whether it is included.
    config_files: HashMap<&'a str, bool>,
    /// Unit filename to the `config.files` entry naming it: the entry's
    /// own path and its include decision. The path is kept because an
    /// excluded entry is only the end of the story for the file at that
    /// path -- a unit file of the same name somewhere else survives it.
    config_units: HashMap<&'a str, (&'a str, bool)>,
    /// Every path `rpm.file_ownership` names, to its owning package.
    file_owners: HashMap<&'a str, &'a str>,
    /// Unit filename to the path of the unit file RPM owns and the
    /// owning package. A `/etc` path is kept only while no other path
    /// claims the filename, because the vendor copy is the one that
    /// survives an excluded `/etc` override.
    unit_owners: HashMap<&'a str, (&'a str, &'a str)>,
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
        let mut config_units = HashMap::new();
        if let Some(config) = &snap.config {
            for entry in &config.files {
                let included = entry.disposition.is_included();
                config_files.insert(entry.path.as_str(), included);
                if let Some(unit) = unit_filename(&entry.path) {
                    config_units
                        .entry(unit)
                        .or_insert((entry.path.as_str(), included));
                }
            }
        }

        let mut file_owners = HashMap::new();
        let mut unit_owners = HashMap::new();
        let mut install_set = HashSet::new();
        let mut baseline = None;
        if let Some(rpm) = &snap.rpm {
            for entry in &rpm.file_ownership {
                for path in &entry.paths {
                    file_owners
                        .entry(path.as_str())
                        .or_insert(entry.package_name.as_str());
                    if let Some(unit) = unit_filename(path) {
                        let owner = (path.as_str(), entry.package_name.as_str());
                        match unit_owners.get(unit) {
                            None => {
                                unit_owners.insert(unit, owner);
                            }
                            // The vendor copy displaces a recorded `/etc`
                            // one: it is the file systemd runs once the
                            // `/etc` override is gone.
                            Some((recorded, _))
                                if recorded.starts_with(ETC_UNIT_PREFIX)
                                    && !path.starts_with(ETC_UNIT_PREFIX) =>
                            {
                                unit_owners.insert(unit, owner);
                            }
                            Some(_) => {}
                        }
                    }
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
            config_units,
            file_owners,
            unit_owners,
            install_set,
            baseline,
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

/// The filename of a path under a systemd *system* unit directory, and
/// `None` for anything else, user units included.
fn unit_filename(path: &str) -> Option<&str> {
    if !path.contains(SYSTEM_UNIT_DIR_MARKER) {
        return None;
    }
    Path::new(path).file_name()?.to_str()
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

        VarDirBacking::StateDirectory { unit }
        | VarDirBacking::CacheDirectory { unit }
        | VarDirBacking::LogsDirectory { unit } => match evidence.config_units.get(unit.as_str()) {
            Some((_, true)) => EffectiveBacking::Carried,
            // Dropping one unit file does not drop the unit. A host can
            // have an `/etc` override and an RPM-owned vendor unit of the
            // same filename, both declaring the directory; systemd runs
            // the `/etc` one while it exists and the vendor one when it
            // does not. So an excluded entry only loses the creator when
            // no other unit file of that name reaches the image, and
            // that question is settled by full path, not by filename.
            Some((config_path, false)) => match evidence.unit_owners.get(unit.as_str()) {
                Some((owner_path, package)) if owner_path != config_path => {
                    from_carriage(evidence.carriage(package), unit)
                }
                _ => EffectiveBacking::Provision {
                    lost: Some(unit.clone()),
                },
            },
            None => match evidence.unit_owners.get(unit.as_str()) {
                Some((_, package)) => from_carriage(evidence.carriage(package), unit),
                None => EffectiveBacking::Unresolved {
                    creator: unit.clone(),
                    reason: UnresolvedReason::UnownedUnit,
                },
            },
        },

        VarDirBacking::RpmOwned { package } => from_carriage(evidence.carriage(package), package),
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
