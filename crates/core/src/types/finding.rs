use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FindingKind {
    Actionable {
        include: bool,
    },
    Advisory {
        advisory_type: AdvisoryType,
        rationale: String,
    },
    /// Informational inventory — displayed but never included in
    /// Containerfile output and non-toggleable across all interactive
    /// surfaces. Used for network findings.
    Inventory,
}

impl Default for FindingKind {
    fn default() -> Self {
        Self::included()
    }
}

impl FindingKind {
    pub fn included() -> Self {
        Self::Actionable { include: true }
    }

    pub fn excluded() -> Self {
        Self::Actionable { include: false }
    }

    pub fn advisory(advisory_type: AdvisoryType, rationale: impl Into<String>) -> Self {
        Self::Advisory {
            advisory_type,
            rationale: rationale.into(),
        }
    }

    pub fn inventory() -> Self {
        Self::Inventory
    }

    /// Convert a legacy bool into FindingKind (true → included, false → excluded).
    pub fn from_bool(include: bool) -> Self {
        if include {
            Self::included()
        } else {
            Self::excluded()
        }
    }

    /// Merge-safe include setter: preserves Advisory and Inventory semantics.
    /// Only Actionable findings have their include flag changed; Advisory and
    /// Inventory findings are non-toggleable and pass through unchanged.
    pub fn with_include(&self, val: bool) -> Self {
        match self {
            Self::Advisory { .. } | Self::Inventory => self.clone(),
            _ => Self::from_bool(val),
        }
    }

    pub fn is_included(&self) -> bool {
        matches!(self, Self::Actionable { include: true })
    }

    pub fn is_advisory(&self) -> bool {
        matches!(self, Self::Advisory { .. })
    }

    pub fn is_inventory(&self) -> bool {
        matches!(self, Self::Inventory)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryType {
    UnbackedVarDir,
    CrossTreeSymlink,
    Modernization,
}

/// Spec field name: `shadow_type` (not `override_type`)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShadowType {
    DropIn,
    FullShadow,
}

/// Serde default helper: returns `FindingKind::excluded()`.
/// Used for fields that default to excluded (e.g. tuned_disposition).
pub fn default_finding_excluded() -> FindingKind {
    FindingKind::excluded()
}

/// Serde default helper: returns `FindingKind::inventory()`.
/// Used for fields that default to inventory (e.g. network items).
pub fn default_finding_inventory() -> FindingKind {
    FindingKind::inventory()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finding_kind_serde_roundtrip_actionable() {
        let kind = FindingKind::included();
        let json = serde_json::to_string(&kind).unwrap();
        let parsed: FindingKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, parsed);
        assert!(parsed.is_included());
        assert!(!parsed.is_advisory());
    }

    #[test]
    fn test_finding_kind_serde_roundtrip_advisory() {
        let kind = FindingKind::advisory(AdvisoryType::UnbackedVarDir, "No tmpfiles.d backing");
        let json = serde_json::to_string(&kind).unwrap();
        let parsed: FindingKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, parsed);
        assert!(!parsed.is_included());
        assert!(parsed.is_advisory());
    }

    #[test]
    fn test_advisory_json_shape() {
        let kind = FindingKind::advisory(AdvisoryType::Modernization, "xinetd is deprecated");
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains(r#""kind":"advisory"#));
        assert!(json.contains(r#""advisory_type":"modernization"#));
        assert!(json.contains(r#""rationale":"xinetd is deprecated"#));
    }

    #[test]
    fn test_finding_kind_serde_roundtrip_inventory() {
        let kind = FindingKind::inventory();
        let json = serde_json::to_string(&kind).unwrap();
        let parsed: FindingKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, parsed);
        assert!(!parsed.is_included());
        assert!(!parsed.is_advisory());
        assert!(parsed.is_inventory());
    }

    #[test]
    fn test_inventory_json_shape() {
        let kind = FindingKind::inventory();
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains(r#""kind":"inventory"#));
        // No additional fields — unit variant
        assert!(!json.contains("include"));
        assert!(!json.contains("advisory_type"));
    }

    #[test]
    fn test_default_is_included() {
        let kind = FindingKind::default();
        assert!(kind.is_included());
    }
}
