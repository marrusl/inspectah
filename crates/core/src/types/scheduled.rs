use super::aggregate::AggregatePrevalence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CronJob {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub rpm_owned: bool,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemdTimer {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub on_calendar: String,
    #[serde(default)]
    pub exec_start: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub timer_content: String,
    #[serde(default)]
    pub service_content: String,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregatePrevalence>,
}

impl Default for SystemdTimer {
    fn default() -> Self {
        Self {
            include: true,
            name: Default::default(),
            on_calendar: Default::default(),
            exec_start: Default::default(),
            description: Default::default(),
            source: Default::default(),
            path: Default::default(),
            timer_content: Default::default(),
            service_content: Default::default(),
            locked: Default::default(),
            aggregate: Default::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtJob {
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub working_dir: String,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregatePrevalence>,
}

impl Default for AtJob {
    fn default() -> Self {
        Self {
            include: true,
            file: Default::default(),
            command: Default::default(),
            user: Default::default(),
            working_dir: Default::default(),
            locked: Default::default(),
            aggregate: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GeneratedTimerUnit {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub timer_content: String,
    #[serde(default)]
    pub service_content: String,
    #[serde(default)]
    pub cron_expr: String,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub command: String,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScheduledTaskSection {
    #[serde(default)]
    pub cron_jobs: Vec<CronJob>,
    #[serde(default)]
    pub systemd_timers: Vec<SystemdTimer>,
    #[serde(default)]
    pub at_jobs: Vec<AtJob>,
    #[serde(default)]
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

    #[test]
    fn cron_job_without_include_deserializes_as_true() {
        let json = r#"{"path":"/etc/cron.d/backup","source":"file"}"#;
        let cj: CronJob = serde_json::from_str(json).unwrap();
        assert!(
            cj.include,
            "missing include field should deserialize as true"
        );
    }
}
