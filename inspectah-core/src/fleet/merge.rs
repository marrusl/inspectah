use std::borrow::Cow;

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
