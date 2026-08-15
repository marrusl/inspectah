//! Advisory findings shared across the `render_all` artifacts.
//!
//! An advisory is a finding the user cannot act on through include/exclude:
//! it carries a rationale and nothing else. The `/var` and full-shadow
//! advisories reach the renderers through dedicated snapshot fields, but the
//! config inspector folds its advisories into `ConfigFileEntry.disposition`,
//! where every include-filtered table drops them alongside the entries the
//! user excluded on purpose. This module lifts them back out so each
//! renderer presents them as advisories rather than losing them.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::{AdvisoryType, FindingKind};

/// A config-borne advisory finding, borrowed from the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigAdvisory<'a> {
    /// Path of the config file the advisory is about.
    pub path: &'a str,
    /// Why this finding is advisory rather than actionable.
    pub advisory_type: &'a AdvisoryType,
    /// Human-readable explanation carried on the disposition.
    pub rationale: &'a str,
}

/// Collect every config entry whose disposition is `FindingKind::Advisory`.
///
/// Today that is the cross-tree symlink detector and the modernization
/// pattern catalog, both in the config inspector. The walk is variant-driven
/// rather than type-driven so a new `AdvisoryType` reaches the renderers
/// without a change here.
pub fn config_advisories(snap: &InspectionSnapshot) -> Vec<ConfigAdvisory<'_>> {
    let Some(config) = snap.config.as_ref() else {
        return Vec::new();
    };

    config
        .files
        .iter()
        .filter_map(|f| match &f.disposition {
            FindingKind::Advisory {
                advisory_type,
                rationale,
            } => Some(ConfigAdvisory {
                path: &f.path,
                advisory_type,
                rationale,
            }),
            _ => None,
        })
        .collect()
}

/// Serialized discriminant of an `AdvisoryType` (e.g. `modernization`).
///
/// Renderers surface the discriminant in labels and aria text; going
/// through serde keeps those strings tied to the JSON contract instead of
/// to a hand-written match that can drift from it.
pub fn advisory_type_str(advisory_type: &AdvisoryType) -> String {
    serde_json::to_string(advisory_type)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};

    fn snap_with_configs(files: Vec<ConfigFileEntry>) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection { files });
        snap
    }

    #[test]
    fn collects_every_advisory_variant_and_nothing_else() {
        let snap = snap_with_configs(vec![
            ConfigFileEntry {
                path: "/etc/init.d/legacy-app".into(),
                disposition: FindingKind::advisory(
                    AdvisoryType::Modernization,
                    "sysvinit script — port to a systemd unit",
                ),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/alternatives/java".into(),
                disposition: FindingKind::advisory(
                    AdvisoryType::CrossTreeSymlink,
                    "symlink crosses into /usr",
                ),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/motd".into(),
                disposition: FindingKind::excluded(),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/hosts".into(),
                disposition: FindingKind::inventory(),
                ..Default::default()
            },
        ]);

        let advisories = config_advisories(&snap);

        assert_eq!(
            advisories.len(),
            2,
            "only advisory dispositions are advisories: {advisories:?}"
        );
        assert_eq!(advisories[0].path, "/etc/init.d/legacy-app");
        assert_eq!(advisories[0].advisory_type, &AdvisoryType::Modernization);
        assert_eq!(
            advisories[0].rationale,
            "sysvinit script — port to a systemd unit"
        );
        assert_eq!(advisories[1].path, "/etc/alternatives/java");
        assert_eq!(advisories[1].advisory_type, &AdvisoryType::CrossTreeSymlink);
    }

    #[test]
    fn absent_config_section_yields_no_advisories() {
        assert!(config_advisories(&InspectionSnapshot::new()).is_empty());
    }

    #[test]
    fn advisory_type_str_matches_the_json_contract() {
        assert_eq!(
            advisory_type_str(&AdvisoryType::Modernization),
            "modernization"
        );
        assert_eq!(
            advisory_type_str(&AdvisoryType::CrossTreeSymlink),
            "cross_tree_symlink"
        );
        assert_eq!(
            advisory_type_str(&AdvisoryType::UnbackedVarDir),
            "unbacked_var_dir"
        );
    }
}
