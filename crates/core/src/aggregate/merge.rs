use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::types::aggregate::{AggregatePrevalence, RepoSourceEntry, VariantSelection};

/// Trait for types that participate in aggregate prevalence tracking.
///
/// Every type that can be merged across multiple host snapshots implements
/// this trait. The generic merge engine uses these methods to group items
/// by identity, attach prevalence metadata, and handle content variants.
pub trait AggregateMergeable: Clone {
    /// Unique identity key for grouping across hosts.
    fn identity_key(&self) -> Cow<'_, str>;

    /// Mutable reference to the aggregate prevalence field.
    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence>;

    /// Set the include flag on this item.
    fn set_include(&mut self, val: bool);

    /// Mutable reference to the variant selection field, if this type supports variants.
    fn variant_selection_mut(&mut self) -> Option<&mut VariantSelection> {
        None
    }

    /// Content-based variant key for detecting differing content at the same path.
    /// Returns `None` for types without content variants (e.g., packages).
    fn content_variant_key(&self) -> Option<Cow<'_, str>> {
        None
    }
}

// ---------------------------------------------------------------------------
// RPM types
// ---------------------------------------------------------------------------

use crate::types::rpm::{EnabledModuleStream, PackageEntry, RepoFile, VersionLockEntry};

impl AggregateMergeable for PackageEntry {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}.{}", self.name, self.arch))
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl AggregateMergeable for RepoFile {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl AggregateMergeable for EnabledModuleStream {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}:{}", self.module_name, self.stream))
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl AggregateMergeable for VersionLockEntry {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}.{}", self.name, self.arch))
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Config types
// ---------------------------------------------------------------------------

use crate::types::config::ConfigFileEntry;

impl AggregateMergeable for ConfigFileEntry {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }

    fn variant_selection_mut(&mut self) -> Option<&mut VariantSelection> {
        Some(&mut self.variant_selection)
    }

    fn content_variant_key(&self) -> Option<Cow<'_, str>> {
        use sha2::{Digest, Sha256};
        Some(Cow::Owned(format!(
            "{:x}",
            Sha256::digest(self.content.as_bytes())
        )))
    }
}

// ---------------------------------------------------------------------------
// Service types
// ---------------------------------------------------------------------------

use crate::types::services::{ServiceStateChange, SystemdDropIn};

impl AggregateMergeable for ServiceStateChange {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.unit)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl AggregateMergeable for SystemdDropIn {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }

    fn variant_selection_mut(&mut self) -> Option<&mut VariantSelection> {
        Some(&mut self.variant_selection)
    }

    fn content_variant_key(&self) -> Option<Cow<'_, str>> {
        use sha2::{Digest, Sha256};
        Some(Cow::Owned(format!(
            "{:x}",
            Sha256::digest(self.content.as_bytes())
        )))
    }
}

// ---------------------------------------------------------------------------
// Container types
// ---------------------------------------------------------------------------

use crate::types::containers::{ComposeFile, FlatpakApp, QuadletUnit};

impl AggregateMergeable for QuadletUnit {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }

    fn variant_selection_mut(&mut self) -> Option<&mut VariantSelection> {
        Some(&mut self.variant_selection)
    }

    fn content_variant_key(&self) -> Option<Cow<'_, str>> {
        use sha2::{Digest, Sha256};
        Some(Cow::Owned(format!(
            "{:x}",
            Sha256::digest(self.content.as_bytes())
        )))
    }
}

impl AggregateMergeable for ComposeFile {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }

    fn variant_selection_mut(&mut self) -> Option<&mut VariantSelection> {
        Some(&mut self.variant_selection)
    }

    fn content_variant_key(&self) -> Option<Cow<'_, str>> {
        use sha2::{Digest, Sha256};
        let serialized = serde_json::to_string(&self.images).unwrap_or_default();
        Some(Cow::Owned(format!(
            "{:x}",
            Sha256::digest(serialized.as_bytes())
        )))
    }
}

impl AggregateMergeable for FlatpakApp {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}.{}.{}", self.app_id, self.remote, self.branch))
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Network types
// ---------------------------------------------------------------------------

use crate::types::network::{FirewallZone, NMConnection};

impl AggregateMergeable for NMConnection {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl AggregateMergeable for FirewallZone {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Security types
// ---------------------------------------------------------------------------

use crate::types::selinux::SelinuxPortLabel;

impl AggregateMergeable for SelinuxPortLabel {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}:{}", self.protocol, self.port))
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Kernel/boot types
// ---------------------------------------------------------------------------

use crate::types::kernelboot::{KernelModule, SysctlOverride};

impl AggregateMergeable for KernelModule {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.name)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl AggregateMergeable for SysctlOverride {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.key)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Non-RPM types
// ---------------------------------------------------------------------------

use crate::types::nonrpm::{NonRpmItem, UnmanagedFile, UnmanagedFileSection};

impl AggregateMergeable for NonRpmItem {
    fn identity_key(&self) -> Cow<'_, str> {
        // Composite key: method determines the ecosystem, path provides
        // the environment scope. Falls back to name for legacy items
        // that lack a path (binary detection, etc.).
        if self.path.is_empty() {
            Cow::Borrowed(&self.name)
        } else {
            let ecosystem = match self.method.as_str() {
                "pip list" | "pip dist-info" | "venv" => "pip",
                "npm lockfile" => "npm",
                "gem lockfile" => "gem",
                _ => "other",
            };
            Cow::Owned(format!("{ecosystem}:{}", self.path))
        }
    }

    fn content_variant_key(&self) -> Option<Cow<'_, str>> {
        use sha2::{Digest, Sha256};
        // Hash the unified package list to detect divergence across hosts.
        let mut hasher = Sha256::new();
        hasher.update(self.method.as_bytes());
        hasher.update(b"\n");
        for pkg in &self.packages {
            hasher.update(format!("{}={}\n", pkg.name, pkg.version).as_bytes());
        }
        Some(Cow::Owned(format!("{:x}", hasher.finalize())))
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl AggregateMergeable for UnmanagedFile {
    fn identity_key(&self) -> Cow<'_, str> {
        // File path is the stable identity for unmanaged files.
        Cow::Borrowed(&self.path)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }

    fn content_variant_key(&self) -> Option<Cow<'_, str>> {
        // Use the content hash to detect divergent file content across hosts.
        // content_hash is added by Task 5a — empty in single-host mode.
        if self.content_hash.is_empty() {
            None
        } else {
            Some(Cow::Borrowed(&self.content_hash))
        }
    }

    fn variant_selection_mut(&mut self) -> Option<&mut VariantSelection> {
        Some(&mut self.variant_selection)
    }
}

// ---------------------------------------------------------------------------
// Scheduled task types
// ---------------------------------------------------------------------------

use crate::types::scheduled::{AtJob, CronJob, GeneratedTimerUnit, SystemdTimer};

impl AggregateMergeable for CronJob {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl AggregateMergeable for SystemdTimer {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.name)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl AggregateMergeable for AtJob {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.file)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl AggregateMergeable for GeneratedTimerUnit {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.name)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Storage types
// ---------------------------------------------------------------------------

use crate::types::storage::FstabEntry;

impl AggregateMergeable for FstabEntry {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.mount_point)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Generic merge engine
// ---------------------------------------------------------------------------

/// Merge items from multiple host snapshots into a deduplicated list with
/// aggregate prevalence metadata.
///
/// Each input tuple is `(host_index, item)` where `host_index` refers to
/// the position in `hostnames`. Items are grouped by identity key, and each
/// group gets a [`AggregatePrevalence`] recording how many hosts (and which ones)
/// contributed that item. Types that support content variants are further
/// sub-grouped by content hash (see [`merge_with_variants`]).
///
/// The returned list is sorted by identity key for deterministic output.
pub fn merge_items<T: AggregateMergeable + serde::Serialize>(
    items: Vec<(usize, T)>,
    total_hosts: usize,
    hostnames: &[String],
) -> Vec<T> {
    let mut groups: HashMap<String, Vec<(usize, T)>> = HashMap::new();
    for (host_idx, item) in items {
        let key = item.identity_key().into_owned();
        groups.entry(key).or_default().push((host_idx, item));
    }

    let mut result: Vec<T> = Vec::new();
    for group in groups.values_mut() {
        group.sort_by_key(|(idx, _)| *idx);

        let mut hosts: Vec<String> = group
            .iter()
            .map(|(idx, _)| hostnames[*idx].clone())
            .collect();
        hosts.sort();
        hosts.dedup();

        let has_variants = group[0].1.content_variant_key().is_some();

        if has_variants {
            result.extend(merge_with_variants(group, total_hosts, hostnames));
        } else {
            // Find the most-prevalent payload within same-identity items.
            // Items share identity_key but may differ in non-key fields
            // (e.g., same package name+arch but different versions).
            let mut payload_counts: Vec<(String, usize, usize)> = Vec::new();
            for (pos, (_, item)) in group.iter().enumerate() {
                let mut normalized = item.clone();
                *normalized.aggregate_mut() = None;
                normalized.set_include(true);
                let key = serde_json::to_string(&normalized).unwrap_or_default();
                if let Some(entry) = payload_counts.iter_mut().find(|(k, _, _)| k == &key) {
                    entry.1 += 1;
                } else {
                    payload_counts.push((key, 1, pos));
                }
            }
            // Most prevalent wins; tie-break by first-seen (= first by hostname)
            payload_counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
            let winner_idx = payload_counts[0].2;

            let count = hosts.len() as i32;
            let mut representative = group[winner_idx].1.clone();
            *representative.aggregate_mut() = Some(AggregatePrevalence {
                count,
                total: total_hosts as i32,
                hosts,
                ..Default::default()
            });
            representative.set_include(true);
            result.push(representative);
        }
    }

    // Narrowing pass: non-universal items get include=false
    narrow_non_universal(&mut result);

    result.sort_by(|a, b| a.identity_key().cmp(&b.identity_key()));
    result
}

/// Sub-groups variant-capable items by content hash and assigns variant
/// selection roles.
///
/// - Single content variant across all hosts: [`VariantSelection::Only`]
/// - Multiple variants: most-prevalent is [`VariantSelection::Selected`],
///   rest are [`VariantSelection::Alternative`]. Ties are broken by
///   lexicographic content hash for deterministic output.
fn merge_with_variants<T: AggregateMergeable>(
    group: &mut [(usize, T)],
    total_hosts: usize,
    hostnames: &[String],
) -> Vec<T> {
    let mut subgroups: HashMap<String, Vec<(usize, &T)>> = HashMap::new();
    for (idx, item) in group.iter() {
        // Safe to unwrap: caller verified content_variant_key().is_some()
        let hash = item.content_variant_key().unwrap().into_owned();
        subgroups.entry(hash).or_default().push((*idx, item));
    }

    fn unique_hosts<U: AggregateMergeable>(
        subgroup: &[(usize, &U)],
        hostnames: &[String],
    ) -> Vec<String> {
        let mut hosts: Vec<String> = subgroup
            .iter()
            .map(|(idx, _)| hostnames[*idx].clone())
            .collect();
        hosts.sort();
        hosts.dedup();
        hosts
    }

    // Single variant across all hosts — mark as Only
    if subgroups.len() == 1 {
        let (_, subgroup) = subgroups.into_iter().next().unwrap();
        let hosts = unique_hosts(&subgroup, hostnames);
        let mut item = subgroup[0].1.clone();
        *item.aggregate_mut() = Some(AggregatePrevalence {
            count: hosts.len() as i32,
            total: total_hosts as i32,
            hosts,
            ..Default::default()
        });
        item.set_include(true);
        // variant_selection_mut defaults to Only, no change needed
        return vec![item];
    }

    // Multiple variants — compute aggregate prevalence (union of all hosts
    // across all variants), then rank by per-variant prevalence.
    let aggregate_hosts = {
        let mut all: Vec<String> = subgroups
            .values()
            .flat_map(|sg| sg.iter().map(|(idx, _)| hostnames[*idx].clone()))
            .collect();
        all.sort();
        all.dedup();
        all
    };
    let aggregate_count = aggregate_hosts.len() as i32;

    let mut ranked: Vec<(String, Vec<(usize, &T)>)> = subgroups.into_iter().collect();
    ranked.sort_by(|(hash_a, hosts_a), (hash_b, hosts_b)| {
        let count_a = {
            let mut h: Vec<usize> = hosts_a.iter().map(|(i, _)| *i).collect();
            h.sort();
            h.dedup();
            h.len()
        };
        let count_b = {
            let mut h: Vec<usize> = hosts_b.iter().map(|(i, _)| *i).collect();
            h.sort();
            h.dedup();
            h.len()
        };
        count_b.cmp(&count_a).then_with(|| hash_a.cmp(hash_b))
    });

    let mut variant_results = Vec::new();
    for (i, (_hash, subgroup)) in ranked.iter().enumerate() {
        let hosts = unique_hosts(subgroup, hostnames);
        let mut item = subgroup[0].1.clone();
        *item.aggregate_mut() = Some(AggregatePrevalence {
            count: hosts.len() as i32,
            total: total_hosts as i32,
            hosts,
            aggregate_count: Some(aggregate_count),
            aggregate_hosts: Some(aggregate_hosts.clone()),
        });
        item.set_include(true);
        if let Some(vs) = item.variant_selection_mut() {
            *vs = if i == 0 {
                VariantSelection::Selected
            } else {
                VariantSelection::Alternative
            };
        }
        variant_results.push(item);
    }

    // Narrowing pass: non-universal items get include=false
    narrow_non_universal(&mut variant_results);

    variant_results
}

/// Post-merge narrowing: set `include = false` for items that are not
/// universal across all hosts in the aggregate (count < total).
///
/// This is the single place where aggregate narrowing happens — called after
/// both `merge_items` and `merge_with_variants` produce their results.
fn narrow_non_universal<T: AggregateMergeable>(items: &mut [T]) {
    for item in items.iter_mut() {
        let dominated = item
            .aggregate_mut()
            .as_ref()
            .is_some_and(|f| f.count < f.total);
        if dominated {
            item.set_include(false);
        }
    }
}

// ===========================================================================
// Dedup helpers
// ===========================================================================

/// Deduplicate and sort string lists from multiple hosts.
pub fn dedup_strings(lists: Vec<Vec<String>>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for list in lists {
        for item in list {
            if seen.insert(item.clone()) {
                result.push(item);
            }
        }
    }
    result.sort();
    result
}

/// Deduplicate JSON values by serialized equality.
pub fn dedup_json_values(lists: Vec<Vec<serde_json::Value>>) -> Vec<serde_json::Value> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for list in lists {
        for item in list {
            let key = serde_json::to_string(&item).unwrap_or_default();
            if seen.insert(key) {
                result.push(item);
            }
        }
    }
    result
}

/// Collect items from optional sections, pairing each with its host index.
fn collect_items<S, T, F>(sections: &[Option<S>], extractor: F) -> Vec<(usize, T)>
where
    T: Clone,
    F: Fn(&S) -> &Vec<T>,
{
    let mut items = Vec::new();
    for (idx, section) in sections.iter().enumerate() {
        if let Some(s) = section {
            for item in extractor(s) {
                items.push((idx, item.clone()));
            }
        }
    }
    items
}

/// Collect string lists from optional sections.
fn collect_string_lists<S, F>(sections: &[Option<S>], extractor: F) -> Vec<Vec<String>>
where
    F: Fn(&S) -> &Vec<String>,
{
    sections
        .iter()
        .filter_map(|s| s.as_ref().map(|s| extractor(s).clone()))
        .collect()
}

/// Pick an optional scalar value from the first host (sorted by hostname).
fn first_host_option<S, T, F>(
    sections: &[Option<S>],
    hostnames: &[String],
    extractor: F,
) -> Option<T>
where
    T: Clone,
    F: Fn(&S) -> &Option<T>,
{
    let mut pairs: Vec<(&str, &T)> = Vec::new();
    for (idx, section) in sections.iter().enumerate() {
        if let Some(s) = section
            && let Some(val) = extractor(s)
        {
            pairs.push((hostnames.get(idx).map(|s| s.as_str()).unwrap_or(""), val));
        }
    }
    pairs.sort_by_key(|(h, _)| *h);
    pairs.first().map(|(_, v)| (*v).clone())
}

/// Pick the most-prevalent non-empty string value across hosts.
///
/// Counts occurrences of each distinct value. Returns the value with the
/// highest count. Tie-break: first seen (which is first by sorted hostname
/// since sections are pre-sorted). Returns empty string if no section has
/// a non-empty value.
fn most_prevalent_scalar<S, F>(sections: &[Option<S>], extractor: F) -> String
where
    F: Fn(&S) -> &str,
{
    let mut counts: Vec<(String, usize)> = Vec::new();
    for section in sections.iter().flatten() {
        let val = extractor(section);
        if val.is_empty() {
            continue;
        }
        if let Some(entry) = counts.iter_mut().find(|(v, _)| v == val) {
            entry.1 += 1;
        } else {
            counts.push((val.to_string(), 1));
        }
    }
    // Stable: highest count first; ties preserve insertion order (first-seen)
    counts.sort_by(|(_, a), (_, b)| b.cmp(a));
    counts.first().map(|(v, _)| v.clone()).unwrap_or_default()
}

/// Check whether a scalar field has the same value on every host that
/// has the section present.
///
/// Used for tuned universality: a custom profile is only included when
/// every host agrees on the same active profile.
fn is_scalar_universal<S, F>(sections: &[Option<S>], accessor: F, winner: &str) -> bool
where
    F: Fn(&S) -> &str,
{
    sections.iter().flatten().all(|s| accessor(s) == winner)
}

/// Pick the most-prevalent bool value across hosts.
///
/// Counts `true` vs `false` occurrences. Returns the value with the higher
/// count. Tie-break: `false` wins (conservative default).
fn most_prevalent_bool<S, F>(sections: &[Option<S>], extractor: F) -> bool
where
    F: Fn(&S) -> bool,
{
    let mut true_count: usize = 0;
    let mut false_count: usize = 0;
    for section in sections.iter().flatten() {
        if extractor(section) {
            true_count += 1;
        } else {
            false_count += 1;
        }
    }
    true_count > false_count
}

// ===========================================================================
// Section adapters
// ===========================================================================

use crate::types::config::ConfigSection;
use crate::types::containers::ContainerSection;
use crate::types::kernelboot::{KernelBootSection, is_stock_tuned_profile};
use crate::types::network::NetworkSection;
use crate::types::nonrpm::NonRpmSoftwareSection;
use crate::types::rpm::RpmSection;
use crate::types::scheduled::ScheduledTaskSection;
use crate::types::selinux::SelinuxSection;
use crate::types::services::ServiceSection;
use crate::types::storage::StorageSection;
use crate::types::users::UserGroupSection;

/// Pick an optional scalar value from a specific host index.
/// Falls back to `None` if the host has no section or the field is empty.
fn baseline_host_option<S, T, F>(
    sections: &[Option<S>],
    baseline_idx: usize,
    extractor: F,
) -> Option<T>
where
    T: Clone,
    F: Fn(&S) -> &Option<T>,
{
    sections
        .get(baseline_idx)
        .and_then(|s| s.as_ref())
        .and_then(|s| extractor(s).clone())
}

/// Merge RPM sections from multiple hosts.
///
/// `baseline_host_idx` identifies which sorted host's baseline-bearing fields
/// to use (e.g. `baseline_package_names`, `base_image`).
/// When `None`, no baseline is selected and baseline fields use defaults.
///
/// Returns `(merged_section, repo_conflicts)` where `repo_conflicts` maps
/// `name.arch` identity keys to the distinct repos with host counts, only
/// for packages installed from 2+ different repos across the aggregate.
pub fn merge_rpm_sections(
    sections: Vec<Option<RpmSection>>,
    total_hosts: usize,
    hostnames: &[String],
    baseline_host_idx: Option<usize>,
) -> Option<(RpmSection, HashMap<String, Vec<RepoSourceEntry>>)> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let packages_added = merge_items(
        collect_items(&sections, |s| &s.packages_added),
        total_hosts,
        hostnames,
    );
    let base_image_only = merge_items(
        collect_items(&sections, |s| &s.base_image_only),
        total_hosts,
        hostnames,
    );
    let repo_files = merge_items(
        collect_items(&sections, |s| &s.repo_files),
        total_hosts,
        hostnames,
    );
    let gpg_keys = merge_items(
        collect_items(&sections, |s| &s.gpg_keys),
        total_hosts,
        hostnames,
    );
    let module_streams = merge_items(
        collect_items(&sections, |s| &s.module_streams),
        total_hosts,
        hostnames,
    );
    let version_locks = merge_items(
        collect_items(&sections, |s| &s.version_locks),
        total_hosts,
        hostnames,
    );

    // Dedup string lists
    let dnf_history_removed =
        dedup_strings(collect_string_lists(&sections, |s| &s.dnf_history_removed));
    let module_stream_conflicts = dedup_strings(collect_string_lists(&sections, |s| {
        &s.module_stream_conflicts
    }));
    let multiarch_packages =
        dedup_strings(collect_string_lists(&sections, |s| &s.multiarch_packages));
    let duplicate_packages =
        dedup_strings(collect_string_lists(&sections, |s| &s.duplicate_packages));
    let repo_providing_packages = dedup_strings(collect_string_lists(&sections, |s| {
        &s.repo_providing_packages
    }));
    let ostree_removals = dedup_strings(collect_string_lists(&sections, |s| &s.ostree_removals));

    // Dedup version_changes by name.arch
    let version_changes = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for vc in &s.version_changes {
                let key = format!("{}.{}", vc.name, vc.arch);
                if seen.insert(key) {
                    result.push(vc.clone());
                }
            }
        }
        result.sort_by(|a, b| {
            let ka = format!("{}.{}", a.name, a.arch);
            let kb = format!("{}.{}", b.name, b.arch);
            ka.cmp(&kb)
        });
        result
    };

    // Dedup ostree_overrides by name
    let ostree_overrides = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for oo in &s.ostree_overrides {
                if seen.insert(oo.name.clone()) {
                    result.push(oo.clone());
                }
            }
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    };

    // Dedup rpm_va by path
    let rpm_va = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for entry in &s.rpm_va {
                if seen.insert(entry.path.clone()) {
                    result.push(entry.clone());
                }
            }
        }
        result.sort_by(|a, b| a.path.cmp(&b.path));
        result
    };

    // --- Leaf intersection across authoritative hosts ---
    // An authoritative host has leaf_packages: Some(_).
    // Hosts with leaf_packages: None (degraded) are skipped entirely.
    let authoritative_indices: Vec<usize> = sections
        .iter()
        .enumerate()
        .filter(|(_, s)| s.as_ref().and_then(|s| s.leaf_packages.as_ref()).is_some())
        .map(|(i, _)| i)
        .collect();

    let leaf_authority_hosts = Some(authoritative_indices.len() as u32);
    let leaf_total_hosts = Some(total_hosts as u32);

    let (leaf_packages, leaf_union, auto_packages, leaf_dep_tree) =
        if authoritative_indices.is_empty() {
            // All hosts degraded — no leaf truth available.
            (
                None,
                None,
                None,
                serde_json::Value::Object(serde_json::Map::new()),
            )
        } else {
            // Compute intersection AND union of authoritative hosts' leaf sets.
            // Intersection: packages that are leaves on ALL authoritative hosts
            //   → used to filter universal packages (keeps only true leaves).
            // Union: packages that are a leaf on ANY authoritative host
            //   → used to filter partial-prevalence packages (a package present
            //     on 2 of 3 hosts can't be in the intersection because host 3
            //     doesn't have it, but it should survive if it's a leaf where
            //     it exists).
            let leaf_sets: Vec<HashSet<String>> = authoritative_indices
                .iter()
                .map(|&i| {
                    sections[i]
                        .as_ref()
                        .unwrap()
                        .leaf_packages
                        .as_ref()
                        .unwrap()
                        .iter()
                        .cloned()
                        .collect()
                })
                .collect();

            let mut intersection = leaf_sets[0].clone();
            let mut union = leaf_sets[0].clone();
            for set in &leaf_sets[1..] {
                intersection.retain(|pkg| set.contains(pkg));
                union.extend(set.iter().cloned());
            }
            let mut sorted_leaf: Vec<String> = intersection.into_iter().collect();
            sorted_leaf.sort();

            // Aggregate auto_packages: None (not independently meaningful).
            let auto = None;

            // Dep tree from first authoritative host (sorted by hostname),
            // filtered to intersection entries only.
            let leaf_ids: HashSet<&str> = sorted_leaf.iter().map(|s| s.as_str()).collect();
            let dep_tree = {
                let mut auth_pairs: Vec<(usize, &str)> = authoritative_indices
                    .iter()
                    .map(|&i| (i, hostnames.get(i).map(|s| s.as_str()).unwrap_or("")))
                    .collect();
                auth_pairs.sort_by_key(|(_, h)| *h);

                let donor_idx = auth_pairs[0].0;
                let donor_tree = &sections[donor_idx].as_ref().unwrap().leaf_dep_tree;

                if let Some(obj) = donor_tree.as_object() {
                    let filtered: serde_json::Map<String, serde_json::Value> = obj
                        .iter()
                        .filter(|(k, _)| leaf_ids.contains(k.as_str()))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    serde_json::Value::Object(filtered)
                } else {
                    serde_json::Value::Object(serde_json::Map::new())
                }
            };

            (Some(sorted_leaf), Some(union), auto, dep_tree)
        };

    let versionlock_command_output =
        first_host_option(&sections, hostnames, |s| &s.versionlock_command_output);

    // Baseline-bearing fields: source from the winning baseline host (not first-sorted).
    // This ensures RPM section baseline data is consistent with the top-level
    // baseline selection in the orchestrator.
    let (baseline_package_names, baseline_module_streams, base_image, baseline_suppressed) =
        if let Some(idx) = baseline_host_idx {
            (
                baseline_host_option(&sections, idx, |s| &s.baseline_package_names),
                baseline_host_option(&sections, idx, |s| &s.baseline_module_streams),
                baseline_host_option(&sections, idx, |s| &s.base_image),
                baseline_host_option(&sections, idx, |s| &s.baseline_suppressed),
            )
        } else {
            // No baseline selected — use defaults
            (None, None, None, None)
        };
    // file_ownership: dedup by package_name
    let file_ownership = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for entry in &s.file_ownership {
                if seen.insert(entry.package_name.clone()) {
                    result.push(entry.clone());
                }
            }
        }
        result.sort_by(|a, b| a.package_name.cmp(&b.package_name));
        result
    };

    // Filter packages_added to leaf-only when authoritative leaf data exists.
    // This MUST happen before repo-conflict detection so that auto/transitive
    // packages cannot appear in the repo_conflicts map.
    //
    // Two-tier filter:
    //   - Universal packages (count >= total, i.e. present on all hosts):
    //     keep only if in the leaf intersection (leaf on ALL authoritative
    //     hosts). A universal package that is auto on some hosts is not a
    //     user-intent package aggregate-wide.
    //   - Partial packages (count < total, i.e. not on every host): keep
    //     if in the leaf union (leaf on ANY authoritative host). These
    //     can't appear in the intersection because hosts that don't have
    //     them won't list them as leaves. Without the union check they
    //     would be silently deleted from the output.
    let packages_added = if let Some(ref leaf_set) = leaf_packages {
        let leaf_intersection: HashSet<&str> = leaf_set.iter().map(|s| s.as_str()).collect();
        let leaf_any: HashSet<&str> = leaf_union
            .as_ref()
            .map(|u| u.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        packages_added
            .into_iter()
            .filter(|pkg| {
                let id = format!("{}.{}", pkg.name, pkg.arch);
                let is_universal = pkg.aggregate.as_ref().is_some_and(|f| f.count >= f.total);
                if is_universal {
                    leaf_intersection.contains(id.as_str())
                } else {
                    leaf_any.contains(id.as_str())
                }
            })
            .collect()
    } else {
        packages_added
    };

    // Force include=true for leaf intersection survivors.
    // Packages in the intersection are leaves on ALL authoritative hosts —
    // they should be included in the Containerfile regardless of what
    // narrow_non_universal() did (which uses total host count including
    // degraded hosts, so a package can be in the intersection yet have
    // count < total).
    //
    // Packages that survived via the leaf union but are NOT in the
    // intersection keep their include flag as-is (narrow_non_universal set
    // include=false for partials, and that's the correct Containerfile
    // decision).
    let mut packages_added = packages_added;
    if let Some(ref leaf_set) = leaf_packages {
        let leaf_intersection: HashSet<&str> = leaf_set.iter().map(|s| s.as_str()).collect();
        for pkg in &mut packages_added {
            let id = format!("{}.{}", pkg.name, pkg.arch);
            if leaf_intersection.contains(id.as_str()) {
                pkg.include = true;
            }
        }
    }

    // Detect repo-source conflicts: packages installed from different repos
    // across the aggregate. Only tracks conflicts when repos span different tiers
    // (e.g., epel vs baseos). Same-tier differences (e.g., anaconda vs baseos)
    // are not meaningful conflicts.
    let repo_conflicts = {
        use crate::types::repo::repo_tier;

        let mut conflicts: HashMap<String, Vec<RepoSourceEntry>> = HashMap::new();
        for pkg in &packages_added {
            let key = format!("{}.{}", pkg.name, pkg.arch);
            let mut repo_counts: HashMap<String, usize> = HashMap::new();
            for section in sections.iter().flatten() {
                for host_pkg in &section.packages_added {
                    if host_pkg.name == pkg.name
                        && host_pkg.arch == pkg.arch
                        && !host_pkg.source_repo.is_empty()
                    {
                        *repo_counts
                            .entry(host_pkg.source_repo.to_lowercase())
                            .or_insert(0) += 1;
                    }
                }
            }
            if repo_counts.len() >= 2 {
                // Check if all repos map to the same tier -- if so, skip.
                let tiers: HashSet<_> = repo_counts
                    .keys()
                    .map(|r| std::mem::discriminant(&repo_tier(r)))
                    .collect();
                if tiers.len() < 2 {
                    continue;
                }
                let mut entries: Vec<RepoSourceEntry> = repo_counts
                    .into_iter()
                    .map(|(repo, host_count)| RepoSourceEntry { repo, host_count })
                    .collect();
                entries.sort_by(|a, b| {
                    b.host_count
                        .cmp(&a.host_count)
                        .then_with(|| a.repo.cmp(&b.repo))
                });
                conflicts.insert(key, entries);
            }
        }
        conflicts
    };

    // Reconcile source_repo with repo-majority winner: for any package
    // that appears in the conflict map, overwrite its source_repo with the
    // winning repo (highest host_count, alphabetical tie-break — same sort
    // already applied above). merge_items picks the representative by
    // full-payload prevalence, which can disagree with repo majority when
    // the majority repo is split across multiple payload variants.
    let packages_added = {
        let mut pkgs = packages_added;
        for pkg in &mut pkgs {
            let key = format!("{}.{}", pkg.name, pkg.arch);
            if let Some(entries) = repo_conflicts.get(&key)
                && let Some(winner) = entries.first()
            {
                pkg.source_repo = winner.repo.clone();
            }
        }
        pkgs
    };

    Some((
        RpmSection {
            packages_added,
            base_image_only,
            rpm_va,
            repo_files,
            gpg_keys,
            dnf_history_removed,
            version_changes,
            leaf_packages,
            auto_packages,
            leaf_dep_tree,
            module_streams,
            version_locks,
            module_stream_conflicts,
            baseline_module_streams,
            versionlock_command_output,
            multiarch_packages,
            duplicate_packages,
            repo_providing_packages,
            ostree_overrides,
            ostree_removals,
            base_image,
            baseline_package_names,
            leaf_authority_hosts,
            leaf_total_hosts,
            baseline_suppressed,
            file_ownership,
            installed_groups: None,
        },
        repo_conflicts,
    ))
}

/// Merge config sections from multiple hosts.
pub fn merge_config_sections(
    sections: Vec<Option<ConfigSection>>,
    total_hosts: usize,
    hostnames: &[String],
) -> Option<ConfigSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let files = merge_items(
        collect_items(&sections, |s| &s.files),
        total_hosts,
        hostnames,
    );

    Some(ConfigSection { files })
}

/// Merge service sections from multiple hosts.
pub fn merge_service_sections(
    sections: Vec<Option<ServiceSection>>,
    total_hosts: usize,
    hostnames: &[String],
) -> Option<ServiceSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let state_changes = merge_items(
        collect_items(&sections, |s| &s.state_changes),
        total_hosts,
        hostnames,
    );
    let drop_ins = merge_items(
        collect_items(&sections, |s| &s.drop_ins),
        total_hosts,
        hostnames,
    );
    let enabled_units = dedup_strings(collect_string_lists(&sections, |s| &s.enabled_units));
    let disabled_units = dedup_strings(collect_string_lists(&sections, |s| &s.disabled_units));
    let preset_matched_units =
        dedup_strings(collect_string_lists(&sections, |s| &s.preset_matched_units));

    Some(ServiceSection {
        state_changes,
        enabled_units,
        disabled_units,
        drop_ins,
        preset_matched_units,
    })
}

/// Merge container sections from multiple hosts.
pub fn merge_container_sections(
    sections: Vec<Option<ContainerSection>>,
    total_hosts: usize,
    hostnames: &[String],
) -> Option<ContainerSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let quadlet_units = merge_items(
        collect_items(&sections, |s| &s.quadlet_units),
        total_hosts,
        hostnames,
    );
    let compose_files = merge_items(
        collect_items(&sections, |s| &s.compose_files),
        total_hosts,
        hostnames,
    );

    // Skip running_containers — runtime state, not config
    let running_containers = Vec::new();

    let flatpak_apps = merge_items(
        collect_items(&sections, |s| &s.flatpak_apps),
        total_hosts,
        hostnames,
    );

    Some(ContainerSection {
        quadlet_units,
        compose_files,
        running_containers,
        flatpak_apps,
    })
}

/// Merge network sections from multiple hosts.
pub fn merge_network_sections(
    sections: Vec<Option<NetworkSection>>,
    total_hosts: usize,
    hostnames: &[String],
) -> Option<NetworkSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let connections = merge_items(
        collect_items(&sections, |s| &s.connections),
        total_hosts,
        hostnames,
    );
    let firewall_zones = merge_items(
        collect_items(&sections, |s| &s.firewall_zones),
        total_hosts,
        hostnames,
    );

    // Dedup firewall_direct_rules by identity (all fields)
    let firewall_direct_rules = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for rule in &s.firewall_direct_rules {
                let key = format!(
                    "{}:{}:{}:{}:{}",
                    rule.ipv, rule.table, rule.chain, rule.priority, rule.args
                );
                if seen.insert(key) {
                    result.push(rule.clone());
                }
            }
        }
        result
    };

    // Dedup static_routes by path
    let static_routes = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for route in &s.static_routes {
                if seen.insert(route.path.clone()) {
                    result.push(route.clone());
                }
            }
        }
        result.sort_by(|a, b| a.path.cmp(&b.path));
        result
    };

    let ip_routes = dedup_strings(collect_string_lists(&sections, |s| &s.ip_routes));
    let ip_rules = dedup_strings(collect_string_lists(&sections, |s| &s.ip_rules));
    let hosts_additions = dedup_strings(collect_string_lists(&sections, |s| &s.hosts_additions));

    // Dedup proxy by source
    let proxy = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for entry in &s.proxy {
                if seen.insert(entry.source.clone()) {
                    result.push(entry.clone());
                }
            }
        }
        result.sort_by(|a, b| a.source.cmp(&b.source));
        result
    };

    // Most-prevalent value for resolv_provenance
    let resolv_provenance = most_prevalent_scalar(&sections, |s| &s.resolv_provenance);

    Some(NetworkSection {
        connections,
        firewall_zones,
        firewall_direct_rules,
        static_routes,
        ip_routes,
        ip_rules,
        resolv_provenance,
        hosts_additions,
        proxy,
    })
}

/// Merge storage sections from multiple hosts.
pub fn merge_storage_sections(
    sections: Vec<Option<StorageSection>>,
    total_hosts: usize,
    hostnames: &[String],
) -> Option<StorageSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let fstab_entries = merge_items(
        collect_items(&sections, |s| &s.fstab_entries),
        total_hosts,
        hostnames,
    );

    // Dedup mount_points by target
    let mount_points = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for mp in &s.mount_points {
                if seen.insert(mp.target.clone()) {
                    result.push(mp.clone());
                }
            }
        }
        result.sort_by(|a, b| a.target.cmp(&b.target));
        result
    };

    // Dedup lvm_info by lv_name + vg_name
    let lvm_info = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for vol in &s.lvm_info {
                let key = format!("{}/{}", vol.vg_name, vol.lv_name);
                if seen.insert(key) {
                    result.push(vol.clone());
                }
            }
        }
        result.sort_by(|a, b| {
            let ka = format!("{}/{}", a.vg_name, a.lv_name);
            let kb = format!("{}/{}", b.vg_name, b.lv_name);
            ka.cmp(&kb)
        });
        result
    };

    // Dedup var_directories by path
    let var_directories = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for dir in &s.var_directories {
                if seen.insert(dir.path.clone()) {
                    result.push(dir.clone());
                }
            }
        }
        result.sort_by(|a, b| a.path.cmp(&b.path));
        result
    };

    // Dedup credential_refs by mount_point + credential_path
    let credential_refs = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for cr in &s.credential_refs {
                let key = format!("{}:{}", cr.mount_point, cr.credential_path);
                if seen.insert(key) {
                    result.push(cr.clone());
                }
            }
        }
        result.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
        result
    };

    Some(StorageSection {
        fstab_entries,
        mount_points,
        lvm_info,
        var_directories,
        credential_refs,
    })
}

/// Merge scheduled task sections from multiple hosts.
pub fn merge_scheduled_sections(
    sections: Vec<Option<ScheduledTaskSection>>,
    total_hosts: usize,
    hostnames: &[String],
) -> Option<ScheduledTaskSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let cron_jobs = merge_items(
        collect_items(&sections, |s| &s.cron_jobs),
        total_hosts,
        hostnames,
    );
    let systemd_timers = merge_items(
        collect_items(&sections, |s| &s.systemd_timers),
        total_hosts,
        hostnames,
    );
    let at_jobs = merge_items(
        collect_items(&sections, |s| &s.at_jobs),
        total_hosts,
        hostnames,
    );
    let generated_timer_units = merge_items(
        collect_items(&sections, |s| &s.generated_timer_units),
        total_hosts,
        hostnames,
    );

    Some(ScheduledTaskSection {
        cron_jobs,
        systemd_timers,
        at_jobs,
        generated_timer_units,
    })
}

/// Merge SELinux sections from multiple hosts.
pub fn merge_selinux_sections(
    sections: Vec<Option<SelinuxSection>>,
    total_hosts: usize,
    hostnames: &[String],
) -> Option<SelinuxSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let port_labels = merge_items(
        collect_items(&sections, |s| &s.port_labels),
        total_hosts,
        hostnames,
    );

    let custom_modules = dedup_strings(collect_string_lists(&sections, |s| &s.custom_modules));
    let fcontext_rules = dedup_strings(collect_string_lists(&sections, |s| &s.fcontext_rules));

    // Dedup boolean_overrides by JSON equality
    let boolean_overrides = dedup_json_values(
        sections
            .iter()
            .filter_map(|s| s.as_ref().map(|s| s.boolean_overrides.clone()))
            .collect(),
    );

    // Dedup audit_rules (CarryForwardFile) by path
    let audit_rules = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for rule in &s.audit_rules {
                if seen.insert(rule.path.clone()) {
                    result.push(rule.clone());
                }
            }
        }
        result.sort_by(|a, b| a.path.cmp(&b.path));
        result
    };

    // Dedup pam_configs (CarryForwardFile) by path
    let pam_configs = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for cfg in &s.pam_configs {
                if seen.insert(cfg.path.clone()) {
                    result.push(cfg.clone());
                }
            }
        }
        result.sort_by(|a, b| a.path.cmp(&b.path));
        result
    };

    // Most-prevalent scalar fields
    let mode = most_prevalent_scalar(&sections, |s| &s.mode);
    let fips_mode = most_prevalent_bool(&sections, |s| s.fips_mode);

    Some(SelinuxSection {
        mode,
        custom_modules,
        boolean_overrides,
        fcontext_rules,
        audit_rules,
        fips_mode,
        pam_configs,
        port_labels,
    })
}

/// Merge kernel/boot sections from multiple hosts.
pub fn merge_kernelboot_sections(
    sections: Vec<Option<KernelBootSection>>,
    total_hosts: usize,
    hostnames: &[String],
) -> Option<KernelBootSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let sysctl_overrides = merge_items(
        collect_items(&sections, |s| &s.sysctl_overrides),
        total_hosts,
        hostnames,
    );
    let loaded_modules = merge_items(
        collect_items(&sections, |s| &s.loaded_modules),
        total_hosts,
        hostnames,
    );
    let non_default_modules = merge_items(
        collect_items(&sections, |s| &s.non_default_modules),
        total_hosts,
        hostnames,
    );

    // Dedup ConfigSnippets by path
    let modules_load_d = dedup_config_snippets(&sections, |s| &s.modules_load_d);
    let modprobe_d = dedup_config_snippets(&sections, |s| &s.modprobe_d);
    let dracut_conf = dedup_config_snippets(&sections, |s| &s.dracut_conf);
    let tuned_custom_profiles = dedup_config_snippets(&sections, |s| &s.tuned_custom_profiles);

    // Dedup alternatives by name
    let alternatives = {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for s in sections.iter().flatten() {
            for alt in &s.alternatives {
                if seen.insert(alt.name.clone()) {
                    result.push(alt.clone());
                }
            }
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    };

    // Most-prevalent scalars
    let cmdline = most_prevalent_scalar(&sections, |s| &s.cmdline);
    let grub_defaults = most_prevalent_scalar(&sections, |s| &s.grub_defaults);
    let tuned_active = most_prevalent_scalar(&sections, |s| &s.tuned_active);
    // locale/timezone: pass through from first host sorted by hostname (per spec)
    let locale = first_host_option(&sections, hostnames, |s| &s.locale);
    let timezone = first_host_option(&sections, hostnames, |s| &s.timezone);

    // Tuned include is a composition of two rules:
    // 1. Classifier: stock profiles (virtual-guest, throughput-performance,
    //    etc.) are excluded — they are auto-selected by tuned's recommendation
    //    engine and including them would override that.
    // 2. Universality: non-stock profiles are included only if the winner
    //    profile is universal across all hosts. A custom profile on 2/3 hosts
    //    is not aggregate consensus, so it should not be included.
    let tuned_include = !tuned_active.is_empty()
        && !is_stock_tuned_profile(&tuned_active)
        && is_scalar_universal(&sections, |s| &s.tuned_active, &tuned_active);

    Some(KernelBootSection {
        cmdline,
        grub_defaults,
        sysctl_overrides,
        modules_load_d,
        modprobe_d,
        dracut_conf,
        loaded_modules,
        non_default_modules,
        tuned_include,
        tuned_active,
        tuned_custom_profiles,
        locale,
        timezone,
        alternatives,
    })
}

/// Helper: dedup ConfigSnippet lists by path.
fn dedup_config_snippets<S, F>(
    sections: &[Option<S>],
    extractor: F,
) -> Vec<crate::types::kernelboot::ConfigSnippet>
where
    F: Fn(&S) -> &Vec<crate::types::kernelboot::ConfigSnippet>,
{
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for s in sections.iter().flatten() {
        for snippet in extractor(s) {
            if seen.insert(snippet.path.clone()) {
                result.push(snippet.clone());
            }
        }
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    result
}

/// Merge non-RPM software sections from multiple hosts.
pub fn merge_nonrpm_sections(
    sections: Vec<Option<NonRpmSoftwareSection>>,
    total_hosts: usize,
    hostnames: &[String],
) -> Option<NonRpmSoftwareSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let items = merge_items(
        collect_items(&sections, |s| &s.items),
        total_hosts,
        hostnames,
    );

    // env_files are ConfigFileEntry — merge with variants
    let env_files = merge_items(
        collect_items(&sections, |s| &s.env_files),
        total_hosts,
        hostnames,
    );

    Some(NonRpmSoftwareSection { items, env_files })
}

/// Merge unmanaged file sections from multiple hosts.
///
/// Uses Plan 2's UnmanagedFileSection contract: `items` (not `files`),
/// with `total_size` and `total_count` recomputed from merged items.
pub fn merge_unmanaged_file_sections(
    sections: Vec<Option<UnmanagedFileSection>>,
    total_hosts: usize,
    hostnames: &[String],
) -> Option<UnmanagedFileSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let items = merge_items(
        collect_items(&sections, |s| &s.items),
        total_hosts,
        hostnames,
    );

    // Recompute totals from merged items.
    let total_size: u64 = items.iter().map(|f| f.size).sum();
    let total_count = items.len();

    Some(UnmanagedFileSection {
        items,
        total_size,
        total_count,
    })
}

/// Merge users/groups sections from multiple hosts.
///
/// Users and groups are `Vec<serde_json::Value>` — deduplicated by the
/// `"name"` field extracted from each JSON object. For users, group
/// membership lists are merged (union). For groups, member lists are
/// merged (union).
pub fn merge_usersgroups_sections(
    sections: Vec<Option<UserGroupSection>>,
    _total_hosts: usize,
    _hostnames: &[String],
) -> Option<UserGroupSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let users = merge_json_by_name(
        sections
            .iter()
            .filter_map(|s| s.as_ref().map(|s| &s.users))
            .collect(),
        &["groups", "secondary_groups"],
    );
    let groups = merge_json_by_name(
        sections
            .iter()
            .filter_map(|s| s.as_ref().map(|s| &s.groups))
            .collect(),
        &["members"],
    );

    let sudoers_rules = dedup_strings(collect_string_lists(&sections, |s| &s.sudoers_rules));
    let passwd_entries = dedup_strings(collect_string_lists(&sections, |s| &s.passwd_entries));
    let shadow_entries = dedup_strings(collect_string_lists(&sections, |s| &s.shadow_entries));
    let group_entries = dedup_strings(collect_string_lists(&sections, |s| &s.group_entries));
    let gshadow_entries = dedup_strings(collect_string_lists(&sections, |s| &s.gshadow_entries));
    let subuid_entries = dedup_strings(collect_string_lists(&sections, |s| &s.subuid_entries));
    let subgid_entries = dedup_strings(collect_string_lists(&sections, |s| &s.subgid_entries));

    // Dedup ssh_authorized_keys_refs by JSON equality
    let ssh_authorized_keys_refs = dedup_json_values(
        sections
            .iter()
            .filter_map(|s| s.as_ref().map(|s| s.ssh_authorized_keys_refs.clone()))
            .collect(),
    );

    Some(UserGroupSection {
        users,
        groups,
        sudoers_rules,
        ssh_authorized_keys_refs,
        passwd_entries,
        shadow_entries,
        group_entries,
        gshadow_entries,
        subuid_entries,
        subgid_entries,
    })
}

/// Merge JSON objects by their `"name"` field, union-merging array fields.
fn merge_json_by_name(
    all_lists: Vec<&Vec<serde_json::Value>>,
    union_array_fields: &[&str],
) -> Vec<serde_json::Value> {
    let mut by_name: HashMap<String, serde_json::Value> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for list in all_lists {
        for item in list {
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if let Some(existing) = by_name.get_mut(&name) {
                // Union array fields
                for field in union_array_fields {
                    if let (Some(existing_arr), Some(new_arr)) = (
                        existing.get(*field).and_then(|v| v.as_array()).cloned(),
                        item.get(*field).and_then(|v| v.as_array()),
                    ) {
                        let mut merged = existing_arr;
                        for val in new_arr {
                            if !merged.contains(val) {
                                merged.push(val.clone());
                            }
                        }
                        merged.sort_by(|a, b| {
                            let sa = serde_json::to_string(a).unwrap_or_default();
                            let sb = serde_json::to_string(b).unwrap_or_default();
                            sa.cmp(&sb)
                        });
                        if let Some(obj) = existing.as_object_mut() {
                            obj.insert(field.to_string(), serde_json::Value::Array(merged));
                        }
                    }
                }
            } else {
                order.push(name.clone());
                by_name.insert(name, item.clone());
            }
        }
    }

    order.sort();
    order
        .iter()
        .filter_map(|name| by_name.remove(name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_merge_tracks_repo_conflict() {
        use crate::types::rpm::{PackageEntry, PackageState, RpmSection};

        let host_a_rpm = RpmSection {
            packages_added: vec![PackageEntry {
                name: "nginx".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                source_repo: "epel".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let host_b_rpm = RpmSection {
            packages_added: vec![PackageEntry {
                name: "nginx".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                source_repo: "appstream".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let host_c_rpm = RpmSection {
            packages_added: vec![PackageEntry {
                name: "nginx".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                source_repo: "epel".into(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let hostnames = vec!["host-a".into(), "host-b".into(), "host-c".into()];

        let (merged, repo_conflicts) = merge_rpm_sections(
            vec![Some(host_a_rpm), Some(host_b_rpm), Some(host_c_rpm)],
            3,
            &hostnames,
            None,
        )
        .expect("merge should succeed");

        let nginx = merged
            .packages_added
            .iter()
            .find(|p| p.name == "nginx")
            .expect("nginx should be in merged output");
        assert_eq!(nginx.source_repo, "epel"); // majority wins

        assert!(repo_conflicts.contains_key("nginx.x86_64"));
        let conflict = &repo_conflicts["nginx.x86_64"];
        assert_eq!(conflict.len(), 2);
        assert_eq!(conflict[0].repo, "epel");
        assert_eq!(conflict[0].host_count, 2);
        assert_eq!(conflict[1].repo, "appstream");
        assert_eq!(conflict[1].host_count, 1);
    }

    #[test]
    fn test_merge_no_conflict_single_repo() {
        use crate::types::rpm::{PackageEntry, PackageState, RpmSection};

        let sections = vec![
            Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    source_repo: "baseos".into(),
                    state: PackageState::Added,
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    source_repo: "baseos".into(),
                    state: PackageState::Added,
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
        ];

        let hostnames = vec!["host-a".into(), "host-b".into()];
        let (merged, conflicts) =
            merge_rpm_sections(sections, 2, &hostnames, None).expect("merge should succeed");

        assert_eq!(merged.packages_added[0].source_repo, "baseos");
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_merge_repo_conflict_tie() {
        use crate::types::rpm::{PackageEntry, PackageState, RpmSection};

        let sections = vec![
            Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "nginx".into(),
                    arch: "x86_64".into(),
                    source_repo: "epel".into(),
                    state: PackageState::Added,
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "nginx".into(),
                    arch: "x86_64".into(),
                    source_repo: "appstream".into(),
                    state: PackageState::Added,
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
        ];

        let hostnames = vec!["host-a".into(), "host-b".into()];
        let (merged, conflicts) =
            merge_rpm_sections(sections, 2, &hostnames, None).expect("merge should succeed");

        let nginx = merged
            .packages_added
            .iter()
            .find(|p| p.name == "nginx")
            .expect("nginx should be in merged output");
        // At equal host_count, alphabetical tie-break makes appstream the
        // winner — reconciliation overwrites source_repo accordingly.
        assert_eq!(nginx.source_repo, "appstream");

        let conflict = &conflicts["nginx.x86_64"];
        assert_eq!(conflict.len(), 2);
        assert_eq!(conflict[0].repo, "appstream"); // alpha first at equal count
        assert_eq!(conflict[0].host_count, 1);
        assert_eq!(conflict[1].repo, "epel");
        assert_eq!(conflict[1].host_count, 1);
    }

    #[test]
    fn test_same_tier_repos_not_counted_as_conflict() {
        use crate::types::rpm::{PackageEntry, PackageState, RpmSection};

        // anaconda and baseos are both Distro tier -- no real conflict.
        let sections = vec![
            Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    source_repo: "anaconda".into(),
                    state: PackageState::Added,
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    source_repo: "baseos".into(),
                    state: PackageState::Added,
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    source_repo: "baseos".into(),
                    state: PackageState::Added,
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
        ];

        let hostnames = vec!["host-a".into(), "host-b".into(), "host-c".into()];
        let (_, conflicts) =
            merge_rpm_sections(sections, 3, &hostnames, None).expect("merge should succeed");

        assert!(
            !conflicts.contains_key("bash.x86_64"),
            "same-tier repos (anaconda vs baseos) should not be counted as conflict"
        );
    }

    #[test]
    fn test_cross_tier_repos_counted_as_conflict() {
        use crate::types::rpm::{PackageEntry, PackageState, RpmSection};

        // baseos (Distro) vs epel (ThirdParty) is a real conflict.
        let sections = vec![
            Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "nginx".into(),
                    arch: "x86_64".into(),
                    source_repo: "baseos".into(),
                    state: PackageState::Added,
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "nginx".into(),
                    arch: "x86_64".into(),
                    source_repo: "epel".into(),
                    state: PackageState::Added,
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
        ];

        let hostnames = vec!["host-a".into(), "host-b".into()];
        let (_, conflicts) =
            merge_rpm_sections(sections, 2, &hostnames, None).expect("merge should succeed");

        assert!(
            conflicts.contains_key("nginx.x86_64"),
            "cross-tier repos (baseos vs epel) should be counted as conflict"
        );
    }

    #[test]
    fn test_config_variant_aggregate_prevalence() {
        use crate::types::config::{ConfigFileEntry, ConfigSection};

        // 3 hosts: web-01 has no /etc/chrony.conf, web-02 has content A,
        // web-03 has content B. Per-variant prevalence is 1/3 each.
        // Aggregate prevalence should be 2/3.
        let host_a_cfg = ConfigSection {
            files: vec![], // web-01 has no chrony.conf
        };
        let host_b_cfg = ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/chrony.conf".into(),
                content: "server ntp1.example.com".into(),
                include: true,
                ..Default::default()
            }],
        };
        let host_c_cfg = ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/chrony.conf".into(),
                content: "server ntp2.example.com".into(),
                include: true,
                ..Default::default()
            }],
        };

        let hostnames = vec!["web-01".into(), "web-02".into(), "web-03".into()];
        let merged = merge_config_sections(
            vec![Some(host_a_cfg), Some(host_b_cfg), Some(host_c_cfg)],
            3,
            &hostnames,
        )
        .expect("merge should succeed");

        let chrony_entries: Vec<_> = merged
            .files
            .iter()
            .filter(|f| f.path == "/etc/chrony.conf")
            .collect();
        assert_eq!(chrony_entries.len(), 2, "should have 2 content variants");

        for entry in &chrony_entries {
            let agg = entry
                .aggregate
                .as_ref()
                .expect("should have aggregate prevalence");
            // Per-variant: each has 1 host
            assert_eq!(agg.count, 1, "per-variant count should be 1");
            assert_eq!(agg.total, 3, "total should be 3");
            // Aggregate: union of both variants = 2 hosts
            assert_eq!(agg.aggregate_count, Some(2), "aggregate count should be 2");
            let agg_hosts = agg
                .aggregate_hosts
                .as_ref()
                .expect("should have aggregate hosts");
            assert_eq!(agg_hosts.len(), 2);
            assert!(agg_hosts.contains(&"web-02".to_string()));
            assert!(agg_hosts.contains(&"web-03".to_string()));
        }
    }

    /// Regression: when the majority repo is split across multiple payload
    /// variants (different versions), merge_items picks the representative
    /// by full-payload prevalence which may disagree with repo majority.
    /// The reconciliation step must overwrite source_repo with the
    /// repo-majority winner.
    #[test]
    fn test_merge_source_repo_follows_majority_not_payload() {
        use crate::types::rpm::{PackageEntry, PackageState, RpmSection};

        // host-a: nginx from appstream, version 1.0
        let host_a_rpm = RpmSection {
            packages_added: vec![PackageEntry {
                name: "nginx".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                source_repo: "appstream".into(),
                version: "1.0".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        // host-b: nginx from epel, version 1.1
        let host_b_rpm = RpmSection {
            packages_added: vec![PackageEntry {
                name: "nginx".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                source_repo: "epel".into(),
                version: "1.1".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        // host-c: nginx from epel, version 1.2
        let host_c_rpm = RpmSection {
            packages_added: vec![PackageEntry {
                name: "nginx".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                source_repo: "epel".into(),
                version: "1.2".into(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let hostnames = vec!["host-a".into(), "host-b".into(), "host-c".into()];

        let (merged, repo_conflicts) = merge_rpm_sections(
            vec![Some(host_a_rpm), Some(host_b_rpm), Some(host_c_rpm)],
            3,
            &hostnames,
            None,
        )
        .expect("merge should succeed");

        let nginx = merged
            .packages_added
            .iter()
            .find(|p| p.name == "nginx")
            .expect("nginx should be in merged output");

        // Repo majority is epel (2/3), but payload prevalence is 1/1/1
        // so merge_items could pick any payload. The reconciliation step
        // must overwrite source_repo with the repo-majority winner.
        assert_eq!(
            nginx.source_repo, "epel",
            "source_repo must follow repo majority (epel), not payload prevalence"
        );

        // Verify conflict map is correct
        assert!(repo_conflicts.contains_key("nginx.x86_64"));
        let conflict = &repo_conflicts["nginx.x86_64"];
        assert_eq!(conflict.len(), 2);
        assert_eq!(conflict[0].repo, "epel");
        assert_eq!(conflict[0].host_count, 2);
        assert_eq!(conflict[1].repo, "appstream");
        assert_eq!(conflict[1].host_count, 1);
    }

    #[test]
    fn flatpak_different_remotes_not_collapsed() {
        use crate::types::containers::{ContainerSection, FlatpakApp};

        // Same app_id but different remotes — must produce two distinct items.
        // Previously collapsed by app_id-only dedup.
        let host_a = ContainerSection {
            flatpak_apps: vec![FlatpakApp {
                app_id: "org.mozilla.Firefox".into(),
                remote: "flathub".into(),
                branch: "stable".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let host_b = ContainerSection {
            flatpak_apps: vec![FlatpakApp {
                app_id: "org.mozilla.Firefox".into(),
                remote: "fedora".into(),
                branch: "stable".into(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let merged = merge_container_sections(
            vec![Some(host_a), Some(host_b)],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .expect("merge should succeed");

        assert_eq!(
            merged.flatpak_apps.len(),
            2,
            "same app_id with different remotes must not be collapsed"
        );
        let remotes: Vec<&str> = merged
            .flatpak_apps
            .iter()
            .map(|a| a.remote.as_str())
            .collect();
        assert!(remotes.contains(&"fedora"));
        assert!(remotes.contains(&"flathub"));

        // Each distinct identity appears on exactly 1 of 2 hosts
        for app in &merged.flatpak_apps {
            let agg = app
                .aggregate
                .as_ref()
                .expect("flatpak should have aggregate data");
            assert_eq!(agg.count, 1);
            assert_eq!(agg.total, 2);
        }
    }

    #[test]
    fn flatpak_same_identity_deduped() {
        use crate::types::containers::{ContainerSection, FlatpakApp};

        // Same (app_id, remote, branch) across hosts with different remote_url.
        // remote_url is render metadata — these should collapse to one entry.
        let host_a = ContainerSection {
            flatpak_apps: vec![FlatpakApp {
                app_id: "org.mozilla.Firefox".into(),
                remote: "flathub".into(),
                branch: "stable".into(),
                remote_url: "https://dl.flathub.org/repo/".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let host_b = ContainerSection {
            flatpak_apps: vec![FlatpakApp {
                app_id: "org.mozilla.Firefox".into(),
                remote: "flathub".into(),
                branch: "stable".into(),
                remote_url: "https://mirror.example.com/flathub/".into(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let merged = merge_container_sections(
            vec![Some(host_a), Some(host_b)],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .expect("merge should succeed");

        assert_eq!(
            merged.flatpak_apps.len(),
            1,
            "same (app_id, remote, branch) must collapse despite different remote_url"
        );

        let agg = merged.flatpak_apps[0]
            .aggregate
            .as_ref()
            .expect("merged flatpak should have aggregate data");
        assert_eq!(agg.count, 2, "present on both hosts");
        assert_eq!(agg.total, 2);
        assert!(agg.hosts.contains(&"host-a".to_string()));
        assert!(agg.hosts.contains(&"host-b".to_string()));
    }

    #[test]
    fn flatpak_different_branches_not_collapsed() {
        use crate::types::containers::{ContainerSection, FlatpakApp};

        // Same app_id and remote but different branch — two distinct items.
        let host_a = ContainerSection {
            flatpak_apps: vec![FlatpakApp {
                app_id: "org.mozilla.Firefox".into(),
                remote: "flathub".into(),
                branch: "stable".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let host_b = ContainerSection {
            flatpak_apps: vec![FlatpakApp {
                app_id: "org.mozilla.Firefox".into(),
                remote: "flathub".into(),
                branch: "beta".into(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let merged = merge_container_sections(
            vec![Some(host_a), Some(host_b)],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .expect("merge should succeed");

        assert_eq!(
            merged.flatpak_apps.len(),
            2,
            "same app_id with different branches must not be collapsed"
        );

        // Each branch variant appears on exactly 1 of 2 hosts
        for app in &merged.flatpak_apps {
            let agg = app
                .aggregate
                .as_ref()
                .expect("flatpak should have aggregate data");
            assert_eq!(agg.count, 1);
            assert_eq!(agg.total, 2);
        }
    }

    // -----------------------------------------------------------------------
    // Aggregate narrowing tests (Task 11)
    // -----------------------------------------------------------------------

    #[test]
    fn aggregate_merge_non_universal_quadlet_excluded() {
        use crate::types::containers::{ContainerSection, QuadletUnit};

        // Quadlet appears on 2 of 3 hosts
        let host_a = ContainerSection {
            quadlet_units: vec![QuadletUnit {
                name: "myapp.container".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let host_b = ContainerSection {
            quadlet_units: vec![QuadletUnit {
                name: "myapp.container".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let host_c = ContainerSection {
            ..Default::default()
        };

        let merged = merge_container_sections(
            vec![Some(host_a), Some(host_b), Some(host_c)],
            3,
            &["host-a".into(), "host-b".into(), "host-c".into()],
        )
        .expect("merge should succeed");

        assert_eq!(merged.quadlet_units.len(), 1);
        let q = &merged.quadlet_units[0];
        let agg = q.aggregate.as_ref().expect("should have aggregate data");
        assert_eq!(agg.count, 2);
        assert_eq!(agg.total, 3);
        assert!(!q.include, "non-universal quadlet must have include=false");
    }

    #[test]
    fn aggregate_merge_universal_quadlet_included() {
        use crate::types::containers::{ContainerSection, QuadletUnit};

        // Quadlet appears on all 3 hosts
        let make_host = || ContainerSection {
            quadlet_units: vec![QuadletUnit {
                name: "myapp.container".into(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let merged = merge_container_sections(
            vec![Some(make_host()), Some(make_host()), Some(make_host())],
            3,
            &["host-a".into(), "host-b".into(), "host-c".into()],
        )
        .expect("merge should succeed");

        assert_eq!(merged.quadlet_units.len(), 1);
        let q = &merged.quadlet_units[0];
        let agg = q.aggregate.as_ref().expect("should have aggregate data");
        assert_eq!(agg.count, 3);
        assert_eq!(agg.total, 3);
        assert!(q.include, "universal quadlet must have include=true");
    }

    #[test]
    fn aggregate_merge_tuned_universal_stock_excluded() {
        use crate::types::kernelboot::KernelBootSection;

        // All 3 hosts have virtual-guest (stock profile)
        let make_host = || KernelBootSection {
            tuned_active: "virtual-guest".into(),
            ..Default::default()
        };

        let merged = merge_kernelboot_sections(
            vec![Some(make_host()), Some(make_host()), Some(make_host())],
            3,
            &["host-a".into(), "host-b".into(), "host-c".into()],
        )
        .expect("merge should succeed");

        assert!(
            !merged.tuned_include,
            "stock tuned profile must be excluded even when universal"
        );
    }

    #[test]
    fn aggregate_merge_tuned_universal_custom_included() {
        use crate::types::kernelboot::KernelBootSection;

        // All 3 hosts have my-custom-profile
        let make_host = || KernelBootSection {
            tuned_active: "my-custom-profile".into(),
            ..Default::default()
        };

        let merged = merge_kernelboot_sections(
            vec![Some(make_host()), Some(make_host()), Some(make_host())],
            3,
            &["host-a".into(), "host-b".into(), "host-c".into()],
        )
        .expect("merge should succeed");

        assert!(
            merged.tuned_include,
            "custom tuned profile universal across all hosts must be included"
        );
    }

    #[test]
    fn aggregate_merge_tuned_non_universal_custom_excluded() {
        use crate::types::kernelboot::KernelBootSection;

        // 2 of 3 hosts have my-custom-profile, 1 has something else
        let host_a = KernelBootSection {
            tuned_active: "my-custom-profile".into(),
            ..Default::default()
        };
        let host_b = KernelBootSection {
            tuned_active: "my-custom-profile".into(),
            ..Default::default()
        };
        let host_c = KernelBootSection {
            tuned_active: "throughput-performance".into(),
            ..Default::default()
        };

        let merged = merge_kernelboot_sections(
            vec![Some(host_a), Some(host_b), Some(host_c)],
            3,
            &["host-a".into(), "host-b".into(), "host-c".into()],
        )
        .expect("merge should succeed");

        assert!(
            !merged.tuned_include,
            "non-universal custom tuned profile must be excluded"
        );
    }

    #[test]
    fn aggregate_merge_narrowing_applies_to_all_item_types() {
        // Verify narrowing works generically through merge_items
        // by testing with PackageEntry (non-variant type)
        let items: Vec<(usize, PackageEntry)> = vec![
            (
                0,
                PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    ..Default::default()
                },
            ),
            (
                1,
                PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    ..Default::default()
                },
            ),
            // host 2 does NOT have httpd
        ];

        let result = merge_items(
            items,
            3,
            &["host-a".into(), "host-b".into(), "host-c".into()],
        );
        assert_eq!(result.len(), 1);
        let pkg = &result[0];
        assert!(
            !pkg.include,
            "non-universal package must have include=false after narrowing"
        );
        assert_eq!(pkg.aggregate.as_ref().unwrap().count, 2);
    }

    #[test]
    fn aggregate_merge_narrowing_universal_item_stays_included() {
        // All 3 hosts have the same package
        let items: Vec<(usize, PackageEntry)> = vec![
            (
                0,
                PackageEntry {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    ..Default::default()
                },
            ),
            (
                1,
                PackageEntry {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    ..Default::default()
                },
            ),
            (
                2,
                PackageEntry {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    ..Default::default()
                },
            ),
        ];

        let result = merge_items(
            items,
            3,
            &["host-a".into(), "host-b".into(), "host-c".into()],
        );
        assert_eq!(result.len(), 1);
        let pkg = &result[0];
        assert!(pkg.include, "universal package must remain include=true");
        assert_eq!(pkg.aggregate.as_ref().unwrap().count, 3);
    }

    #[test]
    fn leaf_filter_preserves_partial_packages_with_include_false() {
        // Scenario: 3 hosts, all authoritative for leaf classification.
        //   - "vim" is a leaf on all 3 hosts (universal leaf) → kept, include=true
        //   - "htop" is a leaf on hosts A and B only (partial leaf) → kept, include=false
        //   - "glibc" is on all 3 hosts but NOT a leaf (transitive) → filtered out
        use crate::types::rpm::{PackageEntry, PackageState, RpmSection};

        let make_section = |pkgs: Vec<(&str, &str)>, leaves: Vec<&str>| -> RpmSection {
            RpmSection {
                packages_added: pkgs
                    .into_iter()
                    .map(|(name, arch)| PackageEntry {
                        name: name.into(),
                        arch: arch.into(),
                        state: PackageState::Added,
                        include: true,
                        ..Default::default()
                    })
                    .collect(),
                leaf_packages: Some(leaves.into_iter().map(String::from).collect()),
                leaf_dep_tree: serde_json::json!({}),
                ..Default::default()
            }
        };

        // Host A: has vim, htop, glibc; leaves are vim and htop
        let host_a = make_section(
            vec![("vim", "x86_64"), ("htop", "x86_64"), ("glibc", "x86_64")],
            vec!["vim.x86_64", "htop.x86_64"],
        );
        // Host B: has vim, htop, glibc; leaves are vim and htop
        let host_b = make_section(
            vec![("vim", "x86_64"), ("htop", "x86_64"), ("glibc", "x86_64")],
            vec!["vim.x86_64", "htop.x86_64"],
        );
        // Host C: has vim, glibc (NO htop); leaves are vim only
        let host_c = make_section(
            vec![("vim", "x86_64"), ("glibc", "x86_64")],
            vec!["vim.x86_64"],
        );

        let hostnames = vec!["host-a".into(), "host-b".into(), "host-c".into()];
        let (merged, _conflicts) = merge_rpm_sections(
            vec![Some(host_a), Some(host_b), Some(host_c)],
            3,
            &hostnames,
            None,
        )
        .expect("merge should succeed");

        // vim: universal leaf → present with include=true
        let vim = merged
            .packages_added
            .iter()
            .find(|p| p.name == "vim")
            .expect("vim should survive leaf filter");
        assert!(vim.include, "universal leaf vim must have include=true");
        assert_eq!(vim.aggregate.as_ref().unwrap().count, 3);

        // htop: partial leaf (2/3 hosts) → present with include=false
        let htop = merged
            .packages_added
            .iter()
            .find(|p| p.name == "htop")
            .expect("partial leaf htop must survive leaf filter, not be deleted");
        assert!(
            !htop.include,
            "partial leaf htop must have include=false (narrow_non_universal)"
        );
        assert_eq!(htop.aggregate.as_ref().unwrap().count, 2);

        // glibc: transitive on all hosts (not a leaf anywhere) → filtered out
        let glibc = merged.packages_added.iter().find(|p| p.name == "glibc");
        assert!(
            glibc.is_none(),
            "transitive package glibc must be filtered out by leaf filter"
        );
    }

    #[test]
    fn nonrpm_aggregate_identity_key_composite() {
        use crate::types::nonrpm::NonRpmItem;

        let item = NonRpmItem {
            name: "requests".to_string(),
            path: "/opt/myapp/venv".to_string(),
            method: "venv".to_string(),
            ..Default::default()
        };
        assert_eq!(item.identity_key().as_ref(), "pip:/opt/myapp/venv");
    }

    #[test]
    fn nonrpm_aggregate_identity_key_npm() {
        use crate::types::nonrpm::NonRpmItem;

        let item = NonRpmItem {
            name: "express".to_string(),
            path: "/srv/webapp".to_string(),
            method: "npm lockfile".to_string(),
            ..Default::default()
        };
        assert_eq!(item.identity_key().as_ref(), "npm:/srv/webapp");
    }

    #[test]
    fn nonrpm_aggregate_identity_key_fallback() {
        use crate::types::nonrpm::NonRpmItem;

        let item = NonRpmItem {
            name: "custom-binary".to_string(),
            path: String::new(),
            method: "binary".to_string(),
            ..Default::default()
        };
        assert_eq!(item.identity_key().as_ref(), "custom-binary");
    }

    #[test]
    fn nonrpm_merge_100_pct_prevalence_includes() {
        use crate::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};

        // Two hosts, same environment on both → 100% prevalence → include: true
        let section_a = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                name: "myapp-venv".to_string(),
                path: "/opt/myapp/venv".to_string(),
                method: "venv".to_string(),
                include: true,
                ..Default::default()
            }],
            env_files: vec![],
        });
        let section_b = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                name: "myapp-venv".to_string(),
                path: "/opt/myapp/venv".to_string(),
                method: "venv".to_string(),
                include: true,
                ..Default::default()
            }],
            env_files: vec![],
        });

        let merged = merge_nonrpm_sections(
            vec![section_a, section_b],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .unwrap();

        let item = &merged.items[0];
        let agg = item.aggregate.as_ref().unwrap();
        assert_eq!(agg.count, 2);
        assert_eq!(agg.total, 2);
        assert!(
            item.include,
            "100% prevalence should default to include: true"
        );
    }

    #[test]
    fn nonrpm_merge_partial_prevalence_excludes() {
        use crate::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};

        // Two hosts, environment on only one → 50% prevalence → include: false
        let section_a = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                name: "myapp-venv".to_string(),
                path: "/opt/myapp/venv".to_string(),
                method: "venv".to_string(),
                include: true,
                ..Default::default()
            }],
            env_files: vec![],
        });
        let section_b = Some(NonRpmSoftwareSection {
            items: vec![],
            env_files: vec![],
        });

        let merged = merge_nonrpm_sections(
            vec![section_a, section_b],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .unwrap();

        let item = &merged.items[0];
        let agg = item.aggregate.as_ref().unwrap();
        assert_eq!(agg.count, 1);
        assert_eq!(agg.total, 2);
        assert!(
            !item.include,
            "partial prevalence should default to include: false"
        );
    }

    // -----------------------------------------------------------------------
    // UnmanagedFile merge tests
    // -----------------------------------------------------------------------

    #[test]
    fn unmanaged_file_merge_by_path_same_content() {
        use crate::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};

        // Same path + same content hash -> single merged item
        let section_a = Some(UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/splunk/bin/splunkd".to_string(),
                size: 52_000_000,
                include: true,
                content_hash: "aaa111".to_string(),
                ..Default::default()
            }],
            total_size: 52_000_000,
            total_count: 1,
        });
        let section_b = Some(UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/splunk/bin/splunkd".to_string(),
                size: 52_000_000,
                include: true,
                content_hash: "aaa111".to_string(),
                ..Default::default()
            }],
            total_size: 52_000_000,
            total_count: 1,
        });

        let merged = merge_unmanaged_file_sections(
            vec![section_a, section_b],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .unwrap();

        // Same path + same content hash -> merged into one item.
        assert_eq!(merged.items.len(), 1);
        let file = &merged.items[0];
        assert_eq!(file.path, "/opt/splunk/bin/splunkd");
        let agg = file.aggregate.as_ref().unwrap();
        assert_eq!(agg.count, 2);
        assert_eq!(agg.total, 2);
        // Totals recomputed
        assert_eq!(merged.total_count, 1);
        assert_eq!(merged.total_size, 52_000_000);
    }

    #[test]
    fn unmanaged_file_merge_by_path_different_content_creates_variants() {
        use crate::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};

        // Same path but different content hashes -> two variant entries
        let section_a = Some(UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/splunk/bin/splunkd".to_string(),
                size: 52_000_000,
                include: true,
                content_hash: "aaa111".to_string(),
                ..Default::default()
            }],
            total_size: 52_000_000,
            total_count: 1,
        });
        let section_b = Some(UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/splunk/bin/splunkd".to_string(),
                size: 52_000_000,
                include: true,
                content_hash: "bbb222".to_string(),
                ..Default::default()
            }],
            total_size: 52_000_000,
            total_count: 1,
        });

        let merged = merge_unmanaged_file_sections(
            vec![section_a, section_b],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .unwrap();

        // Different content hashes -> two variant entries, each on one host.
        assert_eq!(merged.items.len(), 2);
        for file in &merged.items {
            assert_eq!(file.path, "/opt/splunk/bin/splunkd");
            let agg = file.aggregate.as_ref().unwrap();
            assert_eq!(agg.count, 1);
            assert_eq!(agg.total, 2);
        }
        // Totals recomputed from the two variant items
        assert_eq!(merged.total_count, 2);
        assert_eq!(merged.total_size, 104_000_000);
    }

    #[test]
    fn unmanaged_file_merge_100_pct_prevalence_includes() {
        use crate::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};

        // File present on all hosts -> 100% -> include: true
        let section_a = Some(UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/app/server".to_string(),
                size: 10_000,
                include: true,
                ..Default::default()
            }],
            total_size: 10_000,
            total_count: 1,
        });
        let section_b = Some(UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/app/server".to_string(),
                size: 10_000,
                include: true,
                ..Default::default()
            }],
            total_size: 10_000,
            total_count: 1,
        });

        let merged = merge_unmanaged_file_sections(
            vec![section_a, section_b],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .unwrap();

        let file = &merged.items[0];
        let agg = file.aggregate.as_ref().unwrap();
        assert_eq!(agg.count, 2);
        assert_eq!(agg.total, 2);
        assert!(
            file.include,
            "100% prevalence should default to include: true"
        );
    }

    #[test]
    fn unmanaged_file_merge_partial_prevalence_excludes() {
        use crate::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};

        // File on 1 of 2 hosts -> 50% -> include: false
        let section_a = Some(UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/app/server".to_string(),
                size: 10_000,
                include: true,
                ..Default::default()
            }],
            total_size: 10_000,
            total_count: 1,
        });
        let section_b = Some(UnmanagedFileSection {
            items: vec![],
            total_size: 0,
            total_count: 0,
        });

        let merged = merge_unmanaged_file_sections(
            vec![section_a, section_b],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .unwrap();

        let file = &merged.items[0];
        let agg = file.aggregate.as_ref().unwrap();
        assert_eq!(agg.count, 1);
        assert_eq!(agg.total, 2);
        assert!(
            !file.include,
            "partial prevalence should default to include: false"
        );
    }
}
