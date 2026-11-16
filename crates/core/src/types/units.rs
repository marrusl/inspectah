//! systemd unit names and the kinds of unit inspectah models.
//!
//! A [`UnitName`] is parsed once, at the collector or when a snapshot is
//! loaded, and every later use reads a proved value. Its kind, and whether it
//! is a template or an instance, are read from the name and never stored
//! beside it.
//!
//! The snapshot's `units` section, [`UnitSection`], and its state changes
//! and drop-ins live here too.

use super::FindingKind;
use super::aggregate::{AggregatePrevalence, VariantSelection};
use super::finding::ShadowType;
use serde::{Deserialize, Serialize};

/// Longest unit name systemd accepts, in bytes.
const UNIT_NAME_MAX_BYTES: usize = 255;

/// Characters `systemd.unit(5)` allows in a unit name besides ASCII letters
/// and digits.
const UNIT_NAME_PUNCTUATION: &str = r":_.\@-";

/// Unit kinds that are not storage: carried as unit state whatever their
/// fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OrdinaryKind {
    Service,
    Socket,
    Path,
    Timer,
    Target,
    Slice,
}

/// Kinds that describe storage. Carried only when a package owns the unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StorageKind {
    Mount,
    Automount,
    Swap,
}

/// Every unit type suffix systemd reads under `/etc/systemd/system`,
/// `device` and `scope` included: a file or drop-in directory may name
/// those kinds even though [`UnitName`] does not admit them.
pub const UNIT_TYPE_SUFFIXES: &[&str] = &[
    "service",
    "socket",
    "device",
    "mount",
    "automount",
    "swap",
    "target",
    "path",
    "timer",
    "slice",
    "scope",
];

/// The kind of a unit, read from its name's suffix.
///
/// `.device` and `.scope` have no variant: they never exist as unit files.
/// The order (ordinary kinds, then storage kinds, each in declaration
/// order) is the order units are listed in a `systemctl` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UnitKind {
    Ordinary(OrdinaryKind),
    Storage(StorageKind),
}

impl UnitKind {
    /// The kind a type suffix (the text after the last `.`) names, if any.
    fn from_suffix(suffix: &str) -> Option<Self> {
        let kind = match suffix {
            "service" => Self::Ordinary(OrdinaryKind::Service),
            "socket" => Self::Ordinary(OrdinaryKind::Socket),
            "path" => Self::Ordinary(OrdinaryKind::Path),
            "timer" => Self::Ordinary(OrdinaryKind::Timer),
            "target" => Self::Ordinary(OrdinaryKind::Target),
            "slice" => Self::Ordinary(OrdinaryKind::Slice),
            "mount" => Self::Storage(StorageKind::Mount),
            "automount" => Self::Storage(StorageKind::Automount),
            "swap" => Self::Storage(StorageKind::Swap),
            _ => return None,
        };
        Some(kind)
    }

    /// The type suffix that names this kind, lowercase, without the `.`:
    /// the inverse of the suffix reading, and how refine names a row's
    /// kind (§ 11.1).
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Ordinary(OrdinaryKind::Service) => "service",
            Self::Ordinary(OrdinaryKind::Socket) => "socket",
            Self::Ordinary(OrdinaryKind::Path) => "path",
            Self::Ordinary(OrdinaryKind::Timer) => "timer",
            Self::Ordinary(OrdinaryKind::Target) => "target",
            Self::Ordinary(OrdinaryKind::Slice) => "slice",
            Self::Storage(StorageKind::Mount) => "mount",
            Self::Storage(StorageKind::Automount) => "automount",
            Self::Storage(StorageKind::Swap) => "swap",
        }
    }
}

/// Why a string is not a [`UnitName`]. Each variant names the rule it failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnitNameError {
    /// The 1-byte minimum.
    #[error("unit name is empty")]
    Empty,
    /// The 255-byte maximum.
    #[error("unit name is {len} bytes, over the {UNIT_NAME_MAX_BYTES}-byte limit")]
    TooLong { len: usize },
    /// The character set; `ch` is the first character outside it.
    #[error(
        "unit name contains the character {ch:?}, outside the systemd unit name set \
         [A-Za-z0-9{UNIT_NAME_PUNCTUATION}]"
    )]
    InvalidCharacter { ch: char },
    /// The suffix of a kind this model knows. `.device` and `.scope` fail here.
    #[error(
        "unit name {name:?} has no recognized type suffix (service, socket, path, timer, \
         target, slice, mount, automount, swap)"
    )]
    UnknownSuffix { name: String },
}

/// A systemd unit name with a recognized type suffix.
///
/// Parsed at the collector and at deserialization, so every later use is
/// of a proved value: 1 to 255 bytes, drawn from `[A-Za-z0-9:_.\\@-]`,
/// ending in the suffix of a kind this model knows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct UnitName(String);

impl UnitName {
    /// The kind named by the suffix.
    pub fn kind(&self) -> UnitKind {
        UnitKind::from_suffix(self.parts().1).expect("suffix proved when the name was parsed")
    }

    /// The name as systemd spells it.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `Some` for `prefix@instance.suffix` with a non-empty instance.
    ///
    /// The prefix ends at the first `@`, as systemd splits it, so the
    /// instance may itself contain `@` and `.`.
    pub fn instance(&self) -> Option<InstanceName<'_>> {
        let (stem, suffix) = self.parts();
        let (prefix, instance) = stem.split_once('@')?;
        (!instance.is_empty()).then_some(InstanceName {
            prefix,
            instance,
            suffix,
        })
    }

    /// True for `prefix@.suffix`.
    pub fn is_template(&self) -> bool {
        matches!(self.parts().0.split_once('@'), Some((_, "")))
    }

    /// The name split at its last `.` into stem and suffix.
    fn parts(&self) -> (&str, &str) {
        self.0
            .rsplit_once('.')
            .expect("suffix proved when the name was parsed")
    }
}

impl TryFrom<String> for UnitName {
    type Error = UnitNameError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        if name.is_empty() {
            return Err(UnitNameError::Empty);
        }
        if name.len() > UNIT_NAME_MAX_BYTES {
            return Err(UnitNameError::TooLong { len: name.len() });
        }
        if let Some(ch) = name
            .chars()
            .find(|&c| !c.is_ascii_alphanumeric() && !UNIT_NAME_PUNCTUATION.contains(c))
        {
            return Err(UnitNameError::InvalidCharacter { ch });
        }
        match name.rsplit_once('.') {
            Some((_, suffix)) if UnitKind::from_suffix(suffix).is_some() => Ok(Self(name)),
            _ => Err(UnitNameError::UnknownSuffix { name }),
        }
    }
}

impl From<UnitName> for String {
    fn from(name: UnitName) -> Self {
        name.0
    }
}

/// The parts of an instance name, borrowed from the [`UnitName`].
///
/// The parts are read-only: [`InstanceName::template`] builds a proved name
/// from them without a re-parse, so a caller must not be able to swap in a
/// prefix the parser never checked.
///
/// ```compile_fail,E0616
/// use inspectah_core::types::units::UnitName;
///
/// let name = UnitName::try_from("getty@tty1.service".to_string()).unwrap();
/// let mut parts = name.instance().unwrap();
/// parts.prefix = "bad name";
/// let _ = parts.template();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceName<'a> {
    prefix: &'a str,
    instance: &'a str,
    /// The type suffix, kept to build the template name.
    suffix: &'a str,
}

impl<'a> InstanceName<'a> {
    /// The text before the first `@` (`getty` in `getty@tty1.service`).
    pub fn prefix(&self) -> &'a str {
        self.prefix
    }

    /// The non-empty text between the first `@` and the suffix (`tty1`).
    pub fn instance(&self) -> &'a str {
        self.instance
    }

    /// `prefix@.suffix`, the template an instance falls back to.
    pub fn template(&self) -> UnitName {
        // Proved without a re-parse: the prefix and suffix come from a proved
        // name, and dropping a non-empty instance only shortens it.
        UnitName(format!("{}@.{}", self.prefix, self.suffix))
    }
}

/// A [`UnitName`] proved to be of an ordinary kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrdinaryUnitName(UnitName);

impl OrdinaryUnitName {
    /// The proved name.
    pub fn as_unit_name(&self) -> &UnitName {
        &self.0
    }
}

/// A [`UnitName`] proved to be a mount, automount or swap unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageUnitName(UnitName, StorageKind);

impl StorageUnitName {
    /// The proved name.
    pub fn as_unit_name(&self) -> &UnitName {
        &self.0
    }

    /// The storage kind the name's suffix names.
    pub fn kind(&self) -> StorageKind {
        self.1
    }
}

/// A [`UnitName`] proved to be a `.target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetUnitName(UnitName);

impl TargetUnitName {
    /// The proved name.
    pub fn as_unit_name(&self) -> &UnitName {
        &self.0
    }
}

/// The unit file behind a name on the source host (§ 7.2 of the units
/// spec). `file` is the name of the file that loads: the unit's own name,
/// or its template's for an instance that has no file of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "fragment", rename_all = "snake_case")]
pub enum UnitFragment {
    /// Under `/usr/lib/systemd/system` only. `owning_package` is `None`
    /// when no package owns the file or the RPM query failed.
    Vendor {
        file: UnitName,
        owning_package: Option<String>,
    },
    /// Under `/etc/systemd/system` only: a file the administrator wrote.
    Local { file: UnitName },
    /// Both. The `/etc` file replaces the vendor one.
    Shadowed {
        file: UnitName,
        owning_package: Option<String>,
    },
    /// No unit file at all. Only a masked name can have this.
    Absent,
}

impl UnitFragment {
    /// The name of the file that loads, when there is one.
    pub fn file(&self) -> Option<&UnitName> {
        match self {
            Self::Vendor { file, .. } | Self::Local { file } | Self::Shadowed { file, .. } => {
                Some(file)
            }
            Self::Absent => None,
        }
    }
}

/// Why a unit and its fragment cannot be the value asked for. Each variant
/// names the rule that refused it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnitSubjectError {
    /// The fragment's file is not the unit's own name or its template's.
    #[error(
        "unit file {} is neither {} nor its template",
        file.as_str(),
        name.as_str()
    )]
    FileNotUnitOrTemplate { name: UnitName, file: UnitName },
    /// A mount, automount or swap unit's file is not its own name.
    #[error(
        "unit file {} is not {}: a mount, automount or swap unit has no template, \
         so its file is its own name",
        file.as_str(),
        name.as_str()
    )]
    StorageFileNotUnit { name: UnitName, file: UnitName },
    /// The fragment names an owning package that is an empty string.
    #[error(
        "{} names an empty owning package: an owner is a package name, or null when \
         none is known",
        name.as_str()
    )]
    EmptyOwningPackage { name: UnitName },
    /// `classify` placed the unit as advisory where a carried unit was asked
    /// for.
    #[error(
        "classify places {} ({fragment}) as advisory, not carried: a mount, automount \
         or swap unit is carried only under the vendor directory with a known owning package",
        name.as_str()
    )]
    AdvisoryNotCarried {
        name: UnitName,
        fragment: AdminStorageFragment,
    },
    /// `classify` placed the unit as carried where an advisory unit was asked
    /// for.
    #[error(
        "classify places {} as carried, not advisory: an ordinary unit, or a mount, \
         automount or swap unit under the vendor directory with a known owning package, \
         is a state change and not a mount finding",
        name.as_str()
    )]
    CarriedNotAdvisory { name: UnitName },
    /// The unit is not a `.target`, the only operand `set-default` takes.
    #[error(
        "{} is not a .target unit: set-default takes a target only",
        name.as_str()
    )]
    NotTarget { name: UnitName },
}

/// The wire form of a unit and its fragment:
/// `{"name": "tmp.mount", "fragment": "vendor", "file": "tmp.mount",
///   "owning_package": "systemd"}`.
///
/// The fields are private: [`UnitSubject::new`] checks the fragment's file
/// against the name, and [`UnitSubject::classify`] trusts that check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UnitSubjectWire")]
pub struct UnitSubject {
    name: UnitName,
    #[serde(flatten)]
    fragment: UnitFragment,
}

/// The unchecked wire form [`UnitSubject`] is deserialized through.
#[derive(Deserialize)]
struct UnitSubjectWire {
    name: UnitName,
    #[serde(flatten)]
    fragment: UnitFragment,
}

impl TryFrom<UnitSubjectWire> for UnitSubject {
    type Error = UnitSubjectError;

    fn try_from(wire: UnitSubjectWire) -> Result<Self, Self::Error> {
        Self::new(wire.name, wire.fragment)
    }
}

impl UnitSubject {
    /// A unit and its fragment, refused when the fragment's file is
    /// neither the unit's name nor its template's, when a mount, automount
    /// or swap unit's file is not its own name (systemd has no template for
    /// those kinds), or when the owning package is an empty string.
    pub fn new(name: UnitName, fragment: UnitFragment) -> Result<Self, UnitSubjectError> {
        if let Some(file) = fragment.file()
            && *file != name
        {
            if let UnitKind::Storage(_) = name.kind() {
                return Err(UnitSubjectError::StorageFileNotUnit {
                    name,
                    file: file.clone(),
                });
            }
            let is_template = name
                .instance()
                .is_some_and(|parts| parts.template() == *file);
            if !is_template {
                return Err(UnitSubjectError::FileNotUnitOrTemplate {
                    name,
                    file: file.clone(),
                });
            }
        }
        if let UnitFragment::Vendor {
            owning_package: Some(owner),
            ..
        }
        | UnitFragment::Shadowed {
            owning_package: Some(owner),
            ..
        } = &fragment
            && owner.is_empty()
        {
            return Err(UnitSubjectError::EmptyOwningPackage { name });
        }
        Ok(Self { name, fragment })
    }

    /// The unit's name.
    pub fn name(&self) -> &UnitName {
        &self.name
    }

    /// The unit file behind the name.
    pub fn fragment(&self) -> &UnitFragment {
        &self.fragment
    }

    /// Total: every name and fragment is exactly one of the two. This is
    /// the one place the rule in § 2 of the units spec is written: a mount,
    /// automount or swap unit is carried only under the vendor directory
    /// with a known owning package.
    pub fn classify(self) -> Classified {
        let Self { name, fragment } = self;
        let UnitKind::Storage(kind) = name.kind() else {
            return Classified::Carried(CarriedUnit::Ordinary(OrdinaryUnit {
                name: OrdinaryUnitName(name),
                fragment,
            }));
        };
        let name = StorageUnitName(name, kind);
        let fragment = match fragment {
            UnitFragment::Vendor {
                owning_package: Some(owning_package),
                ..
            } => {
                return Classified::Carried(CarriedUnit::PackagedStorage(PackagedStorageUnit {
                    name,
                    owning_package,
                }));
            }
            UnitFragment::Vendor {
                owning_package: None,
                ..
            } => AdminStorageFragment::UnownedVendor,
            UnitFragment::Local { .. } => AdminStorageFragment::Local,
            UnitFragment::Shadowed { owning_package, .. } => {
                AdminStorageFragment::Shadowed { owning_package }
            }
            UnitFragment::Absent => AdminStorageFragment::MaskOnly,
        };
        Classified::Advisory(AdvisoryUnit { name, fragment })
    }
}

/// The outcome of [`UnitSubject::classify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classified {
    Carried(CarriedUnit),
    Advisory(AdvisoryUnit),
}

/// A unit inspectah may consider for a `systemctl` line. The only subject
/// a state change holds, and the only input the action plan accepts
/// (§ 10.2 of the units spec).
///
/// Each variant wraps a type with private fields, so a `CarriedUnit` is
/// built only by [`UnitSubject::classify`], after the file check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UnitSubject", into = "UnitSubject")]
pub enum CarriedUnit {
    /// Any fragment: a local service is carried with its file.
    Ordinary(OrdinaryUnit),
    /// A storage unit under the vendor directory with a known owner.
    PackagedStorage(PackagedStorageUnit),
}

impl CarriedUnit {
    /// The unit's name.
    pub fn name(&self) -> &UnitName {
        match self {
            Self::Ordinary(unit) => unit.name.as_unit_name(),
            Self::PackagedStorage(unit) => unit.name.as_unit_name(),
        }
    }

    /// The package known to own the unit's vendor file: `None` for a local
    /// unit, a unit with no file, or a vendor file whose owner is unknown.
    pub fn owning_package(&self) -> Option<&str> {
        match self {
            Self::Ordinary(unit) => match &unit.fragment {
                UnitFragment::Vendor { owning_package, .. }
                | UnitFragment::Shadowed { owning_package, .. } => owning_package.as_deref(),
                UnitFragment::Local { .. } | UnitFragment::Absent => None,
            },
            Self::PackagedStorage(unit) => Some(&unit.owning_package),
        }
    }
}

/// An ordinary unit and its fragment, whatever the fragment is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrdinaryUnit {
    name: OrdinaryUnitName,
    fragment: UnitFragment,
}

impl OrdinaryUnit {
    /// The unit's name.
    pub fn name(&self) -> &OrdinaryUnitName {
        &self.name
    }

    /// The unit file behind the name.
    pub fn fragment(&self) -> &UnitFragment {
        &self.fragment
    }
}

/// A storage unit under the vendor directory with a known owner. There is
/// no field here that could hold an `/etc` path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagedStorageUnit {
    name: StorageUnitName,
    owning_package: String,
}

impl PackagedStorageUnit {
    /// The unit's name.
    pub fn name(&self) -> &StorageUnitName {
        &self.name
    }

    /// The package that owns the vendor file.
    pub fn owning_package(&self) -> &str {
        &self.owning_package
    }
}

impl TryFrom<UnitSubject> for CarriedUnit {
    type Error = UnitSubjectError;

    fn try_from(subject: UnitSubject) -> Result<Self, Self::Error> {
        match subject.classify() {
            Classified::Carried(unit) => Ok(unit),
            Classified::Advisory(AdvisoryUnit { name, fragment }) => {
                Err(UnitSubjectError::AdvisoryNotCarried {
                    name: name.0,
                    fragment,
                })
            }
        }
    }
}

impl From<CarriedUnit> for UnitSubject {
    fn from(unit: CarriedUnit) -> Self {
        match unit {
            CarriedUnit::Ordinary(OrdinaryUnit { name, fragment }) => Self {
                name: name.0,
                fragment,
            },
            CarriedUnit::PackagedStorage(PackagedStorageUnit {
                name,
                owning_package,
            }) => Self {
                fragment: UnitFragment::Vendor {
                    file: name.0.clone(),
                    owning_package: Some(owning_package),
                },
                name: name.0,
            },
        }
    }
}

/// A [`CarriedUnit`] proved to be a `.target`: the only operand
/// `set-default` takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UnitSubject", into = "UnitSubject")]
pub struct TargetUnit {
    name: TargetUnitName,
    fragment: UnitFragment,
}

impl TargetUnit {
    /// The target's name.
    pub fn name(&self) -> &TargetUnitName {
        &self.name
    }

    /// The unit file behind the name, kept for the file check.
    pub fn fragment(&self) -> &UnitFragment {
        &self.fragment
    }
}

impl TryFrom<UnitSubject> for TargetUnit {
    type Error = UnitSubjectError;

    fn try_from(subject: UnitSubject) -> Result<Self, Self::Error> {
        match CarriedUnit::try_from(subject)? {
            CarriedUnit::Ordinary(OrdinaryUnit { name, fragment })
                if name.0.kind() == UnitKind::Ordinary(OrdinaryKind::Target) =>
            {
                Ok(Self {
                    name: TargetUnitName(name.0),
                    fragment,
                })
            }
            other => Err(UnitSubjectError::NotTarget {
                name: other.name().clone(),
            }),
        }
    }
}

/// A target is an ordinary unit, so it is always carried.
impl From<TargetUnit> for CarriedUnit {
    fn from(unit: TargetUnit) -> Self {
        Self::Ordinary(OrdinaryUnit {
            name: OrdinaryUnitName(unit.name.0),
            fragment: unit.fragment,
        })
    }
}

impl From<TargetUnit> for UnitSubject {
    fn from(unit: TargetUnit) -> Self {
        Self {
            name: unit.name.0,
            fragment: unit.fragment,
        }
    }
}

/// Why a storage unit counts as the administrator's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminStorageFragment {
    /// Under `/etc/systemd/system` only.
    Local,
    /// A vendor file with a full copy in `/etc/systemd/system`.
    Shadowed { owning_package: Option<String> },
    /// Under the vendor directory, with no package known to own it.
    UnownedVendor,
    /// A mask on a name that has no unit file anywhere.
    MaskOnly,
}

impl std::fmt::Display for AdminStorageFragment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Local => "local fragment",
            Self::Shadowed { .. } => "shadowed fragment",
            Self::UnownedVendor => "vendor fragment with no known owning package",
            Self::MaskOnly => "absent fragment, a mask only",
        };
        f.write_str(s)
    }
}

/// A storage unit inspectah reports and never acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UnitSubject", into = "UnitSubject")]
pub struct AdvisoryUnit {
    name: StorageUnitName,
    fragment: AdminStorageFragment,
}

impl AdvisoryUnit {
    /// The unit's name.
    pub fn name(&self) -> &StorageUnitName {
        &self.name
    }

    /// Why the unit is the administrator's.
    pub fn fragment(&self) -> &AdminStorageFragment {
        &self.fragment
    }
}

impl TryFrom<UnitSubject> for AdvisoryUnit {
    type Error = UnitSubjectError;

    fn try_from(subject: UnitSubject) -> Result<Self, Self::Error> {
        match subject.classify() {
            Classified::Advisory(unit) => Ok(unit),
            Classified::Carried(unit) => Err(UnitSubjectError::CarriedNotAdvisory {
                name: unit.name().clone(),
            }),
        }
    }
}

impl From<AdvisoryUnit> for UnitSubject {
    fn from(unit: AdvisoryUnit) -> Self {
        let file = unit.name.0.clone();
        let fragment = match unit.fragment {
            AdminStorageFragment::Local => UnitFragment::Local { file },
            AdminStorageFragment::Shadowed { owning_package } => UnitFragment::Shadowed {
                file,
                owning_package,
            },
            AdminStorageFragment::UnownedVendor => UnitFragment::Vendor {
                file,
                owning_package: None,
            },
            AdminStorageFragment::MaskOnly => UnitFragment::Absent,
        };
        Self {
            name: unit.name.0,
            fragment,
        }
    }
}

/// Why a unit's `[Install]` section cannot be relied on. The collector
/// keeps it for the warning, and it travels with every finding the section
/// produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallUnknown {
    /// A value holds a specifier other than `%i`, `%p` or `%N`. `systemctl
    /// enable` does not expand `%I`.
    Specifier,
    /// An empty `RequiredBy=` follows a `RequiredBy=` value in load order.
    /// Whether it clears the value was not measured.
    RequiredByReset,
    /// A value is not a unit name this model admits.
    ForeignName,
    /// The section assigns `UpheldBy=`, an empty assignment included.
    UpheldBy,
    /// The unit file, or a drop-in it reads `[Install]` from, could not be
    /// read as text, so whether the section assigns `Also=` is not known
    /// either.
    Unreadable,
}

/// How a link on the source host ties a unit to its parent (§ 6.6 of the
/// units spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkRelation {
    Wants,
    Requires,
    Upholds,
    Alias,
    /// Not a link: `unit` is named in `parent`'s `Also=` (§ 7.4).
    Also,
}

/// Durable systemd unit states that represent administrator intent.
/// Only these three states produce migration-worthy `UnitStateChange` entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitState {
    Enabled,
    Disabled,
    Masked,
}

/// Preset default from systemd `.preset` files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetDefault {
    Enable,
    Disable,
}

/// Derived action for the Containerfile renderer.
/// Not serialized: computed from `current_state` via `implied_action()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitAction {
    Enable,
    Disable,
    Mask,
}

impl std::fmt::Display for UnitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            UnitState::Enabled => "enabled",
            UnitState::Disabled => "disabled",
            UnitState::Masked => "masked",
        };
        write!(f, "{}", s)
    }
}

impl std::fmt::Display for PresetDefault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PresetDefault::Enable => "enable",
            PresetDefault::Disable => "disable",
        };
        write!(f, "{}", s)
    }
}

impl std::fmt::Display for UnitAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            UnitAction::Enable => "enable",
            UnitAction::Disable => "disable",
            UnitAction::Mask => "mask",
        };
        write!(f, "{}", s)
    }
}

/// What the links on the source host say about a unit that `enable` or
/// `disable` on another unit also acts on (§ 7.3 of the units spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberLinks {
    /// Every link its `[Install]` names is present, and it names at least
    /// one. `enable` would add nothing. Links made by hand may sit beside
    /// them.
    Installed,
    /// Its `[Install]` names at least one link and no link under
    /// `/etc/systemd/system` names the unit. `disable` would remove
    /// nothing.
    Unlinked,
    /// It has no link of its own to make (no `[Install]`, only `Also=`, or
    /// no unit file on the source host) and no link names it. Neither verb
    /// changes it.
    Inert,
    /// Anything else: some of its links, only links made by hand, a mask,
    /// or an `[Install]` that could not be read. Neither verb reproduces
    /// the host.
    Other,
}

/// One unit an `enable` or `disable` on the owning row also acts on
/// (§ 6.3 of the units spec).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlsoMember {
    pub unit: UnitName,
    pub links: MemberLinks,
    /// File names of the vendor drop-ins that apply to the member (§ 7.2).
    /// Names only, each one path component ending in `.conf`, checked on
    /// load with the state change that holds the member.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vendor_drop_ins: Vec<String>,
}

/// A unit whose state on the source host differs from what the image would
/// have. The fragment inside `unit` says where the unit file came from and
/// which package owns it.
///
/// Loaded through [`UnitStateChange::new`], which refuses a state a unit of
/// that name and fragment cannot have.
///
/// The unit and its state are read-only, so no caller can pair a template
/// or a unit with no file with a state other than `masked` after the check.
/// A caller that needs another unit or state builds a new value through
/// `new`:
///
/// ```compile_fail,E0616
/// use inspectah_core::types::units::{UnitState, UnitStateChange};
///
/// fn unmask(change: &mut UnitStateChange) {
///     change.current_state = UnitState::Enabled;
/// }
/// ```
///
/// ```compile_fail,E0616
/// use inspectah_core::types::units::{CarriedUnit, UnitStateChange};
///
/// fn swap(change: &mut UnitStateChange, unit: CarriedUnit) {
///     change.unit = unit;
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "UnitStateChangeWire")]
pub struct UnitStateChange {
    unit: CarriedUnit,
    current_state: UnitState,
    #[serde(deserialize_with = "require_explicit_null")]
    pub default_state: Option<PresetDefault>,
    /// Every unit `Also=` reaches from this one, the unit itself left out,
    /// in the order the walk reached them (§ 7.3). Empty for most units.
    /// Recorded only: the plan decides from it (§ 10.2 rule 7).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<AlsoMember>,
    /// Drop-ins under `/etc/systemd/system` that apply to the unit and are
    /// install files: they hold an `[Install]` section or carry a vendor
    /// drop-in's file name. Beside them, a drop-in in a directory systemd
    /// ranks above `/etc/systemd/system` that hides one of them, since the
    /// image reads the hidden one whenever the hiding one is not there,
    /// and, for an enabled instance, a runtime drop-in whose reset,
    /// replacement or hiding of a drop-in of its name removed or changed a
    /// path `required_config` or `data_paths` would hold (§ 7.5), since the
    /// image reads that path whenever the drop-in is not there. Each is
    /// checked as a destination when the plan reads it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub install_drop_ins: Vec<String>,
    /// File names of the vendor drop-ins that apply to the unit for
    /// `[Install]`: the `.conf` files in its own `.d` and, for an instance,
    /// its template's `.d` under `/usr/lib/systemd/system`. Names only,
    /// each one path component ending in `.conf`, checked on load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vendor_drop_ins: Vec<String>,
    /// Paths under `/etc` an enabled template instance reads and cannot
    /// start without, kept whether or not anything exists there (§ 7.5).
    /// Each is an [`is_instance_config_path`], checked on load. Recorded
    /// only: the plan decides from it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_config: Vec<String>,
    /// Paths under `/var` that hold an enabled template instance's
    /// persistent data and exist on the source host (§ 7.5). Each is an
    /// [`is_instance_data_path`], checked on load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_paths: Vec<String>,
    #[serde(default)]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
}

/// The unchecked wire form [`UnitStateChange`] is deserialized through.
#[derive(Deserialize)]
struct UnitStateChangeWire {
    unit: CarriedUnit,
    current_state: UnitState,
    #[serde(deserialize_with = "require_explicit_null")]
    default_state: Option<PresetDefault>,
    #[serde(default)]
    also: Vec<AlsoMember>,
    #[serde(default)]
    install_drop_ins: Vec<String>,
    #[serde(default)]
    vendor_drop_ins: Vec<String>,
    #[serde(default)]
    required_config: Vec<String>,
    #[serde(default)]
    data_paths: Vec<String>,
    #[serde(default)]
    disposition: FindingKind,
    #[serde(default)]
    locked: bool,
    aggregate: Option<AggregatePrevalence>,
    #[serde(default)]
    attention_reason: Option<String>,
}

impl TryFrom<UnitStateChangeWire> for UnitStateChange {
    type Error = UnitStateChangeError;

    fn try_from(wire: UnitStateChangeWire) -> Result<Self, Self::Error> {
        if let Some(name) = wire
            .vendor_drop_ins
            .iter()
            .chain(wire.also.iter().flat_map(|member| &member.vendor_drop_ins))
            .find(|name| !is_drop_in_file_name(name))
        {
            return Err(UnitStateChangeError::VendorDropInName { name: name.clone() });
        }
        if let Some(path) = wire
            .required_config
            .iter()
            .find(|path| !is_instance_config_path(path))
        {
            return Err(UnitStateChangeError::InstanceConfigPath { path: path.clone() });
        }
        if let Some(path) = wire
            .data_paths
            .iter()
            .find(|path| !is_instance_data_path(path))
        {
            return Err(UnitStateChangeError::InstanceDataPath { path: path.clone() });
        }
        let mut change = Self::new(wire.unit, wire.current_state, wire.default_state)?;
        change.also = wire.also;
        change.install_drop_ins = wire.install_drop_ins;
        change.vendor_drop_ins = wire.vendor_drop_ins;
        change.required_config = wire.required_config;
        change.data_paths = wire.data_paths;
        change.disposition = wire.disposition;
        change.locked = wire.locked;
        change.aggregate = wire.aggregate;
        change.attention_reason = wire.attention_reason;
        Ok(change)
    }
}

/// Why a unit cannot have the state a state change gives it. Each variant
/// names the rule that refused it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnitStateChangeError {
    /// A bare template can only be masked: enabling or disabling it means
    /// nothing without an instance.
    #[error(
        "{} is a template and is {state}: a template can be masked and nothing else",
        unit.as_str()
    )]
    TemplateNotMasked { unit: UnitName, state: UnitState },
    /// A unit with no file can only be masked.
    #[error(
        "{} has no unit file and is {state}: a unit with no file can be masked and \
         nothing else",
        unit.as_str()
    )]
    AbsentNotMasked { unit: UnitName, state: UnitState },
    /// A vendor drop-in is recorded by its file name alone: one path
    /// component ending in `.conf`. The rule holds for the state change's
    /// own list and for each `also` member's.
    #[error(
        "vendor_drop_ins holds {name:?}: a vendor drop-in is recorded as one file name \
         ending in .conf"
    )]
    VendorDropInName { name: String },
    /// An instance's configuration is recorded as an absolute path under
    /// `/etc` ([`is_instance_config_path`]).
    #[error(
        "required_config holds {path:?}: an instance's configuration is recorded as an \
         absolute path under /etc"
    )]
    InstanceConfigPath { path: String },
    /// An instance's data is recorded as an absolute path under `/var`,
    /// outside `/var/run` and `/var/lock` ([`is_instance_data_path`]).
    #[error(
        "data_paths holds {path:?}: an instance's data is recorded as an absolute path \
         under /var, outside /var/run and /var/lock"
    )]
    InstanceDataPath { path: String },
}

/// The attention note the collector gives an enabled instance of a
/// template on no list of § 7.5 whose unit file names no path under `/etc`
/// or `/var` for it (Mark Russell, 2026-11-13). It says nothing of
/// carriage: the plan decides that ([`INSTANCE_CARRIED_ALONE`]).
pub const INSTANCE_READS_UNKNOWN: &str = "inspectah does not know what this instance reads: its \
     template is on no list, and its unit file names no path under /etc or /var for it.";

/// What the plan adds after [`INSTANCE_READS_UNKNOWN`] on the refine row
/// once that instance's enable is planned, and only then: an omitted
/// instance keeps its other notes and gets no carriage claim.
pub const INSTANCE_CARRIED_ALONE: &str = "It is carried on its unit file alone.";

/// The directory an instance's required configuration sits under (§ 7.5).
const INSTANCE_CONFIG_ROOT: &str = "/etc";

/// The directory an instance's persistent data sits under (§ 7.5).
const INSTANCE_DATA_ROOT: &str = "/var";

/// The paths under `/var` that resolve into `/run`, which is empty at
/// every boot, so nothing under them is data to migrate (§ 7.5).
pub const RUNTIME_VAR_DIRS: [&str; 2] = ["/var/run", "/var/lock"];

/// Whether `path` is strictly below the directory `root`: absolute, with
/// no empty, `.` or `..` component after the root, and no control
/// character, so the path names what it reads as and puts no line break
/// into a comment the plan writes from it.
fn is_path_below(path: &str, root: &str) -> bool {
    is_below_with(path, root, false)
}

/// [`is_path_below`], with a `..` component after the root allowed when
/// `parent_ok`. Empty and `.` components and control characters are
/// refused either way.
fn is_below_with(path: &str, root: &str, parent_ok: bool) -> bool {
    path.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|rest| {
            !path.chars().any(char::is_control)
                && rest.split('/').all(|part| match part {
                    "" | "." => false,
                    ".." => parent_ok,
                    _ => true,
                })
        })
}

/// Whether `path` can be an instance's `required_config` entry: a path
/// below `/etc` ([`is_path_below`]), except that a `..` component is
/// allowed. systemd reads such a path as written, and dropping it would
/// let rule 6 carry the instance's enable with that path unchecked
/// (§ 10.8). No export entry sits at a path holding `..`, so rule 6
/// declines the enable naming it. The one rule for the form, shared by the
/// collector and the snapshot loader, so the two cannot disagree.
pub fn is_instance_config_path(path: &str) -> bool {
    is_below_with(path, INSTANCE_CONFIG_ROOT, true)
}

/// Whether `path` can be an instance's `data_paths` entry: a path below
/// `/var` ([`is_path_below`]) that is not [`RUNTIME_VAR_DIRS`] or below one
/// of them. The one rule for the form, shared by the collector and the
/// snapshot loader.
pub fn is_instance_data_path(path: &str) -> bool {
    is_path_below(path, INSTANCE_DATA_ROOT)
        && !RUNTIME_VAR_DIRS
            .iter()
            .any(|dir| path == *dir || is_path_below(path, dir))
}

/// The suffix of a drop-in file systemd reads.
const DROP_IN_FILE_SUFFIX: &str = ".conf";

/// Whether `name` is one path component ending in `.conf`.
fn is_drop_in_file_name(name: &str) -> bool {
    !name.contains('/')
        && name.len() > DROP_IN_FILE_SUFFIX.len()
        && name.ends_with(DROP_IN_FILE_SUFFIX)
}

/// The first character of a hidden file name, which systemd's drop-in
/// listing skips.
const HIDDEN_FILE_PREFIX: char = '.';

/// Whether systemd reads a file named `file_name` in a drop-in directory:
/// a `.conf` name that is not hidden. systemd skips a name starting with
/// `.` when it lists drop-ins, so an editor's `.#override.conf` is never
/// read; that rule was read from systemd's source and not measured on RHEL
/// 9 or 10. The one name rule for drop-ins, shared by the collector's
/// listing and the plan's install-file checks, so the two cannot disagree.
pub fn systemd_reads_drop_in(file_name: &str) -> bool {
    !file_name.starts_with(HIDDEN_FILE_PREFIX) && file_name.ends_with(DROP_IN_FILE_SUFFIX)
}

/// The header of the section `systemctl enable` reads.
const INSTALL_SECTION_HEADER: &str = "[Install]";

/// The characters systemd's unit file parser strips from the ends of a
/// line, a key and a value.
pub const UNIT_FILE_WHITESPACE: &[char] = &[' ', '\t', '\n', '\r'];

/// The characters that start a comment line in a unit file.
const UNIT_FILE_COMMENT_STARTS: &[char] = &['#', ';'];

/// The characters that end a line in a unit file, as systemd reads it.
const UNIT_FILE_LINE_ENDS: &[char] = &['\n', '\r', '\0'];

/// The lines of a unit file as systemd's `read_line` splits them: `\n`,
/// `\r` and `\0` each end a line, and a run of them is one line end while
/// no character repeats in it and no `\0` came before, so `\r\n` and
/// `\n\r` are one line end and `\n\n` is two.
fn physical_lines(content: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut rest = content;
    while !rest.is_empty() {
        let end = rest.find(UNIT_FILE_LINE_ENDS).unwrap_or(rest.len());
        lines.push(&rest[..end]);
        rest = &rest[end..];
        let mut seen = Vec::new();
        while let Some(c) = rest
            .chars()
            .next()
            .filter(|c| UNIT_FILE_LINE_ENDS.contains(c) && !seen.contains(c))
        {
            rest = &rest[c.len_utf8()..];
            seen.push(c);
            if c == '\0' {
                break;
            }
        }
    }
    lines
}

/// The logical lines of a unit file, joined as systemd's config parser
/// joins them (systemd.syntax(7)): a line that ends in a backslash which
/// is not itself escaped continues onto the next, the backslash read as a
/// space. A comment line is skipped wherever it is, inside a continuation
/// too, so it neither ends nor continues one. A continuation still open at
/// the end of the file is a line of its own.
pub fn logical_lines(content: &str) -> Vec<String> {
    numbered_logical_lines(content)
        .into_iter()
        .map(|(_, line)| line)
        .collect()
}

/// [`logical_lines`], each with the number of the physical line it starts
/// on, counted from 1 over every physical line systemd's `read_line` ends
/// ([`physical_lines`]), comments included: the line a reader of the file
/// finds the setting on.
pub fn numbered_logical_lines(content: &str) -> Vec<(usize, String)> {
    let mut lines = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (index, line) in physical_lines(content).into_iter().enumerate() {
        if line
            .trim_start_matches(UNIT_FILE_WHITESPACE)
            .starts_with(UNIT_FILE_COMMENT_STARTS)
        {
            continue;
        }
        let (number, mut joined) = pending.take().unwrap_or((index + 1, String::new()));
        joined.push_str(line);
        let trailing_backslashes = line.len() - line.trim_end_matches('\\').len();
        if trailing_backslashes % 2 == 1 {
            joined.pop();
            joined.push(' ');
            pending = Some((number, joined));
        } else {
            lines.push((number, joined));
        }
    }
    lines.extend(pending);
    lines
}

/// Whether a unit file or drop-in holds an `[Install]` section: some
/// logical line ([`logical_lines`]), trimmed of the whitespace systemd
/// strips, is `[Install]`. The keys are not read. The one lexical test of
/// an install file, shared by the collector and the plan.
pub fn holds_install_section(text: &str) -> bool {
    logical_lines(text)
        .iter()
        .any(|line| line.trim_matches(UNIT_FILE_WHITESPACE) == INSTALL_SECTION_HEADER)
}

impl UnitStateChange {
    /// A state change for `unit`, included, unlocked and with no aggregate.
    /// Refused when the unit is a template or has no file and the state is
    /// not `Masked`.
    pub fn new(
        unit: CarriedUnit,
        current_state: UnitState,
        default_state: Option<PresetDefault>,
    ) -> Result<Self, UnitStateChangeError> {
        if current_state != UnitState::Masked {
            let name = unit.name();
            if name.is_template() {
                return Err(UnitStateChangeError::TemplateNotMasked {
                    unit: name.clone(),
                    state: current_state,
                });
            }
            if let CarriedUnit::Ordinary(ordinary) = &unit
                && *ordinary.fragment() == UnitFragment::Absent
            {
                return Err(UnitStateChangeError::AbsentNotMasked {
                    unit: name.clone(),
                    state: current_state,
                });
            }
        }
        Ok(Self {
            unit,
            current_state,
            default_state,
            also: Vec::new(),
            install_drop_ins: Vec::new(),
            vendor_drop_ins: Vec::new(),
            required_config: Vec::new(),
            data_paths: Vec::new(),
            disposition: FindingKind::included(),
            locked: false,
            aggregate: None,
            attention_reason: None,
        })
    }

    /// The unit, with the fragment it loads from and its owning package.
    pub fn unit(&self) -> &CarriedUnit {
        &self.unit
    }

    /// The unit's state on the source host.
    pub fn current_state(&self) -> UnitState {
        self.current_state
    }
}

fn require_explicit_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

impl UnitStateChange {
    /// Derive the Containerfile action from the current unit state.
    pub fn implied_action(&self) -> UnitAction {
        match self.current_state {
            UnitState::Enabled => UnitAction::Enable,
            UnitState::Disabled => UnitAction::Disable,
            UnitState::Masked => UnitAction::Mask,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemdDropIn {
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default)]
    pub variant_selection: VariantSelection,
    pub aggregate: Option<AggregatePrevalence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_type: Option<ShadowType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_rationale: Option<String>,
}

/// One unit's enablement on the source host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitInventoryEntry {
    pub unit: UnitName,
    pub state: UnitState,
    /// The preset default for the unit, when a preset rule names it.
    /// Resolved only for a unit that can carry a state change.
    pub preset: Option<PresetDefault>,
}

impl UnitInventoryEntry {
    /// True when the unit is enabled or disabled as its preset says, so
    /// the image already has this state.
    pub fn matches_preset(&self) -> bool {
        matches!(
            (self.state, self.preset),
            (UnitState::Enabled, Some(PresetDefault::Enable))
                | (UnitState::Disabled, Some(PresetDefault::Disable))
        )
    }
}

/// An administrator's mount, automount or swap unit: reported, never acted
/// on. It holds no `What=`, which can carry a credential; refine links the
/// row to the unit file's config entry instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MountUnitFinding {
    pub unit: AdvisoryUnit,
    /// Present when the unit is enabled or masked.
    pub state_change: Option<(UnitState, Option<PresetDefault>)>,
    /// `Where=` for a mount or automount, when it is an absolute path.
    pub mount_point: Option<String>,
    /// Whether `Options=` holds `nofail`, when the collector read the unit
    /// file systemd loads for the unit and parsed its `Options=` (a mount
    /// or swap unit). `None` when it did not: a mask with no unit file, a
    /// file that is not text or is over the size cap, a read error, or an
    /// automount, which has no `Options=`. A missing or null value reads as
    /// `None`.
    #[serde(default)]
    pub nofail: Option<bool>,
    pub aggregate: Option<AggregatePrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnitSection {
    /// Units whose state differs from what the image would have.
    pub state_changes: Vec<UnitStateChange>,
    /// Every unit with a durable state, changed or not, once each.
    pub inventory: Vec<UnitInventoryEntry>,
    pub drop_ins: Vec<SystemdDropIn>,
    /// The administrator's mount, automount and swap units.
    pub mount_findings: Vec<MountUnitFinding>,
    /// Links inspectah reports and does not carry (§ 6.6 of the units
    /// spec).
    pub manual_links: Vec<ManualLink>,
    /// Units whose `[Install]` cannot be relied on, where the image's
    /// links can differ from the host's with nothing else to say so
    /// (§ 6.6, § 7.3).
    pub undetermined_installs: Vec<UndeterminedInstall>,
    /// The default target, when the host sets one (§ 7.6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_target: Option<DefaultTarget>,
}

/// The target `/etc/systemd/system/default.target` links to on the source
/// host (§ 7.6 of the units spec). Not an enable and not compared with a
/// preset: the export sets it as the image's default whatever it is
/// (§ 10.5). Recorded as included.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DefaultTarget {
    pub target: TargetUnit,
    #[serde(default)]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
}

/// Why a link on the source host is reported and not carried (§ 6.6 of
/// the units spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualLinkCause {
    /// The linked unit has no link of its own to make: no `[Install]`
    /// section, or one that names no link (only `Also=`, or lists a
    /// drop-in reset to empty).
    StaticUnit,
    /// The unit's `[Install]` section does not name this parent for this
    /// relation.
    NotInInstall,
    /// The section names this link and others that are absent, so neither
    /// `enable` nor `disable` reproduces the host.
    PartialInstall,
    /// The `[Install]` section cannot be relied on, one that holds
    /// `UpheldBy=` included. The reason is kept so the advisory can name
    /// it.
    Undetermined { reason: InstallUnknown },
    /// An instance of a template tied to one host's devices (§ 7.5).
    HostDevice,
    /// The unit's `/etc` files make this link, and the package's
    /// `[Install]` alone does not: the image lacks it (§ 7.4).
    HostInstallOnly,
    /// The package's `[Install]` alone makes this link, and the unit's
    /// `/etc` files do not: the image has it and the host did not (§ 7.4).
    VendorInstallOnly,
}

/// A link on the source host that inspectah reports and does not carry
/// (§ 6.6 of the units spec). It holds no carried unit and no decision:
/// nothing turns it into a command, and it has no toggle. The two
/// install-difference causes are the one case where it describes something
/// other than a link on the host: a difference between the links the host's
/// files make and the links the package's files make.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManualLink {
    pub relation: LinkRelation,
    /// The unit whose `.wants`, `.requires` or `.upholds` directory holds
    /// the link. For an alias, the alias name.
    pub parent: UnitName,
    pub unit: UnitName,
    pub cause: ManualLinkCause,
    /// For the two install-difference causes: the `/etc` install files
    /// that produce the difference (§ 7.4). They are the files of the unit
    /// whose `[Install]` was compared: `unit` for a link, `parent` for
    /// relation `Also`. Empty otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub install_files: Vec<String>,
    pub aggregate: Option<AggregatePrevalence>,
}

/// A unit whose `[Install]` on the source host is `Undetermined` and whose
/// `/etc` install files change it, or that assigns `Also=`, so its members
/// cannot be listed or checked (§ 6.6, § 7.3 of the units spec). The
/// section is the effective one or, for a unit at its `enable` preset whose
/// `/etc` files change it, the one its package alone provides (§ 7.4; Mark
/// Russell, 2026-11-15). One per unit, whether or not the unit has a link,
/// since the image's links can differ from the host's with no link on
/// either side to report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UndeterminedInstall {
    pub unit: UnitName,
    pub reason: InstallUnknown,
    /// The unit's `/etc` install files: its unit file or shadow, and its
    /// install drop-ins. Empty when it has none.
    pub install_files: Vec<String>,
    pub aggregate: Option<AggregatePrevalence>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The half of the redaction field guard
    /// (`every_field_of_a_content_bearing_type_is_classified` in
    /// `crates/pipeline/src/render/mod.rs`) that needs this module's
    /// privacy: [`UnitStateChange`] keeps its unit and state private, so
    /// only code here can name every field. The pattern has no `..`, so a
    /// field added to the type stops the build here until it is classified.
    #[test]
    fn every_field_of_a_unit_state_change_is_classified() {
        fn _classify_unit_state_change(change: UnitStateChange) {
            let UnitStateChange {
                unit: _,             // exempt: a proved name, fragment and package name
                current_state: _,    // exempt: enabled, disabled or masked
                default_state: _,    // exempt: enable or disable
                also: _,             // names and links: classified in the pipeline guard
                install_drop_ins: _, // identifiers: drop-in paths, no content
                vendor_drop_ins: _,  // identifiers: file names, no content
                required_config: _,  // identifiers: paths under /etc, no content
                data_paths: _,       // identifiers: paths under /var, no content
                disposition: _,      // decision
                locked: _,           // decision
                aggregate: _,        // fleet counts
                attention_reason: _, // generated prose, never file content
            } = change;
        }
    }

    /// The nested half of the redaction field guard: the outer patterns
    /// (above, and in `crates/pipeline/src/render/mod.rs`) classify a row's
    /// `unit` whole, so every type a `unit` field holds is destructured
    /// here, where its private fields can be named: each enum matched with
    /// no wildcard arm, each struct with no `..`. A field or variant added
    /// to any of them stops the build here until it is classified.
    #[test]
    fn every_field_of_a_nested_unit_type_is_classified() {
        fn _classify_carried_unit(unit: CarriedUnit) {
            match unit {
                CarriedUnit::Ordinary(OrdinaryUnit {
                    name: _,     // exempt: a proved unit name
                    fragment: _, // classified in `_classify_unit_fragment`
                }) => {}
                CarriedUnit::PackagedStorage(PackagedStorageUnit {
                    name: _,           // exempt: a proved unit name
                    owning_package: _, // exempt: an rpm package name
                }) => {}
            }
        }

        fn _classify_advisory_unit(unit: AdvisoryUnit) {
            let AdvisoryUnit {
                name: _, // exempt: a proved unit name
                fragment,
            } = unit;
            match fragment {
                AdminStorageFragment::Local => {}
                AdminStorageFragment::Shadowed {
                    owning_package: _, // exempt: an rpm package name
                } => {}
                AdminStorageFragment::UnownedVendor => {}
                AdminStorageFragment::MaskOnly => {}
            }
        }

        fn _classify_target_unit(unit: TargetUnit) {
            let TargetUnit {
                name: _,     // exempt: a proved unit name
                fragment: _, // classified in `_classify_unit_fragment`
            } = unit;
        }

        fn _classify_unit_subject(subject: UnitSubject) {
            let UnitSubject {
                name: _,     // exempt: a proved unit name
                fragment: _, // classified in `_classify_unit_fragment`
            } = subject;
        }

        fn _classify_unit_fragment(fragment: UnitFragment) {
            match fragment {
                UnitFragment::Vendor {
                    file: _,           // exempt: a proved unit name
                    owning_package: _, // exempt: an rpm package name
                } => {}
                UnitFragment::Local {
                    file: _, // exempt: a proved unit name
                } => {}
                UnitFragment::Shadowed {
                    file: _,           // exempt: a proved unit name
                    owning_package: _, // exempt: an rpm package name
                } => {}
                UnitFragment::Absent => {}
            }
        }

        fn _classify_unit_names(
            name: UnitName,
            ordinary: OrdinaryUnitName,
            storage: StorageUnitName,
            target: TargetUnitName,
        ) {
            let UnitName(_) = name; // exempt: a proved unit name
            let OrdinaryUnitName(_) = ordinary; // exempt: a proved unit name
            let StorageUnitName(_, _) = storage; // exempt: a proved unit name and its kind
            let TargetUnitName(_) = target; // exempt: a proved unit name
        }
    }

    /// Expected outcome of parsing one name in the table test.
    enum Expect {
        Kind(UnitKind),
        Fails(&'static str),
    }

    #[test]
    fn unit_name_parse_table() {
        let max_len = format!("{}.service", "a".repeat(255 - ".service".len()));
        let over_len = format!("{}.service", "a".repeat(256 - ".service".len()));
        assert_eq!(max_len.len(), 255);
        assert_eq!(over_len.len(), 256);

        use Expect::{Fails, Kind};
        let cases: Vec<(String, Expect)> = vec![
            // Every suffix of § 5 that has a variant.
            (
                "sshd.service".into(),
                Kind(UnitKind::Ordinary(OrdinaryKind::Service)),
            ),
            (
                "cockpit.socket".into(),
                Kind(UnitKind::Ordinary(OrdinaryKind::Socket)),
            ),
            (
                "cups.path".into(),
                Kind(UnitKind::Ordinary(OrdinaryKind::Path)),
            ),
            (
                "fstrim.timer".into(),
                Kind(UnitKind::Ordinary(OrdinaryKind::Timer)),
            ),
            (
                "multi-user.target".into(),
                Kind(UnitKind::Ordinary(OrdinaryKind::Target)),
            ),
            (
                "user.slice".into(),
                Kind(UnitKind::Ordinary(OrdinaryKind::Slice)),
            ),
            (
                "tmp.mount".into(),
                Kind(UnitKind::Storage(StorageKind::Mount)),
            ),
            (
                "proc-sys-fs-binfmt_misc.automount".into(),
                Kind(UnitKind::Storage(StorageKind::Automount)),
            ),
            (
                "dev-zram0.swap".into(),
                Kind(UnitKind::Storage(StorageKind::Swap)),
            ),
            // Excluded kinds and names with no known suffix.
            ("dev-sda.device".into(), Fails("suffix")),
            ("session-1.scope".into(), Fails("suffix")),
            ("sshd".into(), Fails("suffix")),
            ("sshd.Service".into(), Fails("suffix")),
            ("sshd.service.d".into(), Fails("suffix")),
            // Length bounds, in bytes.
            (String::new(), Fails("empty")),
            (max_len, Kind(UnitKind::Ordinary(OrdinaryKind::Service))),
            (over_len, Fails("255")),
            // Bytes outside the systemd.unit(5) set.
            ("foo'bar.service".into(), Fails("character")),
            ("foo\"bar.service".into(), Fails("character")),
            ("foo bar.service".into(), Fails("character")),
            ("foo\nbar.service".into(), Fails("character")),
            ("foo\tbar.service".into(), Fails("character")),
            ("foo/bar.service".into(), Fails("character")),
            ("foo*.service".into(), Fails("character")),
            ("foo\u{e9}.service".into(), Fails("character")),
            ("foo\u{7f}.service".into(), Fails("character")),
            // Escaped and odd names systemd itself produces.
            (
                r"foo\x2dbar.service".into(),
                Kind(UnitKind::Ordinary(OrdinaryKind::Service)),
            ),
            (
                "-.slice".into(),
                Kind(UnitKind::Ordinary(OrdinaryKind::Slice)),
            ),
            (
                "-.mount".into(),
                Kind(UnitKind::Storage(StorageKind::Mount)),
            ),
            (
                r"systemd-cryptsetup@luks\x2d1234.service".into(),
                Kind(UnitKind::Ordinary(OrdinaryKind::Service)),
            ),
            (
                "dbus-org.freedesktop.nm-dispatcher.service".into(),
                Kind(UnitKind::Ordinary(OrdinaryKind::Service)),
            ),
            (
                "a:b_c.socket".into(),
                Kind(UnitKind::Ordinary(OrdinaryKind::Socket)),
            ),
        ];

        for (input, expect) in cases {
            let result = UnitName::try_from(input.clone());
            match (result, expect) {
                (Ok(name), Kind(kind)) => {
                    assert_eq!(name.kind(), kind, "kind of {input:?}");
                    assert_eq!(name.as_str(), input);
                }
                (Err(err), Fails(rule)) => {
                    let text = err.to_string();
                    assert!(
                        text.contains(rule),
                        "error for {input:?} should name the {rule:?} rule, got: {text}"
                    );
                }
                (Ok(_), Fails(rule)) => {
                    panic!("{input:?} parsed, expected the {rule:?} rule to fail")
                }
                (Err(err), Kind(_)) => panic!("{input:?} failed: {err}"),
            }
        }
    }

    #[test]
    fn character_error_names_the_character() {
        let err = UnitName::try_from("foo\nbar.service".to_string()).unwrap_err();
        assert_eq!(err, UnitNameError::InvalidCharacter { ch: '\n' });
        assert!(err.to_string().contains(r"'\n'"), "{err}");
    }

    #[test]
    fn instance_and_template() {
        let instance = UnitName::try_from("getty@tty1.service".to_string()).unwrap();
        assert!(!instance.is_template());
        let parts = instance
            .instance()
            .expect("getty@tty1.service is an instance");
        assert_eq!(parts.prefix(), "getty");
        assert_eq!(parts.instance(), "tty1");
        let template = parts.template();
        assert_eq!(template.as_str(), "getty@.service");
        assert_eq!(template.kind(), UnitKind::Ordinary(OrdinaryKind::Service));
        assert!(template.is_template());

        let template = UnitName::try_from("getty@.service".to_string()).unwrap();
        assert!(template.is_template());
        assert!(template.instance().is_none());

        let plain = UnitName::try_from("sshd.service".to_string()).unwrap();
        assert!(!plain.is_template());
        assert!(plain.instance().is_none());

        // The instance part may itself hold dots and escapes; the suffix is
        // the text after the last dot.
        let escaped = UnitName::try_from(r"wg-quick@wg0.conf\x2dx.service".to_string()).unwrap();
        let parts = escaped.instance().unwrap();
        assert_eq!(parts.prefix(), "wg-quick");
        assert_eq!(parts.instance(), r"wg0.conf\x2dx");
        assert_eq!(parts.template().as_str(), "wg-quick@.service");
    }

    #[test]
    fn serde_round_trip_and_rejection() {
        let name: UnitName = serde_json::from_str(r#""cockpit.socket""#).unwrap();
        assert_eq!(name.kind(), UnitKind::Ordinary(OrdinaryKind::Socket));
        assert_eq!(serde_json::to_string(&name).unwrap(), r#""cockpit.socket""#);

        let expected = UnitName::try_from("foo bar.service".to_string())
            .unwrap_err()
            .to_string();
        let err = serde_json::from_str::<UnitName>(r#""foo bar.service""#).unwrap_err();
        assert!(
            err.to_string().contains(&expected),
            "serde error {err} should carry {expected}"
        );
    }

    /// A proved unit name for a test literal.
    fn unit_name(name: &str) -> UnitName {
        UnitName::try_from(name.to_owned()).expect("test unit names are valid")
    }

    /// A carried unit from the vendor directory, owned as given.
    fn test_carried_unit(name: UnitName, owning_package: Option<String>) -> CarriedUnit {
        let fragment = UnitFragment::Vendor {
            file: name.clone(),
            owning_package,
        };
        CarriedUnit::try_from(UnitSubject::new(name, fragment).expect("file is the unit"))
            .expect("a test unit is carried")
    }

    #[test]
    fn test_service_state_change_roundtrip_uses_typed_enums() {
        let section = UnitSection {
            state_changes: vec![
                UnitStateChange::new(
                    test_carried_unit(unit_name("firewalld.service"), Some("firewalld".into())),
                    UnitState::Enabled,
                    Some(PresetDefault::Disable),
                )
                .unwrap(),
                UnitStateChange::new(
                    test_carried_unit(unit_name("cups.service"), Some("cups".into())),
                    UnitState::Masked,
                    None,
                )
                .unwrap(),
            ],
            inventory: vec![UnitInventoryEntry {
                unit: unit_name("firewalld.service"),
                state: UnitState::Enabled,
                preset: Some(PresetDefault::Disable),
            }],
            drop_ins: vec![],
            mount_findings: vec![],
            manual_links: Vec::new(),
            undetermined_installs: Vec::new(),
            default_target: None,
        };

        let json = serde_json::to_string(&section).unwrap();
        let parsed: UnitSection = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, section);
    }

    #[test]
    fn test_implied_action_derives_from_current_state() {
        let enabled = UnitStateChange::new(
            test_carried_unit(unit_name("firewalld.service"), Some("firewalld".into())),
            UnitState::Enabled,
            Some(PresetDefault::Disable),
        )
        .unwrap();
        let disabled = UnitStateChange::new(
            test_carried_unit(unit_name("sshd.service"), Some("openssh-server".into())),
            UnitState::Disabled,
            Some(PresetDefault::Enable),
        )
        .unwrap();
        let masked = UnitStateChange::new(
            test_carried_unit(unit_name("cups.service"), Some("cups".into())),
            UnitState::Masked,
            None,
        )
        .unwrap();

        assert_eq!(enabled.implied_action(), UnitAction::Enable);
        assert_eq!(disabled.implied_action(), UnitAction::Disable);
        assert_eq!(masked.implied_action(), UnitAction::Mask);
    }

    #[test]
    fn test_option_preset_default_serde_roundtrip() {
        let with_preset = UnitStateChange::new(
            test_carried_unit(unit_name("firewalld.service"), Some("firewalld".into())),
            UnitState::Enabled,
            Some(PresetDefault::Disable),
        )
        .unwrap();
        let json = serde_json::to_string(&with_preset).unwrap();
        let parsed: UnitStateChange = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.default_state, Some(PresetDefault::Disable));

        let without_preset = UnitStateChange::new(
            test_carried_unit(unit_name("cups.service"), Some("cups".into())),
            UnitState::Masked,
            None,
        )
        .unwrap();
        let json = serde_json::to_string(&without_preset).unwrap();
        let parsed: UnitStateChange = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.default_state, None);

        let null_json = r#"{
            "unit":{"name":"cups.service","fragment":"vendor","file":"cups.service","owning_package":"cups"},
            "current_state":"masked",
            "default_state":null,
            "include":true,
            "aggregate":null
        }"#;
        let parsed: UnitStateChange = serde_json::from_str(null_json).unwrap();
        assert_eq!(parsed.default_state, None);
    }

    #[test]
    fn test_inventory_roundtrip_and_preset_match() {
        let section = UnitSection {
            state_changes: Vec::new(),
            inventory: vec![
                UnitInventoryEntry {
                    unit: unit_name("sshd.service"),
                    state: UnitState::Enabled,
                    preset: None,
                },
                UnitInventoryEntry {
                    unit: unit_name("chronyd.service"),
                    state: UnitState::Enabled,
                    preset: Some(PresetDefault::Enable),
                },
                UnitInventoryEntry {
                    unit: unit_name("cups.service"),
                    state: UnitState::Disabled,
                    preset: Some(PresetDefault::Disable),
                },
                UnitInventoryEntry {
                    unit: unit_name("gdm.service"),
                    state: UnitState::Masked,
                    preset: Some(PresetDefault::Disable),
                },
            ],
            drop_ins: Vec::new(),
            mount_findings: Vec::new(),
            manual_links: Vec::new(),
            undetermined_installs: Vec::new(),
            default_target: None,
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: UnitSection = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, section);

        let matched: Vec<&str> = parsed
            .inventory
            .iter()
            .filter(|e| e.matches_preset())
            .map(|e| e.unit.as_str())
            .collect();
        assert_eq!(matched, vec!["chronyd.service", "cups.service"]);
    }

    #[test]
    fn test_missing_field_fails_deserialization() {
        // A missing inventory must fail, not silently backfill. The same
        // payload with the field loads.
        let with_field = r#"{"state_changes":[],"inventory":[],"drop_ins":[],"mount_findings":[],"manual_links":[],"undetermined_installs":[]}"#;
        serde_json::from_str::<UnitSection>(with_field).expect("complete section loads");

        let json = r#"{"state_changes":[],"drop_ins":[],"mount_findings":[],"manual_links":[],"undetermined_installs":[]}"#;
        let err = serde_json::from_str::<UnitSection>(json).unwrap_err();
        assert!(
            err.to_string().contains("inventory"),
            "expected missing-field error for inventory, got: {err}"
        );
    }

    #[test]
    fn test_missing_default_state_does_not_deserialize() {
        let json = r#"{
            "unit":{"name":"firewalld.service","fragment":"vendor","file":"firewalld.service","owning_package":"firewalld"},
            "current_state":"enabled",
            "include":true,
            "aggregate":null
        }"#;

        let err = serde_json::from_str::<UnitStateChange>(json).unwrap_err();
        assert!(
            err.to_string().contains("default_state"),
            "expected missing-field error, got: {err}"
        );
    }

    // -- Serde backward-compat contract tests ---------------------------------
    // Verify that JSON without `include` deserializes with include=true.

    #[test]
    fn service_without_include_deserializes_as_true() {
        let json = r#"{"unit":{"name":"test.service","fragment":"vendor","file":"test.service","owning_package":null},"current_state":"enabled","default_state":null}"#;
        let sc: UnitStateChange = serde_json::from_str(json).unwrap();
        assert!(
            sc.disposition.is_included(),
            "missing include field should deserialize as true"
        );
    }

    #[test]
    fn dropin_without_include_deserializes_as_true() {
        let json = r#"{"unit":"test.service","path":"/etc/systemd/system/test.service.d/override.conf","content":"[Service]\nRestart=always"}"#;
        let di: SystemdDropIn = serde_json::from_str(json).unwrap();
        assert!(
            di.disposition.is_included(),
            "missing include field should deserialize as true"
        );
    }

    #[test]
    fn service_state_change_without_shadow_deserializes() {
        let json = r#"{"unit":{"name":"sshd.service","fragment":"vendor","file":"sshd.service","owning_package":null},"current_state":"enabled","default_state":null,"disposition":{"kind":"actionable","include":true}}"#;
        let ssc: UnitStateChange = serde_json::from_str(json).unwrap();
        assert_eq!(ssc.current_state, UnitState::Enabled);
    }

    // -- Carried and advisory units (T2, on the types) ------------------------

    /// One name of each storage kind.
    const STORAGE_NAMES: &[&str] = &[
        "tmp.mount",
        "proc-sys-fs-binfmt_misc.automount",
        "dev-zram0.swap",
    ];

    /// The wire form of a unit subject.
    fn subject_json(name: &str, fragment: &str, owner: Option<&str>) -> serde_json::Value {
        let mut value = serde_json::json!({"name": name, "fragment": fragment});
        if fragment != "absent" {
            value["file"] = name.into();
        }
        if matches!(fragment, "vendor" | "shadowed") {
            value["owning_package"] = owner.into();
        }
        value
    }

    #[test]
    fn carried_unit_refuses_an_administrators_storage_unit() {
        let refused: &[(&str, Option<&str>)] = &[
            ("local", None),
            ("shadowed", Some("systemd")),
            ("shadowed", None),
            ("absent", None),
            ("vendor", None),
        ];
        for name in STORAGE_NAMES {
            // Positive control: the same name, vendor with an owner, loads.
            let owned = subject_json(name, "vendor", Some("systemd"));
            let carried: CarriedUnit = serde_json::from_value(owned.clone())
                .unwrap_or_else(|err| panic!("{name} vendor with an owner: {err}"));
            match &carried {
                CarriedUnit::PackagedStorage(unit) => {
                    assert_eq!(unit.name().as_unit_name().as_str(), *name);
                    assert_eq!(unit.owning_package(), "systemd");
                }
                other => panic!("{name} classified as {other:?}"),
            }
            assert_eq!(serde_json::to_value(&carried).unwrap(), owned);

            for (fragment, owner) in refused {
                let value = subject_json(name, fragment, *owner);
                let err = serde_json::from_value::<CarriedUnit>(value)
                    .expect_err("an administrator's storage unit must not be carried")
                    .to_string();
                assert!(
                    err.contains(&format!("classify places {name}")) && err.contains("advisory"),
                    "{name} {fragment}: error must name classify: {err}"
                );
            }
        }
    }

    #[test]
    fn advisory_unit_refuses_a_packaged_storage_unit() {
        for name in STORAGE_NAMES {
            // Positive control: the same name with a local fragment is advisory.
            let local = subject_json(name, "local", None);
            let advisory: AdvisoryUnit = serde_json::from_value(local.clone()).unwrap();
            assert_eq!(advisory.fragment(), &AdminStorageFragment::Local);
            assert_eq!(serde_json::to_value(&advisory).unwrap(), local);

            let owned = subject_json(name, "vendor", Some("systemd"));
            let err = serde_json::from_value::<AdvisoryUnit>(owned)
                .expect_err("a package-owned storage unit is not advisory")
                .to_string();
            assert!(
                err.contains(&format!("classify places {name} as carried")),
                "{name}: error must name classify: {err}"
            );
        }
    }

    #[test]
    fn ordinary_unit_is_carried_with_any_fragment() {
        for (fragment, owner) in [
            ("local", None),
            ("shadowed", None),
            ("absent", None),
            ("vendor", None),
        ] {
            let value = subject_json("sshd.service", fragment, owner);
            let carried: CarriedUnit = serde_json::from_value(value.clone())
                .unwrap_or_else(|err| panic!("sshd.service {fragment}: {err}"));
            assert!(matches!(carried, CarriedUnit::Ordinary(_)));
            assert_eq!(serde_json::to_value(&carried).unwrap(), value);
        }
    }

    #[test]
    fn target_unit_takes_a_target_only() {
        let target = subject_json("multi-user.target", "vendor", Some("systemd"));
        let parsed: TargetUnit = serde_json::from_value(target.clone()).expect("a target loads");
        assert_eq!(parsed.name().as_unit_name().as_str(), "multi-user.target");
        assert_eq!(serde_json::to_value(&parsed).unwrap(), target);

        for value in [
            subject_json("cockpit.socket", "vendor", Some("cockpit-ws")),
            subject_json("tmp.mount", "vendor", Some("systemd")),
        ] {
            let name = value["name"].as_str().unwrap().to_owned();
            let err = serde_json::from_value::<TargetUnit>(value)
                .expect_err("set-default takes a target only")
                .to_string();
            assert!(
                err.contains(&format!("{name} is not a .target")),
                "{name}: error must name the target rule: {err}"
            );
        }
    }

    #[test]
    fn subject_file_must_be_the_unit_or_its_template() {
        // Positive controls: the unit's own file, and an instance's template.
        for (name, file) in [
            ("sshd.service", "sshd.service"),
            ("getty@tty1.service", "getty@.service"),
            ("getty@tty1.service", "getty@tty1.service"),
        ] {
            let value = serde_json::json!({"name": name, "fragment": "local", "file": file});
            serde_json::from_value::<CarriedUnit>(value)
                .unwrap_or_else(|err| panic!("{name} with file {file}: {err}"));
        }

        for (name, file) in [
            ("sshd.service", "httpd.service"),
            ("getty@tty1.service", "serial-getty@.service"),
            ("getty@.service", "getty@tty1.service"),
            ("cockpit.socket", "cockpit.service"),
        ] {
            let value = serde_json::json!({
                "name": name, "fragment": "vendor", "file": file, "owning_package": "systemd"
            });
            let expected = UnitSubjectError::FileNotUnitOrTemplate {
                name: unit_name(name),
                file: unit_name(file),
            }
            .to_string();
            let err = serde_json::from_value::<CarriedUnit>(value)
                .expect_err("a file that is neither the unit nor its template fails")
                .to_string();
            assert!(err.contains(&expected), "{name}/{file}: {err}");
        }
    }

    #[test]
    fn state_change_constructor_refuses_unmasked_template_and_absent_unit() {
        let carried = |name: &str, fragment: UnitFragment| {
            CarriedUnit::try_from(UnitSubject::new(unit_name(name), fragment).unwrap()).unwrap()
        };
        let template = || test_carried_unit(unit_name("getty@.service"), Some("systemd".into()));
        let absent = || carried("gone.service", UnitFragment::Absent);

        // Positive controls: both are fine masked, and an ordinary vendor
        // unit is fine in every state.
        UnitStateChange::new(template(), UnitState::Masked, None).expect("a masked template");
        UnitStateChange::new(absent(), UnitState::Masked, None).expect("a masked absent unit");
        for state in [UnitState::Enabled, UnitState::Disabled, UnitState::Masked] {
            UnitStateChange::new(
                test_carried_unit(unit_name("sshd.service"), None),
                state,
                None,
            )
            .expect("an ordinary vendor unit takes any state");
        }

        for state in [UnitState::Enabled, UnitState::Disabled] {
            assert_eq!(
                UnitStateChange::new(template(), state, None).unwrap_err(),
                UnitStateChangeError::TemplateNotMasked {
                    unit: unit_name("getty@.service"),
                    state,
                }
            );
            assert_eq!(
                UnitStateChange::new(absent(), state, None).unwrap_err(),
                UnitStateChangeError::AbsentNotMasked {
                    unit: unit_name("gone.service"),
                    state,
                }
            );
        }
    }

    #[test]
    fn storage_subject_file_must_be_the_unit_itself() {
        for kind in ["mount", "automount", "swap"] {
            let name = format!("foo@bar.{kind}");
            // Positive control, on the refusal's route: the same vendor unit
            // with an owner and its own file loads as a carried unit.
            let own = serde_json::json!({
                "name": name, "fragment": "vendor", "file": name,
                "owning_package": "systemd"
            });
            serde_json::from_value::<CarriedUnit>(own)
                .unwrap_or_else(|err| panic!("{name} with its own file: {err}"));

            let template = format!("foo@.{kind}");
            let value = serde_json::json!({
                "name": name, "fragment": "vendor", "file": template,
                "owning_package": "systemd"
            });
            let expected = UnitSubjectError::StorageFileNotUnit {
                name: unit_name(&name),
                file: unit_name(&template),
            }
            .to_string();
            let err = serde_json::from_value::<CarriedUnit>(value)
                .expect_err("a storage unit's file is its own name")
                .to_string();
            assert!(err.contains(&expected), "{name}: {err}");
            assert!(
                err.contains("template"),
                "{name}: error names the rule: {err}"
            );
        }
    }

    #[test]
    fn empty_owning_package_is_refused() {
        for (name, fragment) in [
            ("tmp.mount", "vendor"),
            ("tmp.mount", "shadowed"),
            ("sshd.service", "vendor"),
            ("sshd.service", "shadowed"),
        ] {
            // Positive control: a named owner loads.
            let owned = subject_json(name, fragment, Some("systemd"));
            serde_json::from_value::<UnitSubject>(owned)
                .unwrap_or_else(|err| panic!("{name} {fragment} with an owner: {err}"));

            let value = subject_json(name, fragment, Some(""));
            let expected = UnitSubjectError::EmptyOwningPackage {
                name: unit_name(name),
            }
            .to_string();
            let err = serde_json::from_value::<UnitSubject>(value)
                .expect_err("an empty owner is not a known owner")
                .to_string();
            assert!(err.contains(&expected), "{name} {fragment}: {err}");
        }
    }

    mod classify_props {
        use super::super::*;
        use proptest::prelude::*;

        /// Every suffix that has a variant.
        const SUFFIXES: &[&str] = &[
            "service",
            "socket",
            "path",
            "timer",
            "target",
            "slice",
            "mount",
            "automount",
            "swap",
        ];

        /// A proved name, and whether its file is the template's.
        fn subject() -> impl Strategy<Value = UnitSubject> {
            let stem = "[a-z0-9:_.\\\\-]{1,20}";
            let instance = proptest::option::of("[a-z0-9:_.@\\\\-]{0,12}");
            let owner = proptest::option::of("[a-z0-9-]{1,12}");
            (
                stem,
                instance,
                proptest::sample::select(SUFFIXES),
                0..4u8,
                owner,
                any::<bool>(),
            )
                .prop_map(|(stem, instance, suffix, variant, owner, use_template)| {
                    let raw = match instance {
                        Some(instance) => format!("{stem}@{instance}.{suffix}"),
                        None => format!("{stem}.{suffix}"),
                    };
                    let name = UnitName::try_from(raw).expect("generated names are valid");
                    // Only an ordinary instance may load from its template.
                    let file = match name.instance() {
                        Some(parts)
                            if use_template && matches!(name.kind(), UnitKind::Ordinary(_)) =>
                        {
                            parts.template()
                        }
                        _ => name.clone(),
                    };
                    let fragment = match variant {
                        0 => UnitFragment::Vendor {
                            file,
                            owning_package: owner,
                        },
                        1 => UnitFragment::Local { file },
                        2 => UnitFragment::Shadowed {
                            file,
                            owning_package: owner,
                        },
                        _ => UnitFragment::Absent,
                    };
                    UnitSubject::new(name, fragment).expect("file is the unit or its template")
                })
        }

        proptest! {
            /// `classify` returns one of the two for every subject, and the
            /// one the rule names: carried exactly when the kind is not
            /// storage, or the fragment is a vendor file with a known owner.
            /// The wire form loads as that one and is refused as the other,
            /// and the loaded value classifies the same again.
            #[test]
            fn classify_is_total_and_stable(subject in subject()) {
                let name = subject.name().clone();
                let expect_carried = !matches!(name.kind(), UnitKind::Storage(_))
                    || matches!(
                        subject.fragment(),
                        UnitFragment::Vendor { owning_package: Some(_), .. }
                    );
                let wire = serde_json::to_value(&subject).unwrap();
                let reloaded: UnitSubject = serde_json::from_value(wire.clone()).unwrap();
                prop_assert_eq!(&reloaded, &subject);

                let classified = subject.classify();
                prop_assert_eq!(
                    matches!(classified, Classified::Carried(_)),
                    expect_carried,
                    "{} classified as {:?}",
                    name.as_str(),
                    classified
                );
                match classified {
                    Classified::Carried(unit) => {
                        prop_assert_eq!(unit.name(), &name);
                        let json = serde_json::to_value(&unit).unwrap();
                        let again: CarriedUnit = serde_json::from_value(json.clone()).unwrap();
                        prop_assert_eq!(&again, &unit);
                        prop_assert!(serde_json::from_value::<AdvisoryUnit>(json.clone()).is_err());
                        let subject: UnitSubject = serde_json::from_value(json).unwrap();
                        prop_assert_eq!(subject.classify(), Classified::Carried(unit));
                    }
                    Classified::Advisory(unit) => {
                        prop_assert_eq!(unit.name().as_unit_name(), &name);
                        prop_assert!(matches!(name.kind(), UnitKind::Storage(_)));
                        let json = serde_json::to_value(&unit).unwrap();
                        let again: AdvisoryUnit = serde_json::from_value(json.clone()).unwrap();
                        prop_assert_eq!(&again, &unit);
                        prop_assert!(serde_json::from_value::<CarriedUnit>(json.clone()).is_err());
                        let subject: UnitSubject = serde_json::from_value(json).unwrap();
                        prop_assert_eq!(subject.classify(), Classified::Advisory(unit));
                    }
                }
            }
        }
    }

    /// `suffix` is the inverse of the suffix reading, for every suffix the
    /// model knows.
    #[test]
    fn a_kind_names_the_suffix_it_was_read_from() {
        for suffix in [
            "service",
            "socket",
            "path",
            "timer",
            "target",
            "slice",
            "mount",
            "automount",
            "swap",
        ] {
            let kind = UnitKind::from_suffix(suffix).expect("a known suffix");
            assert_eq!(kind.suffix(), suffix);
        }
    }

    /// A finding's `nofail` reads as written when the snapshot holds a
    /// boolean, and as `None` (the options were not read) when it is null
    /// or the key is missing.
    #[test]
    fn a_mount_findings_nofail_reads_false_null_and_missing() {
        let finding = |nofail: Option<serde_json::Value>| {
            let mut json = serde_json::json!({
                "unit": {"name": "data.swap", "fragment": "local", "file": "data.swap"},
                "state_change": ["enabled", null],
                "mount_point": null,
                "aggregate": null
            });
            if let Some(value) = nofail {
                json["nofail"] = value;
            }
            serde_json::from_value::<MountUnitFinding>(json)
                .expect("the finding deserializes")
                .nofail
        };
        assert_eq!(finding(Some(serde_json::json!(false))), Some(false));
        assert_eq!(finding(Some(serde_json::json!(true))), Some(true));
        assert_eq!(finding(Some(serde_json::Value::Null)), None);
        assert_eq!(finding(None), None);
    }

    /// Each reason an `[Install]` section is undetermined round-trips as
    /// its snake_case name.
    #[test]
    fn an_install_unknown_round_trips_by_its_snake_case_name() {
        for (reason, text) in [
            (InstallUnknown::Specifier, "\"specifier\""),
            (InstallUnknown::RequiredByReset, "\"required_by_reset\""),
            (InstallUnknown::ForeignName, "\"foreign_name\""),
            (InstallUnknown::UpheldBy, "\"upheld_by\""),
            (InstallUnknown::Unreadable, "\"unreadable\""),
        ] {
            assert_eq!(serde_json::to_string(&reason).expect("serializes"), text);
            assert_eq!(
                serde_json::from_str::<InstallUnknown>(text).expect("deserializes"),
                reason
            );
        }
    }
}
