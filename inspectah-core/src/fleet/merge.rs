use std::borrow::Cow;
use std::collections::HashMap;

use crate::types::fleet::{FleetPrevalence, VariantSelection};

/// Trait for types that participate in fleet prevalence tracking.
///
/// Every type that can be merged across multiple host snapshots implements
/// this trait. The generic merge engine uses these methods to group items
/// by identity, attach prevalence metadata, and handle content variants.
pub trait FleetMergeable: Clone {
    /// Unique identity key for grouping across hosts.
    fn identity_key(&self) -> Cow<'_, str>;

    /// Mutable reference to the fleet prevalence field.
    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence>;

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

use crate::types::rpm::{
    EnabledModuleStream, PackageEntry, RepoFile, VersionLockEntry,
};

impl FleetMergeable for PackageEntry {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}.{}", self.name, self.arch))
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl FleetMergeable for RepoFile {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl FleetMergeable for EnabledModuleStream {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}:{}", self.module_name, self.stream))
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl FleetMergeable for VersionLockEntry {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}.{}", self.name, self.arch))
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Config types
// ---------------------------------------------------------------------------

use crate::types::config::ConfigFileEntry;

impl FleetMergeable for ConfigFileEntry {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
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

impl FleetMergeable for ServiceStateChange {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.unit)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl FleetMergeable for SystemdDropIn {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
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

use crate::types::containers::{ComposeFile, QuadletUnit};

impl FleetMergeable for QuadletUnit {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
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

impl FleetMergeable for ComposeFile {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
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

// ---------------------------------------------------------------------------
// Network types
// ---------------------------------------------------------------------------

use crate::types::network::{FirewallZone, NMConnection};

impl FleetMergeable for NMConnection {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = Some(val);
    }
}

impl FleetMergeable for FirewallZone {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Security types
// ---------------------------------------------------------------------------

use crate::types::selinux::SelinuxPortLabel;

impl FleetMergeable for SelinuxPortLabel {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Owned(format!("{}:{}", self.protocol, self.port))
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Kernel/boot types
// ---------------------------------------------------------------------------

use crate::types::kernelboot::{KernelModule, SysctlOverride};

impl FleetMergeable for KernelModule {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.name)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

impl FleetMergeable for SysctlOverride {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.key)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Non-RPM types
// ---------------------------------------------------------------------------

use crate::types::nonrpm::NonRpmItem;

impl FleetMergeable for NonRpmItem {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.name)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Scheduled task types
// ---------------------------------------------------------------------------

use crate::types::scheduled::CronJob;

impl FleetMergeable for CronJob {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence> {
        &mut self.fleet
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }
}

// ---------------------------------------------------------------------------
// Generic merge engine
// ---------------------------------------------------------------------------

/// Merge items from multiple host snapshots into a deduplicated list with
/// fleet prevalence metadata.
///
/// Each input tuple is `(host_index, item)` where `host_index` refers to
/// the position in `hostnames`. Items are grouped by identity key, and each
/// group gets a [`FleetPrevalence`] recording how many hosts (and which ones)
/// contributed that item. Types that support content variants are further
/// sub-grouped by content hash (see [`merge_with_variants`]).
///
/// The returned list is sorted by identity key for deterministic output.
pub fn merge_items<T: FleetMergeable>(
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
    for (_key, group) in &mut groups {
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
            let count = hosts.len() as i32;
            let mut representative = group[0].1.clone();
            *representative.fleet_mut() = Some(FleetPrevalence {
                count,
                total: total_hosts as i32,
                hosts,
            });
            representative.set_include(true);
            result.push(representative);
        }
    }

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
fn merge_with_variants<T: FleetMergeable>(
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

    fn unique_hosts<U: FleetMergeable>(
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
        *item.fleet_mut() = Some(FleetPrevalence {
            count: hosts.len() as i32,
            total: total_hosts as i32,
            hosts,
        });
        item.set_include(true);
        // variant_selection_mut defaults to Only, no change needed
        return vec![item];
    }

    // Multiple variants — rank by prevalence, break ties by hash
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
        *item.fleet_mut() = Some(FleetPrevalence {
            count: hosts.len() as i32,
            total: total_hosts as i32,
            hosts,
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
    variant_results
}
