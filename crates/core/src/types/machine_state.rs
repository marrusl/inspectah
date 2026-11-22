//! The machine-state class: configuration files that belong to one
//! machine rather than to the image.
//!
//! A *carriable* member is a host identity or generated file whose content
//! is safe to hold: it is an ordinary config entry with the disposition
//! [`FindingKind::MachineState`](super::FindingKind::MachineState),
//! excluded by default and overridable. A *withheld* member (every host
//! credential, and the identity files a copy always breaks) is a
//! [`WithheldConfigEntry`], which has no content field and no include
//! flag, so a credential with content, or an included credential, cannot
//! be written down.
//!
//! The rule table that maps paths to members, the read gate and snapshot
//! validation live in [`crate::machine_state`].

use super::aggregate::AggregatePrevalence;
use super::config::ConfigFileKind;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Why a file belongs to one machine rather than to the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineStateReason {
    /// Names this machine: machine-id, hostname, iSCSI IQN, Insights id.
    HostIdentity,
    /// Holds a key or a certificate bound to this machine.
    HostCredential,
    /// Written by a tool from settings carried elsewhere.
    Generated,
}

/// A class member whose content the snapshot may hold. Only these can
/// appear in `FindingKind::MachineState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarriableMember {
    Hostname,
    Syspurpose,
    NvmeHostId,
    IscsiInitiatorName,
    IpaDefaultConf,
    SaltMinionId,
    AuthselectSwitchable,
    GrubTuned,
}

impl CarriableMember {
    /// Every carriable member, in declaration order.
    pub const ALL: &'static [CarriableMember] = &[
        Self::Hostname,
        Self::Syspurpose,
        Self::NvmeHostId,
        Self::IscsiInitiatorName,
        Self::IpaDefaultConf,
        Self::SaltMinionId,
        Self::AuthselectSwitchable,
        Self::GrubTuned,
    ];

    /// The member's JSON name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hostname => "hostname",
            Self::Syspurpose => "syspurpose",
            Self::NvmeHostId => "nvme_host_id",
            Self::IscsiInitiatorName => "iscsi_initiator_name",
            Self::IpaDefaultConf => "ipa_default_conf",
            Self::SaltMinionId => "salt_minion_id",
            Self::AuthselectSwitchable => "authselect_switchable",
            Self::GrubTuned => "grub_tuned",
        }
    }

    /// How refine offers to carry the member: `Warn` or `Plain`, never
    /// `Never`, since a carriable member's content is held.
    pub const fn override_level(self) -> OverrideLevel {
        match self {
            Self::IscsiInitiatorName | Self::SaltMinionId => OverrideLevel::Warn,
            Self::Hostname
            | Self::Syspurpose
            | Self::NvmeHostId
            | Self::IpaDefaultConf
            | Self::AuthselectSwitchable
            | Self::GrubTuned => OverrideLevel::Plain,
        }
    }
}

/// Every class member. A withheld row names one of these.
///
/// Serialized as one flat snake-case string whichever variant holds it:
/// `Carriable(IscsiInitiatorName)` is `"iscsi_initiator_name"`, so a
/// carriable member has one spelling in Rust and one in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MachineStateMember {
    // Withheld by rule: content never held; not opened when matched by
    // path, alias or inode.
    MachineId,
    InsightsState,
    RhsmConsumer,
    RhsmEntitlement,
    LvmSystemDevices,
    KerberosKeytab,
    SshHostKey,
    SshHostCertificate,
    PrivateKeyFile,
    ConfigAgentKey,
    /// A service's shared secret or private key at a fixed path: chrony's
    /// NTP keys, bind's rndc key, unbound's control keys.
    ServiceKey,
    /// Carriable members, withheld only when their read failed and on a
    /// fleet aggregate.
    Carriable(CarriableMember),
}

impl MachineStateMember {
    /// Every member withheld by rule, in declaration order. The carriable
    /// members are [`CarriableMember::ALL`].
    pub const WITHHELD: &'static [MachineStateMember] = &[
        Self::MachineId,
        Self::InsightsState,
        Self::RhsmConsumer,
        Self::RhsmEntitlement,
        Self::LvmSystemDevices,
        Self::KerberosKeytab,
        Self::SshHostKey,
        Self::SshHostCertificate,
        Self::PrivateKeyFile,
        Self::ConfigAgentKey,
        Self::ServiceKey,
    ];

    /// The member's JSON name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MachineId => "machine_id",
            Self::InsightsState => "insights_state",
            Self::RhsmConsumer => "rhsm_consumer",
            Self::RhsmEntitlement => "rhsm_entitlement",
            Self::LvmSystemDevices => "lvm_system_devices",
            Self::KerberosKeytab => "kerberos_keytab",
            Self::SshHostKey => "ssh_host_key",
            Self::SshHostCertificate => "ssh_host_certificate",
            Self::PrivateKeyFile => "private_key_file",
            Self::ConfigAgentKey => "config_agent_key",
            Self::ServiceKey => "service_key",
            Self::Carriable(m) => m.as_str(),
        }
    }

    /// The member named `name` in JSON.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::WITHHELD
            .iter()
            .copied()
            .chain(CarriableMember::ALL.iter().copied().map(Self::Carriable))
            .find(|m| m.as_str() == name)
    }

    /// Why the file belongs to one machine.
    pub const fn reason(self) -> MachineStateReason {
        use MachineStateReason::{Generated, HostCredential, HostIdentity};
        match self {
            Self::MachineId | Self::InsightsState | Self::LvmSystemDevices => HostIdentity,
            Self::RhsmConsumer
            | Self::RhsmEntitlement
            | Self::KerberosKeytab
            | Self::SshHostKey
            | Self::SshHostCertificate
            | Self::PrivateKeyFile
            | Self::ConfigAgentKey
            | Self::ServiceKey => HostCredential,
            Self::Carriable(m) => match m {
                CarriableMember::AuthselectSwitchable | CarriableMember::GrubTuned => Generated,
                CarriableMember::Hostname
                | CarriableMember::Syspurpose
                | CarriableMember::NvmeHostId
                | CarriableMember::IscsiInitiatorName
                | CarriableMember::IpaDefaultConf
                | CarriableMember::SaltMinionId => HostIdentity,
            },
        }
    }

    /// What replaces the file on a machine deployed from the image. Derived
    /// from the member, never stored, so no snapshot can carry a notice
    /// that disagrees with its member.
    pub const fn notice(self) -> ProvisioningNotice {
        use ProvisioningNotice::{Automatic, None as NoNotice, Owed};
        use ProvisioningStep as Step;
        use RecreatedBy::{CarriedSetting, FirstBoot, Provisioning};
        match self {
            Self::MachineId => Automatic { by: FirstBoot },
            Self::InsightsState => Owed {
                step: Step::InsightsRegister,
            },
            Self::RhsmConsumer | Self::RhsmEntitlement => Owed {
                step: Step::SubscriptionRegister,
            },
            Self::LvmSystemDevices => Automatic { by: Provisioning },
            Self::KerberosKeytab => Owed {
                step: Step::RealmJoin,
            },
            Self::SshHostCertificate => Owed {
                step: Step::ResignSshHostCerts,
            },
            Self::SshHostKey => Automatic { by: FirstBoot },
            Self::PrivateKeyFile | Self::ServiceKey => Owed {
                step: Step::DeliverPrivateKey,
            },
            Self::ConfigAgentKey => Owed {
                step: Step::RebootstrapConfigAgent,
            },
            Self::Carriable(m) => match m {
                CarriableMember::IscsiInitiatorName => Automatic { by: FirstBoot },
                CarriableMember::SaltMinionId => Owed {
                    step: Step::RebootstrapConfigAgent,
                },
                CarriableMember::IpaDefaultConf => Owed {
                    step: Step::RealmJoin,
                },
                CarriableMember::Hostname => Automatic { by: Provisioning },
                CarriableMember::Syspurpose => Owed {
                    step: Step::SubscriptionRegister,
                },
                CarriableMember::NvmeHostId => Automatic { by: FirstBoot },
                CarriableMember::AuthselectSwitchable => Automatic { by: CarriedSetting },
                CarriableMember::GrubTuned => NoNotice,
            },
        }
    }

    /// How refine offers to carry the member. Every withheld member is
    /// `Never`: the snapshot holds no content to ship.
    pub const fn override_level(self) -> OverrideLevel {
        match self {
            Self::Carriable(m) => m.override_level(),
            _ => OverrideLevel::Never,
        }
    }
}

impl Serialize for MachineStateMember {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MachineStateMember {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::from_name(&name).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown machine-state member: {name:?}"))
        })
    }
}

/// How refine offers to carry a member anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideLevel {
    /// Not offered: the snapshot holds no content for it.
    Never,
    /// Offered behind a confirmation.
    Warn,
    /// Offered like any other include.
    Plain,
}

/// What replaces a machine-state file on a machine deployed from the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProvisioningNotice {
    /// The operator runs a step when deploying a machine from the image.
    Owed { step: ProvisioningStep },
    /// Something recreates the file without the operator.
    Automatic { by: RecreatedBy },
    /// Nothing replaces it and nothing needs to.
    None,
}

/// A step the operator owes at provisioning, in the order every surface
/// lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvisioningStep {
    RealmJoin,
    SubscriptionRegister,
    InsightsRegister,
    ResignSshHostCerts,
    DeliverPrivateKey,
    RebootstrapConfigAgent,
}

impl ProvisioningStep {
    /// Every step, in declaration order.
    pub const ALL: &'static [ProvisioningStep] = &[
        Self::RealmJoin,
        Self::SubscriptionRegister,
        Self::InsightsRegister,
        Self::ResignSshHostCerts,
        Self::DeliverPrivateKey,
        Self::RebootstrapConfigAgent,
    ];

    /// The step as display text.
    pub const fn command(self) -> &'static str {
        match self {
            Self::RealmJoin => "ipa-client-install, realm join or adcli join",
            Self::SubscriptionRegister => "subscription-manager register or rhc connect",
            Self::InsightsRegister => "insights-client --register or rhc connect",
            Self::ResignSshHostCerts => "re-sign the SSH host certificates",
            Self::DeliverPrivateKey => "re-issue the key, or deliver it at deploy time",
            Self::RebootstrapConfigAgent => "re-bootstrap the configuration management agent",
        }
    }

    /// The short form the Containerfile and the export header use: the
    /// leading command and its alternatives in brackets.
    pub const fn short(self) -> &'static str {
        match self {
            Self::RealmJoin => "realm join (or ipa-client-install, adcli join)",
            Self::SubscriptionRegister => "subscription-manager register (or rhc connect)",
            Self::InsightsRegister => "insights-client --register (or rhc connect)",
            Self::ResignSshHostCerts => "re-sign the SSH host certificates",
            Self::DeliverPrivateKey => "deliver the private keys at deploy time",
            Self::RebootstrapConfigAgent => "re-bootstrap the configuration management agent",
        }
    }
}

/// What recreates a file without the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecreatedBy {
    /// A unit creates it when missing.
    FirstBoot,
    /// The installer, cloud-init or DHCP sets it.
    Provisioning,
    /// The tool that wrote it reruns from a setting the image carries.
    CarriedSetting,
}

/// A configuration file the scan found and whose content the snapshot
/// does not hold. Never staged, never included: the type has nowhere to
/// put either.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WithheldConfigEntry {
    /// Normalized absolute path.
    pub path: String,
    /// Which branch found it.
    pub kind: ConfigFileKind,
    #[serde(default)]
    pub package: Option<String>,
    pub member: MachineStateMember,
    /// From stat of the file the path resolves to. `None` when the stat
    /// failed.
    #[serde(default)]
    pub mode: Option<u32>,
    #[serde(default)]
    pub size: Option<u64>,
    /// Hosts that have the path, on a fleet aggregate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregatePrevalence>,
}

/// An unmanaged file the read gate withheld: an alias or hard link of a
/// withheld member's file, a keytab, or a file holding a PEM private key
/// anywhere in its bytes. No disposition, so it cannot be included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WithheldUnmanagedEntry {
    pub path: String,
    pub member: MachineStateMember,
    #[serde(default)]
    pub mode: Option<u32>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregatePrevalence>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_member_round_trips_as_one_flat_string() {
        let all = MachineStateMember::WITHHELD.iter().copied().chain(
            CarriableMember::ALL
                .iter()
                .copied()
                .map(MachineStateMember::Carriable),
        );
        for member in all {
            let json = serde_json::to_string(&member).unwrap();
            assert_eq!(json, format!("\"{}\"", member.as_str()));
            let back: MachineStateMember = serde_json::from_str(&json).unwrap();
            assert_eq!(back, member);
        }
        for member in CarriableMember::ALL {
            let json = serde_json::to_string(member).unwrap();
            assert_eq!(json, format!("\"{}\"", member.as_str()));
            assert_ne!(member.override_level(), OverrideLevel::Never);
        }
        assert!(serde_json::from_str::<MachineStateMember>("\"no_such\"").is_err());
    }

    #[test]
    fn every_credential_is_never_overridable() {
        for member in MachineStateMember::WITHHELD {
            assert_eq!(member.override_level(), OverrideLevel::Never);
        }
        for member in CarriableMember::ALL {
            assert_ne!(
                MachineStateMember::Carriable(*member).reason(),
                MachineStateReason::HostCredential,
                "a carriable member is never a credential"
            );
        }
    }

    #[test]
    fn withheld_entry_has_nowhere_to_hold_content() {
        let json = r#"{"path":"/etc/krb5.keytab","kind":"unowned","package":null,
            "member":"kerberos_keytab","mode":384,"size":82,"content":"secret"}"#;
        let entry: WithheldConfigEntry = serde_json::from_str(json).unwrap();
        let back = serde_json::to_string(&entry).unwrap();
        assert!(!back.contains("content"));
        assert!(!back.contains("secret"));
        assert_eq!(entry.member, MachineStateMember::KerberosKeytab);
    }

    #[test]
    fn notices_serialize_with_their_kind() {
        let owed = MachineStateMember::KerberosKeytab.notice();
        assert_eq!(
            serde_json::to_string(&owed).unwrap(),
            r#"{"kind":"owed","step":"realm_join"}"#
        );
        let auto = MachineStateMember::Carriable(CarriableMember::IscsiInitiatorName).notice();
        assert_eq!(
            serde_json::to_string(&auto).unwrap(),
            r#"{"kind":"automatic","by":"first_boot"}"#
        );
    }
}
