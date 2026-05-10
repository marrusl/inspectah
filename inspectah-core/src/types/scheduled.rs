use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CronJob {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub source: String,
    #[serde(default)]
    pub rpm_owned: bool,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemdTimer {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub on_calendar: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub exec_start: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub description: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub source: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub timer_content: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub service_content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AtJob {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub file: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub command: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub user: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub working_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GeneratedTimerUnit {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub timer_content: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub service_content: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub cron_expr: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub source_path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub command: String,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScheduledTaskSection {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub cron_jobs: Vec<CronJob>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub systemd_timers: Vec<SystemdTimer>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub at_jobs: Vec<AtJob>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub generated_timer_units: Vec<GeneratedTimerUnit>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduled_section_roundtrip() {
        let section = ScheduledTaskSection {
            cron_jobs: vec![CronJob {
                path: "/etc/cron.d/backup".into(),
                source: "file".into(),
                include: true,
                ..Default::default()
            }],
            generated_timer_units: vec![GeneratedTimerUnit {
                name: "backup.timer".into(),
                cron_expr: "0 2 * * *".into(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: ScheduledTaskSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}
