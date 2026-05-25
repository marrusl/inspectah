pub mod attention;
pub mod diff;
pub mod variant_ops;

use std::collections::BTreeMap;

use inspectah_core::snapshot::InspectionSnapshot;

use crate::types::FleetContext;

/// Summary of config-file variant distribution across a fleet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantSummary {
    /// Number of config-file paths that have 2+ distinct content variants.
    pub paths_with_variants: usize,
    /// Per-path variant distribution. Key is the config-file path (sorted).
    pub variant_distribution: BTreeMap<String, VariantInfo>,
}

/// Variant information for a single config-file path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantInfo {
    /// How many distinct content variants exist for this path.
    pub variant_count: usize,
    /// Host counts for each variant, sorted descending (most prevalent first).
    pub host_split: Vec<usize>,
}

/// Compute a summary of config-file variant distribution for a fleet session.
///
/// Returns `None` for non-fleet sessions (no `FleetContext`). For fleet
/// sessions, groups config entries by path, identifies paths with multiple
/// content variants, and builds host-split vectors from `FleetPrevalence`.
pub fn variant_summary(
    snapshot: &InspectionSnapshot,
    fleet_ctx: Option<&FleetContext>,
) -> Option<VariantSummary> {
    // Non-fleet sessions have no variant summary.
    let _ctx = fleet_ctx?;

    let config = snapshot.config.as_ref()?;

    // Group config entries by path. Each path may have multiple entries
    // (one per variant).
    let mut by_path: BTreeMap<&str, Vec<&inspectah_core::types::config::ConfigFileEntry>> =
        BTreeMap::new();
    for entry in &config.files {
        by_path.entry(entry.path.as_str()).or_default().push(entry);
    }

    let mut variant_distribution = BTreeMap::new();

    for (path, entries) in &by_path {
        if entries.len() < 2 {
            continue;
        }

        let mut host_split: Vec<usize> = entries
            .iter()
            .map(|e| {
                e.fleet
                    .as_ref()
                    .map(|f| f.count.max(0) as usize)
                    .unwrap_or(0)
            })
            .collect();
        host_split.sort_unstable_by(|a, b| b.cmp(a)); // descending

        variant_distribution.insert(
            (*path).to_string(),
            VariantInfo {
                variant_count: entries.len(),
                host_split,
            },
        );
    }

    Some(VariantSummary {
        paths_with_variants: variant_distribution.len(),
        variant_distribution,
    })
}
