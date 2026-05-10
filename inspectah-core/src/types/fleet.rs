use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetPrevalence {
    #[serde(default)]
    pub count: i32,
    #[serde(default)]
    pub total: i32,
    #[serde(default)]
    pub hosts: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetMeta {
    #[serde(default)]
    pub source_hosts: Vec<String>,
    #[serde(default)]
    pub total_hosts: i32,
    #[serde(default)]
    pub min_prevalence: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fleet_prevalence_roundtrip() {
        let fp = FleetPrevalence {
            count: 3,
            total: 5,
            hosts: vec!["host1".into(), "host2".into(), "host3".into()],
        };
        let json = serde_json::to_string(&fp).unwrap();
        let parsed: FleetPrevalence = serde_json::from_str(&json).unwrap();
        assert_eq!(fp, parsed);
    }

    #[test]
    fn test_fleet_prevalence_null_deserialize() {
        let val: Option<FleetPrevalence> = serde_json::from_str("null").unwrap();
        assert!(val.is_none());
    }
}
