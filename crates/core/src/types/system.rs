use crate::types::completeness::SourceSystemKind;
use crate::types::os::{OsRelease, OstreeVariant};

pub type ImageRef = String;

/// What we're inspecting. Each variant carries exactly the data
/// its inspectors need. NOT serialized to snapshot JSON — constructed
/// from snapshot fields during pipeline processing.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceSystem {
    PackageBased {
        os_release: OsRelease,
    },
    RpmOstree {
        os_release: OsRelease,
        variant: OstreeVariant,
        base_image: Option<ImageRef>,
    },
    Bootc {
        os_release: OsRelease,
        booted_image: ImageRef,
        staged_image: Option<ImageRef>,
    },
}

/// Migration target. Always bootc-based.
#[derive(Debug, Clone, PartialEq)]
pub enum TargetSystem {
    BootcImage { image_ref: ImageRef },
    CustomImage { image_ref: ImageRef, base: ImageRef },
}

/// Source + target determine inspector behavior and rendering.
#[derive(Debug, Clone)]
pub struct MigrationContext {
    pub source: SourceSystem,
    pub target: TargetSystem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationKind {
    SameStream,
    MajorUpgrade,
    VendorTransition,
    CommunityToEnterprise,
    OstreeToBootc,
}

impl SourceSystem {
    pub fn kind(&self) -> SourceSystemKind {
        match self {
            SourceSystem::PackageBased { .. } => SourceSystemKind::PackageBased,
            SourceSystem::RpmOstree { .. } => SourceSystemKind::RpmOstree,
            SourceSystem::Bootc { .. } => SourceSystemKind::Bootc,
        }
    }

    pub fn os_release(&self) -> &OsRelease {
        match self {
            Self::PackageBased { os_release, .. }
            | Self::RpmOstree { os_release, .. }
            | Self::Bootc { os_release, .. } => os_release,
        }
    }

    pub fn major_version(&self) -> Option<u32> {
        let vid = &self.os_release().version_id;
        vid.split('.').next().and_then(|s| s.parse().ok())
    }
}

impl MigrationContext {
    pub fn is_cross_major(&self) -> bool {
        matches!(self.migration_kind(), MigrationKind::MajorUpgrade)
    }

    pub fn is_cross_vendor(&self) -> bool {
        matches!(
            self.migration_kind(),
            MigrationKind::VendorTransition | MigrationKind::CommunityToEnterprise
        )
    }

    pub fn migration_kind(&self) -> MigrationKind {
        let src = self.source.os_release();
        match &self.source {
            SourceSystem::RpmOstree { .. } => MigrationKind::OstreeToBootc,
            _ => {
                let src_id = src.id.as_str();
                let src_major = self.source.major_version();
                let target_major = self.target_major_version();

                match (src_id, src_major, target_major) {
                    ("fedora", _, _) => MigrationKind::CommunityToEnterprise,
                    ("centos", _, _) => MigrationKind::VendorTransition,
                    (_, Some(s), Some(t)) if s != t => MigrationKind::MajorUpgrade,
                    _ => MigrationKind::SameStream,
                }
            }
        }
    }

    fn target_major_version(&self) -> Option<u32> {
        let image_ref = match &self.target {
            TargetSystem::BootcImage { image_ref } => image_ref,
            TargetSystem::CustomImage { image_ref, .. } => image_ref,
        };
        // Extract major version from image tag (e.g., "rhel-bootc:10.0" → 10)
        image_ref
            .rsplit(':')
            .next()
            .and_then(|tag| tag.split('.').next())
            .and_then(|s| s.parse().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::completeness::SourceSystemKind;
    use crate::types::os::OsRelease;

    #[test]
    fn test_source_system_kind_derivation() {
        let pkg = SourceSystem::PackageBased {
            os_release: OsRelease::default(),
        };
        assert_eq!(pkg.kind(), SourceSystemKind::PackageBased);

        let ostree = SourceSystem::RpmOstree {
            os_release: OsRelease::default(),
            variant: crate::types::os::OstreeVariant::Silverblue,
            base_image: None,
        };
        assert_eq!(ostree.kind(), SourceSystemKind::RpmOstree);

        let bootc = SourceSystem::Bootc {
            os_release: OsRelease::default(),
            booted_image: "registry.example.com/rhel:9".into(),
            staged_image: None,
        };
        assert_eq!(bootc.kind(), SourceSystemKind::Bootc);
    }

    #[test]
    fn test_same_stream_migration() {
        let ctx = MigrationContext {
            source: SourceSystem::PackageBased {
                os_release: OsRelease {
                    id: "rhel".into(),
                    version_id: "9.4".into(),
                    ..Default::default()
                },
            },
            target: TargetSystem::BootcImage {
                image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.4".into(),
            },
        };
        assert_eq!(ctx.migration_kind(), MigrationKind::SameStream);
        assert!(!ctx.is_cross_major());
        assert!(!ctx.is_cross_vendor());
    }

    #[test]
    fn test_major_upgrade_migration() {
        let ctx = MigrationContext {
            source: SourceSystem::PackageBased {
                os_release: OsRelease {
                    id: "rhel".into(),
                    version_id: "9.4".into(),
                    ..Default::default()
                },
            },
            target: TargetSystem::BootcImage {
                image_ref: "registry.redhat.io/rhel10/rhel-bootc:10.0".into(),
            },
        };
        assert_eq!(ctx.migration_kind(), MigrationKind::MajorUpgrade);
        assert!(ctx.is_cross_major());
    }

    #[test]
    fn test_el8_mapped_up_default_is_major_upgrade() {
        // EL8 hosts default-resolve to the EL9 floor tag (rhel-bootc:9.6).
        // target_major_version() parses the major from the image tag, so the
        // default must stay version-pinned — a `:latest` default made this
        // return None and misclassified EL8→EL9 as SameStream.
        let ctx = MigrationContext {
            source: SourceSystem::PackageBased {
                os_release: OsRelease {
                    id: "rhel".into(),
                    version_id: "8.10".into(),
                    ..Default::default()
                },
            },
            target: TargetSystem::BootcImage {
                image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
            },
        };
        assert_eq!(ctx.migration_kind(), MigrationKind::MajorUpgrade);
        assert!(ctx.is_cross_major());
    }

    #[test]
    fn test_el8_default_resolution_composes_to_major_upgrade() {
        // Composition guard for the same regression. The test above pins a
        // literal image_ref, so it passes even if the default resolution goes
        // back to `:latest`; this one resolves the target the way the pipeline
        // does, and fails if the default ever stops being version-pinned.
        let os_release = OsRelease {
            id: "rhel".into(),
            version_id: "8.10".into(),
            ..Default::default()
        };
        let target_ref = crate::baseline::resolve_base_image(&os_release, None, None, None)
            .unwrap()
            .image_ref;
        let ctx = MigrationContext {
            source: SourceSystem::PackageBased { os_release },
            target: TargetSystem::BootcImage {
                image_ref: target_ref,
            },
        };
        assert_eq!(ctx.migration_kind(), MigrationKind::MajorUpgrade);
        assert!(ctx.is_cross_major());
    }

    #[test]
    fn test_el9_default_resolution_composes_to_same_stream() {
        // Negative control for the guard above: the floor clamp lifts a 9.4
        // host to the 9.6 tag, and a within-major minor bump must not read as
        // a major upgrade.
        let os_release = OsRelease {
            id: "rhel".into(),
            version_id: "9.4".into(),
            ..Default::default()
        };
        let target_ref = crate::baseline::resolve_base_image(&os_release, None, None, None)
            .unwrap()
            .image_ref;
        let ctx = MigrationContext {
            source: SourceSystem::PackageBased { os_release },
            target: TargetSystem::BootcImage {
                image_ref: target_ref,
            },
        };
        assert_eq!(ctx.migration_kind(), MigrationKind::SameStream);
        assert!(!ctx.is_cross_major());
    }

    #[test]
    fn test_bootc_source_booted_only() {
        let source = SourceSystem::Bootc {
            os_release: OsRelease::default(),
            booted_image: "registry.redhat.io/rhel9/rhel-bootc:9.4".into(),
            staged_image: Some("registry.redhat.io/rhel9/rhel-bootc:9.5".into()),
        };
        if let SourceSystem::Bootc {
            booted_image,
            staged_image,
            ..
        } = &source
        {
            assert!(!booted_image.is_empty());
            assert!(staged_image.is_some());
        }
    }
}
