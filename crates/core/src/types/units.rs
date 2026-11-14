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

/// The kind of a unit, read from its name's suffix.
///
/// `.device` and `.scope` have no variant: they never exist as unit files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitStateChange {
    pub unit: UnitName,
    pub current_state: UnitState,
    #[serde(deserialize_with = "require_explicit_null")]
    pub default_state: Option<PresetDefault>,
    #[serde(default)]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub owning_package: Option<String>,
    pub aggregate: Option<AggregatePrevalence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnitSection {
    /// Units whose state differs from what the image would have.
    pub state_changes: Vec<UnitStateChange>,
    /// Every unit with a durable state, changed or not, once each.
    pub inventory: Vec<UnitInventoryEntry>,
    pub drop_ins: Vec<SystemdDropIn>,
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_service_state_change_roundtrip_uses_typed_enums() {
        let section = UnitSection {
            state_changes: vec![
                UnitStateChange {
                    unit: unit_name("firewalld.service"),
                    current_state: UnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: Some("firewalld".into()),
                    aggregate: None,
                    attention_reason: None,
                },
                UnitStateChange {
                    unit: unit_name("cups.service"),
                    current_state: UnitState::Masked,
                    default_state: None,
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: Some("cups".into()),
                    aggregate: None,
                    attention_reason: None,
                },
            ],
            inventory: vec![UnitInventoryEntry {
                unit: unit_name("firewalld.service"),
                state: UnitState::Enabled,
                preset: Some(PresetDefault::Disable),
            }],
            drop_ins: vec![],
        };

        let json = serde_json::to_string(&section).unwrap();
        let parsed: UnitSection = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, section);
    }

    #[test]
    fn test_implied_action_derives_from_current_state() {
        let enabled = UnitStateChange {
            unit: unit_name("firewalld.service"),
            current_state: UnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            disposition: FindingKind::included(),
            locked: false,
            owning_package: Some("firewalld".into()),
            aggregate: None,
            attention_reason: None,
        };
        let disabled = UnitStateChange {
            unit: unit_name("sshd.service"),
            current_state: UnitState::Disabled,
            default_state: Some(PresetDefault::Enable),
            disposition: FindingKind::included(),
            locked: false,
            owning_package: Some("openssh-server".into()),
            aggregate: None,
            attention_reason: None,
        };
        let masked = UnitStateChange {
            unit: unit_name("cups.service"),
            current_state: UnitState::Masked,
            default_state: None,
            disposition: FindingKind::included(),
            locked: false,
            owning_package: Some("cups".into()),
            aggregate: None,
            attention_reason: None,
        };

        assert_eq!(enabled.implied_action(), UnitAction::Enable);
        assert_eq!(disabled.implied_action(), UnitAction::Disable);
        assert_eq!(masked.implied_action(), UnitAction::Mask);
    }

    #[test]
    fn test_option_preset_default_serde_roundtrip() {
        let with_preset = UnitStateChange {
            unit: unit_name("firewalld.service"),
            current_state: UnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            disposition: FindingKind::included(),
            locked: false,
            owning_package: Some("firewalld".into()),
            aggregate: None,
            attention_reason: None,
        };
        let json = serde_json::to_string(&with_preset).unwrap();
        let parsed: UnitStateChange = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.default_state, Some(PresetDefault::Disable));

        let without_preset = UnitStateChange {
            unit: unit_name("cups.service"),
            current_state: UnitState::Masked,
            default_state: None,
            disposition: FindingKind::included(),
            locked: false,
            owning_package: Some("cups".into()),
            aggregate: None,
            attention_reason: None,
        };
        let json = serde_json::to_string(&without_preset).unwrap();
        let parsed: UnitStateChange = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.default_state, None);

        let null_json = r#"{
            "unit":"cups.service",
            "current_state":"masked",
            "default_state":null,
            "include":true,
            "owning_package":"cups",
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
        let with_field = r#"{"state_changes":[],"inventory":[],"drop_ins":[]}"#;
        serde_json::from_str::<UnitSection>(with_field).expect("complete section loads");

        let json = r#"{"state_changes":[],"drop_ins":[]}"#;
        let err = serde_json::from_str::<UnitSection>(json).unwrap_err();
        assert!(
            err.to_string().contains("inventory"),
            "expected missing-field error for inventory, got: {err}"
        );
    }

    #[test]
    fn test_missing_default_state_does_not_deserialize() {
        let json = r#"{
            "unit":"firewalld.service",
            "current_state":"enabled",
            "include":true,
            "owning_package":"firewalld",
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
        let json = r#"{"unit":"test.service","current_state":"enabled","default_state":null}"#;
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
        let json = r#"{"unit":"sshd.service","current_state":"enabled","default_state":null,"disposition":{"kind":"actionable","include":true}}"#;
        let ssc: UnitStateChange = serde_json::from_str(json).unwrap();
        assert_eq!(ssc.current_state, UnitState::Enabled);
    }
}
