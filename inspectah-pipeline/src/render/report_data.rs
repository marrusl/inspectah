use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::{Completeness, InspectorId};
use inspectah_core::types::users::UserGroupDecision;
use serde::Serialize;

// ---------------------------------------------------------------------------
// SectionState (from T1)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionState {
    Normal,
    Degraded,
    Failed,
}

pub fn section_state(id: InspectorId, completeness: &Completeness) -> SectionState {
    match completeness {
        Completeness::Complete => SectionState::Normal,
        Completeness::Partial {
            degraded_sections, ..
        } => {
            if degraded_sections.contains(&id) {
                SectionState::Degraded
            } else {
                SectionState::Normal
            }
        }
        Completeness::Incomplete {
            failed_sections,
            degraded_sections,
            ..
        } => {
            if failed_sections.contains(&id) {
                SectionState::Failed
            } else if degraded_sections.contains(&id) {
                SectionState::Degraded
            } else {
                SectionState::Normal
            }
        }
    }
}

// ---------------------------------------------------------------------------
// script_safe_json — escape characters dangerous in HTML <script> blocks
// ---------------------------------------------------------------------------

/// Escape characters that are dangerous inside HTML `<script>` blocks.
///
/// Uses JSON-valid unicode escape sequences so the result is still valid JSON
/// but cannot break out of a `<script>` context.
pub fn script_safe_json(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for ch in json.chars() {
        match ch {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(ch),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// ReportFilterData — minimized DTO for JS filter enhancement
// ---------------------------------------------------------------------------

/// Minimized DTO carrying only display-safe filterable fields for JS
/// enhancement. No secrets, no redaction data.
#[derive(Debug, Clone, Serialize)]
pub struct ReportFilterData {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<FilterablePackage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config_files: Vec<FilterableConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<FilterableService>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scheduled: Vec<FilterableScheduled>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<FilterableUser>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterablePackage {
    pub name: String,
    pub version: String,
    pub release: String,
    pub arch: String,
    pub repo: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterableConfig {
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterableService {
    pub unit: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterableScheduled {
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterableUser {
    pub name: String,
    pub uid: u64,
}

/// Helper: serialize an enum value to its serde string representation.
fn enum_to_string<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .expect("enum serialization is infallible for derived Serialize")
        .trim_matches('"')
        .to_string()
}

/// Build the filter data DTO from a snapshot.
pub fn build_filter_data(snap: &InspectionSnapshot) -> ReportFilterData {
    let packages = snap
        .rpm
        .as_ref()
        .map(|rpm| {
            rpm.packages_added
                .iter()
                .map(|p| FilterablePackage {
                    name: p.name.clone(),
                    version: p.version.clone(),
                    release: p.release.clone(),
                    arch: p.arch.clone(),
                    repo: p.source_repo.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    let config_files = snap
        .config
        .as_ref()
        .map(|cfg| {
            cfg.files
                .iter()
                .map(|f| FilterableConfig {
                    path: f.path.clone(),
                    kind: enum_to_string(&f.kind),
                })
                .collect()
        })
        .unwrap_or_default();

    let services = snap
        .services
        .as_ref()
        .map(|svc| {
            svc.state_changes
                .iter()
                .map(|s| FilterableService {
                    unit: s.unit.clone(),
                    state: enum_to_string(&s.current_state),
                })
                .collect()
        })
        .unwrap_or_default();

    let scheduled = snap
        .scheduled_tasks
        .as_ref()
        .map(|sched| {
            let mut items: Vec<FilterableScheduled> = Vec::new();

            for cj in &sched.cron_jobs {
                items.push(FilterableScheduled {
                    name: cj.path.clone(),
                    kind: "cron".to_string(),
                });
            }
            for t in &sched.systemd_timers {
                items.push(FilterableScheduled {
                    name: t.name.clone(),
                    kind: "systemd_timer".to_string(),
                });
            }
            for t in &sched.generated_timer_units {
                items.push(FilterableScheduled {
                    name: t.name.clone(),
                    kind: "generated_timer".to_string(),
                });
            }
            for a in &sched.at_jobs {
                items.push(FilterableScheduled {
                    name: a.command.clone(),
                    kind: "at".to_string(),
                });
            }

            items
        })
        .unwrap_or_default();

    let users = snap
        .users_groups
        .as_ref()
        .map(|ug| {
            ug.users
                .iter()
                .filter_map(|v| serde_json::from_value::<UserGroupDecision>(v.clone()).ok())
                .filter(|u| u.include)
                .map(|u| FilterableUser {
                    name: u.name.clone(),
                    uid: u.uid,
                })
                .collect()
        })
        .unwrap_or_default();

    ReportFilterData {
        packages,
        config_files,
        services,
        scheduled,
        users,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::completeness::{Completeness, InspectorId};

    // -----------------------------------------------------------------------
    // SectionState tests (from T1)
    // -----------------------------------------------------------------------

    #[test]
    fn section_state_normal_when_complete() {
        let c = Completeness::Complete;
        assert_eq!(section_state(InspectorId::Rpm, &c), SectionState::Normal);
    }

    #[test]
    fn section_state_degraded_when_in_degraded_list() {
        let c = Completeness::Partial {
            degraded_sections: vec![InspectorId::Config],
            reason: "test".into(),
        };
        assert_eq!(
            section_state(InspectorId::Config, &c),
            SectionState::Degraded
        );
        assert_eq!(section_state(InspectorId::Rpm, &c), SectionState::Normal);
    }

    #[test]
    fn section_state_failed_when_in_failed_list() {
        let c = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Storage],
            degraded_sections: vec![],
            reason: "timeout".into(),
        };
        assert_eq!(
            section_state(InspectorId::Storage, &c),
            SectionState::Failed
        );
        assert_eq!(section_state(InspectorId::Rpm, &c), SectionState::Normal);
    }

    #[test]
    fn section_state_prioritizes_failed_over_degraded() {
        let c = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Storage],
            degraded_sections: vec![InspectorId::Storage],
            reason: "partial failure".into(),
        };
        assert_eq!(
            section_state(InspectorId::Storage, &c),
            SectionState::Failed
        );
    }

    // -----------------------------------------------------------------------
    // script_safe_json tests
    // -----------------------------------------------------------------------

    #[test]
    fn script_safe_escapes_less_than() {
        let input = r#"{"msg":"</script>"}"#;
        let output = script_safe_json(input);
        assert!(
            !output.contains("</script>"),
            "output must not contain literal </script>"
        );
        assert!(
            output.contains("\\u003c"),
            "output must contain escaped less-than"
        );
        // Round-trip: the escaped JSON must deserialize to the original value
        let original: serde_json::Value = serde_json::from_str(input).unwrap();
        let roundtrip: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(original, roundtrip);
    }

    #[test]
    fn script_safe_escapes_greater_than() {
        let input = r#"{"v":"a>b"}"#;
        let output = script_safe_json(input);
        assert!(
            !output.contains('>'),
            "output must not contain literal >"
        );
        assert!(
            output.contains("\\u003e"),
            "output must contain escaped greater-than"
        );
    }

    #[test]
    fn script_safe_escapes_html_comment() {
        let input = r#"{"v":"<!-- comment -->"}"#;
        let output = script_safe_json(input);
        assert!(
            !output.contains("<!--"),
            "output must not contain literal HTML comment open"
        );
    }

    #[test]
    fn script_safe_preserves_non_special_content() {
        let input = r#"{"name":"hello","count":42}"#;
        let output = script_safe_json(input);
        assert_eq!(input, output, "non-special content must pass through unchanged");
    }

    // -----------------------------------------------------------------------
    // ReportFilterData / build_filter_data tests
    // -----------------------------------------------------------------------

    #[test]
    fn filter_data_excludes_secrets() {
        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(inspectah_core::types::users::UserGroupSection {
            users: vec![serde_json::json!({
                "name": "admin",
                "uid": 1000,
                "gid": 1000,
                "shell": "/bin/bash",
                "home": "/home/admin",
                "include": true,
                "classification": "interactive",
                "containerfile_strategy": "useradd",
                "password_choice": "none",
                "password_hash": "$6$secret$hash",
                "ssh_keys": ["ssh-rsa AAAA..."],
                "ssh_key_count": 1
            })],
            groups: vec![],
            sudoers_rules: vec![],
            ssh_authorized_keys_refs: vec![],
            passwd_entries: vec![],
            shadow_entries: vec![],
            group_entries: vec![],
            gshadow_entries: vec![],
            subuid_entries: vec![],
            subgid_entries: vec![],
        });

        let filter = build_filter_data(&snap);
        let json = serde_json::to_string(&filter).expect("serialize filter data");

        assert!(
            !json.contains("password_hash"),
            "DTO must not contain password_hash"
        );
        assert!(
            !json.contains("ssh_keys"),
            "DTO must not contain ssh_keys"
        );
        assert!(
            !json.contains("$6$secret$hash"),
            "DTO must not contain actual hash value"
        );
        assert!(
            !json.contains("ssh-rsa"),
            "DTO must not contain actual SSH key"
        );

        // But the user name and uid should be present
        assert_eq!(filter.users.len(), 1);
        assert_eq!(filter.users[0].name, "admin");
        assert_eq!(filter.users[0].uid, 1000);
    }
}
