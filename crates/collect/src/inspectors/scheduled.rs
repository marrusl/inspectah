use super::units::install::{DropInRoot, DropInSource, drop_in_dirs, selected_drop_ins};
use super::units::links::read_host_links;
use super::units::specifiers::{SpecifierSet, expand_unit_specifiers};
use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput, RpmState,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::FindingKind;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::redaction::{Confidence, RedactionHint};
use inspectah_core::types::scheduled::{
    AtJob, CronJob, GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer, TimerTrigger,
};
use inspectah_core::types::units::{UnitName, systemd_reads_drop_in};
use inspectah_core::types::warnings::Warning;
use std::collections::HashMap;
use std::path::Path;

/// Secret-like patterns in cron/at commands that trigger redaction hints.
const SECRET_PATTERNS: &[&str] = &["password", "secret", "token", "key", "credential"];

/// Inspects scheduled tasks: cron directories, system/user crontabs,
/// existing systemd timers, at jobs, and generates timer units from cron entries.
pub struct ScheduledTasksInspector;

impl ScheduledTasksInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ScheduledTasksInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector for ScheduledTasksInspector {
    fn id(&self) -> InspectorId {
        InspectorId::ScheduledTasks
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        let rpm_state = match ctx.rpm_state {
            None => {
                return Err(InspectorError::Failed {
                    reason: "RPM prerequisite unavailable".into(),
                });
            }
            Some(state) => state,
        };

        let exec = ctx.executor;
        let mut warnings: Vec<Warning> = Vec::new();
        let mut hints: Vec<RedactionHint> = Vec::new();
        let mut degraded_reasons: Vec<String> = Vec::new();

        let mut section = ScheduledTaskSection {
            cron_jobs: Vec::new(),
            systemd_timers: Vec::new(),
            at_jobs: Vec::new(),
            generated_timer_units: Vec::new(),
        };

        // --- Cron ---
        scan_cron_dir(
            exec,
            &mut section,
            "/etc/cron.d",
            "cron.d",
            Some(rpm_state),
            &mut hints,
            &mut degraded_reasons,
        );
        scan_cron_file(
            exec,
            &mut section,
            "/etc/crontab",
            "crontab",
            Some(rpm_state),
            &mut hints,
        );

        for period in &["hourly", "daily", "weekly", "monthly"] {
            scan_cron_period_dir(
                exec,
                &mut section,
                period,
                Some(rpm_state),
                &mut degraded_reasons,
            );
        }

        // User crontabs
        scan_cron_dir(
            exec,
            &mut section,
            "/var/spool/cron",
            "spool/cron",
            None,
            &mut hints,
            &mut degraded_reasons,
        );

        // --- Existing systemd timers ---
        let local_listed = scan_systemd_timers(
            exec,
            &mut section,
            "etc/systemd/system",
            "local",
            &mut degraded_reasons,
        );
        scan_systemd_timers(
            exec,
            &mut section,
            "usr/lib/systemd/system",
            "vendor",
            &mut degraded_reasons,
        );
        // A local timer file the listing left out loads over the vendor
        // one, so a vendor description's trigger may not be the unit the
        // timer starts.
        if !local_listed {
            for timer in section
                .systemd_timers
                .iter_mut()
                .filter(|t| t.source == "vendor")
            {
                timer.trigger = TimerTrigger::Unresolved;
            }
        }
        scan_instance_timers(exec, &mut section);

        // --- At jobs ---
        scan_at_jobs(exec, &mut section, &mut hints, &mut degraded_reasons);

        // Emit redaction hints for timer ExecStart fields
        for timer in &section.systemd_timers {
            check_command_redaction(&timer.exec_start, &timer.path, &mut hints);
        }

        // Emit metric for progress rendering
        progress.emit(inspectah_core::types::progress::ProgressEvent::Metric {
            inspector: InspectorId::ScheduledTasks,
            kind: inspectah_core::types::progress::MetricKind::TimersFound,
            value: section.systemd_timers.len(),
        });

        if !warnings.is_empty() || !degraded_reasons.is_empty() {
            // Add degraded file count warning if applicable
            if !degraded_reasons.is_empty() {
                warnings.push(Warning {
                    inspector: "scheduled_tasks".into(),
                    message: format!("degraded: {}", degraded_reasons.join("; ")),
                    ..Default::default()
                });
            }
        }

        let output = InspectorOutput {
            section: SectionData::ScheduledTasks(section),
            warnings,
            redaction_hints: hints,
        };

        if degraded_reasons.is_empty() {
            Ok(output)
        } else {
            Err(InspectorError::Degraded {
                partial: Box::new(output),
                reason: degraded_reasons.join("; "),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Cron scanning
// ---------------------------------------------------------------------------

/// Scans a directory for cron job files.
fn scan_cron_dir(
    exec: &dyn Executor,
    section: &mut ScheduledTaskSection,
    dir_path: &str,
    source: &str,
    rpm_state: Option<&RpmState>,
    hints: &mut Vec<RedactionHint>,
    degraded_reasons: &mut Vec<String>,
) {
    let entries = match exec.read_dir(Path::new(dir_path)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            degraded_reasons.push(format!("permission denied reading {dir_path}"));
            return;
        }
        Err(_) => return,
    };

    for entry_name in &entries {
        if entry_name.starts_with('.') {
            continue;
        }
        let file_path = format!("{dir_path}/{entry_name}");
        let is_rpm_owned = rpm_state
            .map(|s| s.is_rpm_owned(Path::new(&file_path)))
            .unwrap_or(false);

        // For spool/cron, include user name in source
        let cron_source = if source == "spool/cron" {
            format!("spool/cron ({entry_name})")
        } else {
            source.to_string()
        };

        section.cron_jobs.push(CronJob {
            path: strip_leading_slash(&file_path),
            source: cron_source.clone(),
            rpm_owned: is_rpm_owned,
            disposition: FindingKind::included(),
            ..Default::default()
        });

        if is_rpm_owned {
            continue;
        }

        let content = match exec.read_file(Path::new(&file_path)) {
            Ok(c) => c,
            Err(_) => continue,
        };

        parse_cron_entries(
            section,
            &content,
            &file_path,
            &cron_source,
            entry_name,
            hints,
        );
    }
}

/// Scans a single cron file (e.g., /etc/crontab).
fn scan_cron_file(
    exec: &dyn Executor,
    section: &mut ScheduledTaskSection,
    file_path: &str,
    source: &str,
    rpm_state: Option<&RpmState>,
    hints: &mut Vec<RedactionHint>,
) {
    if !exec.file_exists(Path::new(file_path)) {
        return;
    }

    let is_rpm_owned = rpm_state
        .map(|s| s.is_rpm_owned(Path::new(file_path)))
        .unwrap_or(false);

    section.cron_jobs.push(CronJob {
        path: strip_leading_slash(file_path),
        source: source.into(),
        rpm_owned: is_rpm_owned,
        disposition: FindingKind::included(),
        ..Default::default()
    });

    let content = match exec.read_file(Path::new(file_path)) {
        Ok(c) => c,
        Err(_) => return,
    };

    let name = file_path.rsplit('/').next().unwrap_or("crontab");

    parse_cron_entries(section, &content, file_path, source, name, hints);
}

/// Returns true if a line looks like a cron entry (starts with digit or *).
fn is_cron_line(line: &str) -> bool {
    line.starts_with(|c: char| c.is_ascii_digit() || c == '*' || c == '@')
}

/// Extracts cron expressions from file content and generates timer units.
fn parse_cron_entries(
    section: &mut ScheduledTaskSection,
    content: &str,
    file_path: &str,
    source: &str,
    file_name: &str,
    hints: &mut Vec<RedactionHint>,
) {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Handle @shortcuts in user crontabs
        if line.starts_with('@') {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let shortcut = parts[0];
                let command = if source == "cron.d" || source == "crontab" {
                    // System crontabs have a user field after the shortcut
                    if parts.len() > 2 {
                        parts[2..].join(" ")
                    } else {
                        String::new()
                    }
                } else {
                    parts[1..].join(" ")
                };

                let safe_name = format!("cron-{}", file_name.replace('.', "-"));
                let rel_path = strip_leading_slash(file_path);

                check_command_redaction(&command, file_path, hints);

                // @reboot has no calendar equivalent — generate an advisory
                // oneshot service for manual follow-up, not a fake timer.
                if shortcut.eq_ignore_ascii_case("@reboot") {
                    let (service_content, timer_content) =
                        make_reboot_advisory(&safe_name, &rel_path, &command);
                    section.generated_timer_units.push(GeneratedTimerUnit {
                        name: safe_name,
                        timer_content,
                        service_content,
                        cron_expr: "@reboot".into(),
                        source_path: rel_path,
                        command,
                        disposition: FindingKind::included(),
                        ..Default::default()
                    });
                } else {
                    let (timer_content, service_content) =
                        make_timer_service(&safe_name, shortcut, &rel_path, &command);
                    section.generated_timer_units.push(GeneratedTimerUnit {
                        name: safe_name,
                        timer_content,
                        service_content,
                        cron_expr: shortcut.into(),
                        source_path: rel_path,
                        command,
                        disposition: FindingKind::included(),
                        ..Default::default()
                    });
                }
            }
            continue;
        }

        if !is_cron_line(line) {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }

        let cron_expr = parts[..5].join(" ");

        // System crontabs (cron.d, crontab) have a user field at position 5
        let command = if source == "cron.d" || source == "crontab" {
            if parts.len() > 6 {
                parts[6..].join(" ")
            } else {
                String::new()
            }
        } else {
            parts[5..].join(" ")
        };

        let safe_name = format!("cron-{}", file_name.replace('.', "-"));
        let rel_path = strip_leading_slash(file_path);

        let (timer_content, service_content) =
            make_timer_service(&safe_name, &cron_expr, &rel_path, &command);

        check_command_redaction(&command, file_path, hints);

        section.generated_timer_units.push(GeneratedTimerUnit {
            name: safe_name,
            timer_content,
            service_content,
            cron_expr,
            source_path: rel_path,
            command,
            disposition: FindingKind::included(),
            ..Default::default()
        });
    }
}

// ---------------------------------------------------------------------------
// Cron period directories (hourly, daily, weekly, monthly)
// ---------------------------------------------------------------------------

/// Maps cron period names to systemd OnCalendar values.
fn period_schedule(period: &str) -> &str {
    match period {
        "hourly" => "*-*-* *:01:00",
        "daily" => "*-*-* 03:00:00",
        "weekly" => "Mon *-*-* 03:00:00",
        "monthly" => "*-*-01 03:00:00",
        _ => "*-*-* 03:00:00",
    }
}

/// Scans a cron.{period} directory and generates timer units.
fn scan_cron_period_dir(
    exec: &dyn Executor,
    section: &mut ScheduledTaskSection,
    period: &str,
    rpm_state: Option<&RpmState>,
    degraded_reasons: &mut Vec<String>,
) {
    let dir_path = format!("/etc/cron.{period}");
    let entries = match exec.read_dir(Path::new(&dir_path)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            degraded_reasons.push(format!("permission denied reading {dir_path}"));
            return;
        }
        Err(_) => return,
    };

    let on_calendar = period_schedule(period);

    for entry_name in &entries {
        if entry_name.starts_with('.') {
            continue;
        }

        let file_path = format!("{dir_path}/{entry_name}");
        let rel_path = strip_leading_slash(&file_path);
        let is_rpm_owned = rpm_state
            .map(|s| s.is_rpm_owned(Path::new(&file_path)))
            .unwrap_or(false);

        section.cron_jobs.push(CronJob {
            path: rel_path.clone(),
            source: format!("cron.{period}"),
            rpm_owned: is_rpm_owned,
            disposition: FindingKind::included(),
            ..Default::default()
        });

        if is_rpm_owned {
            continue;
        }

        let safe_name = format!("cron-{period}-{}", entry_name.replace('.', "-"));
        let command = format!("/{rel_path}");

        let timer_content = format!(
            "[Unit]\nDescription=Generated from cron.{period}: {rel_path}\n\
             # Original: cron.{period} script\n\n\
             [Timer]\nOnCalendar={on_calendar}\nPersistent=true\n\n\
             [Install]\nWantedBy=timers.target\n"
        );

        let service_content = format!(
            "[Unit]\nDescription=Timer from cron.{period} {rel_path}\n\n\
             [Service]\nType=oneshot\nExecStart={command}\n"
        );

        section.generated_timer_units.push(GeneratedTimerUnit {
            name: safe_name,
            timer_content,
            service_content,
            cron_expr: format!("@{period}"),
            source_path: rel_path,
            command,
            disposition: FindingKind::included(),
            ..Default::default()
        });
    }
}

// ---------------------------------------------------------------------------
// Cron -> systemd OnCalendar conversion
// ---------------------------------------------------------------------------

/// Maps cron month abbreviations to numeric values.
fn month_names() -> HashMap<&'static str, &'static str> {
    [
        ("jan", "1"),
        ("feb", "2"),
        ("mar", "3"),
        ("apr", "4"),
        ("may", "5"),
        ("jun", "6"),
        ("jul", "7"),
        ("aug", "8"),
        ("sep", "9"),
        ("oct", "10"),
        ("nov", "11"),
        ("dec", "12"),
    ]
    .into_iter()
    .collect()
}

/// Maps cron day-of-week abbreviations to systemd names.
fn dow_names_to_systemd() -> HashMap<&'static str, &'static str> {
    [
        ("sun", "Sun"),
        ("mon", "Mon"),
        ("tue", "Tue"),
        ("wed", "Wed"),
        ("thu", "Thu"),
        ("fri", "Fri"),
        ("sat", "Sat"),
    ]
    .into_iter()
    .collect()
}

/// Maps numeric day-of-week values to systemd names.
fn dow_numeric() -> HashMap<&'static str, &'static str> {
    [
        ("0", "Sun"),
        ("1", "Mon"),
        ("2", "Tue"),
        ("3", "Wed"),
        ("4", "Thu"),
        ("5", "Fri"),
        ("6", "Sat"),
        ("7", "Sun"),
    ]
    .into_iter()
    .collect()
}

/// Normalises a single cron name/number to its canonical form.
fn normalise_cron_token(token: &str, kind: &str) -> String {
    let low = token.to_lowercase();
    if kind == "month"
        && let Some(v) = month_names().get(low.as_str())
    {
        return v.to_string();
    }
    if kind == "dow" {
        if let Some(v) = dow_names_to_systemd().get(low.as_str()) {
            return v.to_string();
        }
        if let Some(v) = dow_numeric().get(token) {
            return v.to_string();
        }
    }
    token.to_string()
}

/// Returns true if the string consists only of ASCII digits.
fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// Converts a single cron field to its systemd OnCalendar equivalent.
fn cron_field_to_calendar(field: &str, kind: &str) -> String {
    if field == "*" {
        return "*".to_string();
    }

    // Step values: */5
    if let Some(step) = field.strip_prefix("*/") {
        if is_digits(step) {
            return match kind {
                "minute" => format!("*/{step}"),
                "hour" => format!("00/{step}"),
                _ => field.to_string(),
            };
        }
        return field.to_string();
    }

    // Range+step: 1-10/2 -> 1..10/2
    if field.contains('-')
        && field.contains('/')
        && let Some((range_part, step)) = field.split_once('/')
        && let Some((lo_raw, hi_raw)) = range_part.split_once('-')
    {
        let lo = normalise_cron_token(lo_raw.trim(), kind);
        let hi = normalise_cron_token(hi_raw.trim(), kind);
        return format!("{lo}..{hi}/{step}");
    }

    // Ranges: 1-5 -> 1..5
    if field.contains('-')
        && let Some((lo_raw, hi_raw)) = field.split_once('-')
    {
        let lo = normalise_cron_token(lo_raw.trim(), kind);
        let hi = normalise_cron_token(hi_raw.trim(), kind);
        return format!("{lo}..{hi}");
    }

    // Lists: 1,3,5
    if field.contains(',') {
        let elems: Vec<String> = field
            .split(',')
            .map(|e| normalise_cron_token(e.trim(), kind))
            .collect();
        return elems.join(",");
    }

    // Named or numeric tokens
    let normalised = normalise_cron_token(field, kind);
    if normalised != field {
        return normalised;
    }

    // Plain digit: zero-pad for minute/hour
    if is_digits(field)
        && (kind == "minute" || kind == "hour")
        && let Ok(n) = field.parse::<u32>()
    {
        return format!("{n:02}");
    }

    field.to_string()
}

/// Converts a cron expression to systemd OnCalendar format.
/// Returns (calendar_spec, converted). When converted is false, the expression
/// could not be fully translated.
pub fn cron_to_on_calendar(cron_expr: &str) -> (String, bool) {
    let expr = cron_expr.trim();

    // Named shortcuts
    let shortcuts: HashMap<&str, (&str, bool)> = [
        ("@yearly", ("*-01-01 00:00:00", true)),
        ("@annually", ("*-01-01 00:00:00", true)),
        ("@monthly", ("*-*-01 00:00:00", true)),
        ("@weekly", ("Mon *-*-* 00:00:00", true)),
        ("@daily", ("*-*-* 00:00:00", true)),
        ("@midnight", ("*-*-* 00:00:00", true)),
        ("@hourly", ("*-*-* *:00:00", true)),
    ]
    .into_iter()
    .collect();

    let lower = expr.to_lowercase();
    if let Some(&(cal, conv)) = shortcuts.get(lower.as_str()) {
        return (cal.to_string(), conv);
    }

    // @reboot has no calendar equivalent
    if lower == "@reboot" {
        return ("@reboot".to_string(), false);
    }

    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() < 5 {
        return ("*-*-* 02:00:00".to_string(), false);
    }

    let (minute, hour, dom, month, dow) = (parts[0], parts[1], parts[2], parts[3], parts[4]);

    let cal_min = cron_field_to_calendar(minute, "minute");
    let cal_hour = cron_field_to_calendar(hour, "hour");
    let cal_dom = cron_field_to_calendar(dom, "dom");
    let cal_month = cron_field_to_calendar(month, "month");
    let cal_dow = cron_field_to_calendar(dow, "dow");

    let date_part = format!("*-{cal_month}-{cal_dom}");
    let time_part = format!("{cal_hour}:{cal_min}:00");

    if cal_dow != "*" {
        (format!("{cal_dow} {date_part} {time_part}"), true)
    } else {
        (format!("{date_part} {time_part}"), true)
    }
}

// ---------------------------------------------------------------------------
// Timer unit generation
// ---------------------------------------------------------------------------

/// Generates systemd .timer and .service unit content from a cron expression.
fn make_timer_service(_name: &str, cron_expr: &str, path: &str, command: &str) -> (String, String) {
    let (on_calendar, converted) = cron_to_on_calendar(cron_expr);

    let (fixme_lines, final_calendar) = if !converted {
        if on_calendar == "@reboot" {
            (
                "# FIXME: @reboot has no OnCalendar equivalent.\n\
                 # Use a oneshot service with WantedBy=multi-user.target instead.\n"
                    .to_string(),
                "*-*-* 02:00:00".to_string(),
            )
        } else {
            (
                format!(
                    "# FIXME: cron expression '{}' could not be fully converted.\n\
                     # Review and correct the OnCalendar value below.\n",
                    cron_expr
                ),
                on_calendar,
            )
        }
    } else {
        (String::new(), on_calendar)
    };

    let timer_content = format!(
        "[Unit]\nDescription=Generated from cron: {path}\n\
         # Original cron: {cron_expr}\n\
         {fixme_lines}\n\
         [Timer]\nOnCalendar={final_calendar}\nPersistent=true\n\n\
         [Install]\nWantedBy=timers.target\n"
    );

    let exec_line = if command.is_empty() {
        "ExecStart=/bin/true\n# FIXME: could not extract command from cron entry".to_string()
    } else {
        format!("ExecStart={command}")
    };

    let service_content = format!(
        "[Unit]\nDescription=Timer from cron {path}\n\n\
         [Service]\nType=oneshot\n{exec_line}\n"
    );

    (timer_content, service_content)
}

/// Generates advisory content for `@reboot` cron entries.
///
/// Unlike `make_timer_service`, this does NOT produce a timer unit with a
/// fake OnCalendar value. Instead it produces a oneshot service activated
/// by `WantedBy=multi-user.target` and an empty timer_content, flagging
/// the entry as requiring manual operator follow-up.
///
/// Returns `(service_content, timer_content)` — note: timer_content is empty.
fn make_reboot_advisory(_name: &str, path: &str, command: &str) -> (String, String) {
    let exec_line = if command.is_empty() {
        "ExecStart=/bin/true\n# FIXME: could not extract command from cron entry".to_string()
    } else {
        format!("ExecStart={command}")
    };

    let service_content = format!(
        "[Unit]\n\
         Description=Boot-triggered task from cron: {path}\n\
         # Original: @reboot {command}\n\
         # FIXME: @reboot has no OnCalendar equivalent.\n\
         # This service runs once at boot via WantedBy=multi-user.target.\n\
         # Review whether this behavior is still needed in image mode.\n\n\
         [Service]\nType=oneshot\n{exec_line}\n\n\
         [Install]\nWantedBy=multi-user.target\n"
    );

    // No timer unit — @reboot is not a calendar schedule
    let timer_content = String::new();

    (service_content, timer_content)
}

// ---------------------------------------------------------------------------
// Systemd timer scanning
// ---------------------------------------------------------------------------

/// Extracts the first value of `field=` from unit file text.
fn parse_unit_field(text: &str, field: &str) -> String {
    let prefix = format!("{field}=");
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix(&prefix) {
            return value.trim().to_string();
        }
    }
    String::new()
}

/// Scans a systemd unit directory for .timer files and their paired
/// .service units, and describes each timer from the two files: its
/// schedule, its command and its description. The files themselves are
/// not copied: a local one is a config entry. Returns whether the
/// directory was listed in full: `false` when its listing failed, at once
/// or part way, which is reported through `degraded_reasons`.
fn scan_systemd_timers(
    exec: &dyn Executor,
    section: &mut ScheduledTaskSection,
    base_dir: &str,
    source_label: &str,
    degraded_reasons: &mut Vec<String>,
) -> bool {
    let dir_path = format!("/{base_dir}");
    // Strict listing: a partial one may leave out a timer file, whose
    // trigger is then not recorded.
    let entries = match exec.read_dir_strict(Path::new(&dir_path)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            degraded_reasons.push(format!("permission denied reading {dir_path}"));
            return false;
        }
        Err(e) => {
            degraded_reasons.push(format!("{dir_path}: {e}"));
            return false;
        }
    };

    for entry_name in &entries {
        if !entry_name.ends_with(".timer") {
            continue;
        }

        let timer_path = format!("{dir_path}/{entry_name}");
        let timer_text = match exec.read_file(Path::new(&timer_path)) {
            Ok(t) if !t.is_empty() => t,
            _ => continue,
        };

        let name = entry_name.strip_suffix(".timer").unwrap_or(entry_name);
        let on_calendar = parse_unit_field(&timer_text, "OnCalendar");
        let description = parse_unit_field(&timer_text, "Description");

        let service_path = format!("{dir_path}/{name}.service");
        let service_text = if exec.file_exists(Path::new(&service_path)) {
            exec.read_file(Path::new(&service_path)).unwrap_or_default()
        } else {
            String::new()
        };
        let exec_start = parse_unit_field(&service_text, "ExecStart");
        let (trigger, trigger_drop_ins) = timer_trigger(exec, name, &timer_text);

        section.systemd_timers.push(SystemdTimer {
            name: name.to_string(),
            on_calendar,
            exec_start,
            description,
            source: source_label.to_string(),
            path: strip_leading_slash(&timer_path),
            trigger,
            trigger_drop_ins,
            ..Default::default()
        });
    }
    true
}

/// The unit directories a timer's drop-ins are read from, with the root
/// each is ranked as: of two drop-ins of one file name in one directory
/// name, the administrator's overrides the vendor one.
const TIMER_DROP_IN_ROOTS: [(DropInRoot, &str); 2] = [
    (DropInRoot::Etc, ETC_TIMER_DROP_IN_DIR),
    (DropInRoot::Vendor, VENDOR_TIMER_DIR),
];

/// The administrator's unit directory, whose drop-ins setting `Unit=` are
/// recorded as the trigger's evidence.
const ETC_TIMER_DROP_IN_DIR: &str = "/etc/systemd/system";

/// The directory packages install unit files into.
const VENDOR_TIMER_DIR: &str = "/usr/lib/systemd/system";

/// The suffix of a timer unit's name.
const TIMER_SUFFIX: &str = ".timer";

/// The suffix of a service unit's name.
const SERVICE_SUFFIX: &str = ".service";

/// Describes each instance timer linked under `/etc/systemd/system` that
/// has no description yet (§ 8). An instance timer has no timer file of
/// its own, so it is described from its template's file: the one under
/// `/etc` as `local` and the vendor one as `vendor`, each that exists, as
/// a plain timer is described from each of its files. The command is read
/// from the instance's own service file beside the template when there is
/// one, else from the template service. The links are the ones the units
/// inspector finds instances by ([`read_host_links`]); a failed read there
/// is the units inspector's to report, and an instance it hides is left
/// undescribed, which rule 5 reads as an unresolved trigger.
fn scan_instance_timers(exec: &dyn Executor, section: &mut ScheduledTaskSection) {
    let (host_links, _failures) = read_host_links(exec);
    for unit in host_links.admin.keys() {
        let Some(parts) = unit.instance() else {
            continue;
        };
        let Some(name) = unit.as_str().strip_suffix(TIMER_SUFFIX) else {
            continue;
        };
        if section.systemd_timers.iter().any(|t| t.name == name) {
            continue;
        }
        let template = parts.template();
        let template_service = format!("{}@{SERVICE_SUFFIX}", parts.prefix());
        for (dir, source_label) in [
            (ETC_TIMER_DROP_IN_DIR, "local"),
            (VENDOR_TIMER_DIR, "vendor"),
        ] {
            let template_path = format!("{dir}/{}", template.as_str());
            let timer_text = match exec.read_file(Path::new(&template_path)) {
                Ok(text) if !text.is_empty() => text,
                _ => continue,
            };
            let service_text = [format!("{name}{SERVICE_SUFFIX}"), template_service.clone()]
                .iter()
                .map(|service| format!("{dir}/{service}"))
                .find(|path| exec.file_exists(Path::new(path)))
                .map(|path| exec.read_file(Path::new(&path)).unwrap_or_default())
                .unwrap_or_default();
            let (trigger, trigger_drop_ins) = timer_trigger(exec, name, &timer_text);
            section.systemd_timers.push(SystemdTimer {
                name: name.to_owned(),
                on_calendar: parse_unit_field(&timer_text, "OnCalendar"),
                exec_start: parse_unit_field(&service_text, "ExecStart"),
                description: parse_unit_field(&timer_text, "Description"),
                source: source_label.to_owned(),
                path: strip_leading_slash(&template_path),
                trigger,
                trigger_drop_ins,
                ..Default::default()
            });
        }
    }
}

/// Every `Unit=` value assigned in the `[Timer]` section of `content`, in
/// order, read by logical line as systemd reads keys.
fn timer_unit_assignments(content: &str) -> Vec<String> {
    use inspectah_core::types::units::{UNIT_FILE_WHITESPACE, logical_lines};
    let mut in_timer = false;
    let mut values = Vec::new();
    for line in logical_lines(content) {
        let line = line.trim_matches(UNIT_FILE_WHITESPACE);
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_timer = name == "Timer";
            continue;
        }
        if !in_timer {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim_matches(UNIT_FILE_WHITESPACE) == "Unit"
        {
            values.push(value.trim_matches(UNIT_FILE_WHITESPACE).to_owned());
        }
    }
    values
}

/// The trigger of the timer `name` whose own file reads `timer_text` (for
/// an instance timer, its template's file), and the `/etc` drop-ins that
/// assign its `Unit=` (§ 8). Drop-ins are the `.conf` files of
/// `<name>.timer.d` and, for an instance timer, of its template's
/// `<prefix>@.timer.d`, under each of [`TIMER_DROP_IN_ROOTS`], a hidden
/// name skipped ([`systemd_reads_drop_in`]), applied
/// after the file in the order `[Install]` is read in
/// ([`selected_drop_ins`]): file-name order across every directory, and of
/// the files of one name the first in the instance's own directory before
/// the template's, `/etc` before the vendor directory within each. The
/// last `Unit=` wins and an empty one resets it. With none, the trigger is
/// `<name>.service`. A value with a specifier the reader does not expand,
/// or that is not a unit name, is [`TimerTrigger::Unresolved`], as is the
/// trigger of a timer whose name is not a unit name, and of a timer whose
/// drop-in directory cannot be listed in full or whose drop-in cannot be
/// read: the unit it starts is not known.
fn timer_trigger(exec: &dyn Executor, name: &str, timer_text: &str) -> (TimerTrigger, Vec<String>) {
    let Ok(unit) = UnitName::try_from(format!("{name}{TIMER_SUFFIX}")) else {
        return (TimerTrigger::Unresolved, Vec::new());
    };
    // Each drop-in's source, with its path standing in for its text: the
    // selection reads only the sources, so what it returns is the paths
    // in load order.
    let mut found: Vec<(DropInSource, String)> = Vec::new();
    for (root, base) in TIMER_DROP_IN_ROOTS {
        for dir in drop_in_dirs(&unit) {
            let dir_path = format!("{base}/{dir}");
            // A listing that fails, or fails part way, may hide the
            // drop-in that sets `Unit=`: the trigger is then unknown.
            let entries = match exec.read_dir_strict(Path::new(&dir_path)) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return (TimerTrigger::Unresolved, Vec::new()),
            };
            // A hidden name is skipped, as systemd skips it: the units
            // inspector's listing uses the same rule.
            for entry in entries.into_iter().filter(|e| systemd_reads_drop_in(e)) {
                let path = format!("{dir_path}/{entry}");
                let source = DropInSource {
                    root,
                    dir: dir.clone(),
                    file_name: entry,
                };
                found.push((source, path));
            }
        }
    }
    let sources: Vec<(DropInSource, &str)> = found
        .iter()
        .map(|(source, path)| (source.clone(), path.as_str()))
        .collect();
    let mut assigned: Option<String> = None;
    for value in timer_unit_assignments(timer_text) {
        assigned = (!value.is_empty()).then_some(value);
    }
    let mut evidence = Vec::new();
    for path in selected_drop_ins(&unit, &sources) {
        let Ok(content) = exec.read_file(Path::new(path)) else {
            return (TimerTrigger::Unresolved, Vec::new());
        };
        let values = timer_unit_assignments(&content);
        if values.is_empty() {
            continue;
        }
        if path.starts_with(&format!("{ETC_TIMER_DROP_IN_DIR}/")) {
            evidence.push(path.to_owned());
        }
        for value in values {
            assigned = (!value.is_empty()).then_some(value);
        }
    }
    let unit = match assigned {
        None => Some(format!("{name}{SERVICE_SUFFIX}")),
        Some(value) => expand_unit_specifiers(&value, name, SpecifierSet::Runtime),
    };
    let trigger = unit
        .and_then(|unit| UnitName::try_from(unit).ok())
        .map_or(TimerTrigger::Unresolved, TimerTrigger::Unit);
    (trigger, evidence)
}

// ---------------------------------------------------------------------------
// At job scanning
// ---------------------------------------------------------------------------

/// Returns true for at-spool preamble lines that should be skipped.
fn is_preamble_line(line: &str) -> bool {
    if line.is_empty() || line == "}" {
        return true;
    }
    if line.starts_with("#!/") || line.starts_with('#') {
        return true;
    }
    if line.starts_with("umask") {
        return true;
    }
    if line.starts_with("cd ") {
        return true;
    }
    if line.contains("export") {
        return true;
    }
    if line.starts_with("SHELL=") {
        return true;
    }
    if line.starts_with("echo") && line.contains("inaccessible") {
        return true;
    }
    if line.starts_with("exit") {
        return true;
    }
    false
}

/// Parses an at spool file to extract command, user, and working dir.
fn parse_at_job(content: &str, rel_path: &str) -> AtJob {
    if content.is_empty() {
        return AtJob {
            file: rel_path.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        };
    }

    let mut user = String::new();
    let mut working_dir = String::new();
    let mut cmd_lines: Vec<String> = Vec::new();
    let mut in_preamble = true;

    for line in content.lines() {
        let stripped = line.trim();

        // Extract uid from "# atrun uid=NNNN"
        if stripped.starts_with("# atrun uid=")
            && let Some(uid_part) = stripped.strip_prefix("# atrun uid=")
        {
            let uid = uid_part.split_whitespace().next().unwrap_or("");
            if !uid.is_empty() {
                user = format!("uid={uid}");
            }
        }

        // Extract user from "# mail username N"
        if stripped.starts_with("# mail ") {
            let parts: Vec<&str> = stripped.split_whitespace().collect();
            if parts.len() >= 3 {
                user = parts[2].to_string();
            }
        }

        // Extract working dir from "cd /path || {"
        if stripped.starts_with("cd ") && in_preamble {
            let fields: Vec<&str> = stripped.split_whitespace().collect();
            if fields.len() > 1 {
                let mut wd = fields[1].trim_end_matches('|').to_string();
                if let Some(idx) = wd.find("||") {
                    wd = wd[..idx].trim().to_string();
                }
                working_dir = wd;
            }
            continue;
        }

        if in_preamble && is_preamble_line(stripped) {
            continue;
        }
        in_preamble = false;
        if !stripped.is_empty() {
            cmd_lines.push(stripped.to_string());
        }
    }

    let command = cmd_lines.join("; ");
    AtJob {
        file: rel_path.into(),
        command,
        user,
        working_dir,
        disposition: FindingKind::included(),
        ..Default::default()
    }
}

/// Scans /var/spool/at for at job files.
fn scan_at_jobs(
    exec: &dyn Executor,
    section: &mut ScheduledTaskSection,
    hints: &mut Vec<RedactionHint>,
    degraded_reasons: &mut Vec<String>,
) {
    let entries = match exec.read_dir(Path::new("/var/spool/at")) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            degraded_reasons.push("permission denied reading /var/spool/at".into());
            return;
        }
        Err(_) => return,
    };

    for entry_name in &entries {
        if entry_name.starts_with('.') {
            continue;
        }
        let file_path = format!("/var/spool/at/{entry_name}");
        let content = exec.read_file(Path::new(&file_path)).unwrap_or_default();
        let rel_path = strip_leading_slash(&file_path);
        let job = parse_at_job(&content, &rel_path);

        check_command_redaction(&job.command, &file_path, hints);

        section.at_jobs.push(job);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Strips the leading `/` from a path string.
fn strip_leading_slash(path: &str) -> String {
    path.strip_prefix('/').unwrap_or(path).to_string()
}

/// Checks a command string for secret-like patterns and emits redaction hints.
fn check_command_redaction(command: &str, path: &str, hints: &mut Vec<RedactionHint>) {
    if command.is_empty() {
        return;
    }
    let lower = command.to_lowercase();
    for pattern in SECRET_PATTERNS {
        if lower.contains(pattern) {
            hints.push(RedactionHint {
                path: path.to_string(),
                reason: format!("command may contain credentials (matched '{pattern}')"),
                confidence: Some(Confidence::Medium),
            });
            return; // One hint per command is enough
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::progress::NullProgress;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::system::SourceSystem;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn test_os_release() -> OsRelease {
        OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        }
    }

    fn test_source_system() -> SourceSystem {
        SourceSystem::PackageBased {
            os_release: test_os_release(),
        }
    }

    fn empty_rpm_state() -> RpmState {
        RpmState::default()
    }

    fn rpm_state_with_owned(paths: Vec<&str>) -> RpmState {
        let mut owned = HashSet::new();
        for p in &paths {
            owned.insert(PathBuf::from(p));
        }
        RpmState {
            owned_paths: owned,
            ..Default::default()
        }
    }

    // ---- Test 1: test_scan_cron_d_entries ----

    #[test]
    fn test_scan_cron_d_entries() {
        let exec = MockExecutor::new()
            .with_dir("/etc/cron.d", vec!["custom-backup", "logrotate"])
            .with_file(
                "/etc/cron.d/custom-backup",
                "# Custom backup job - not RPM-owned\n30 2 * * * root /opt/backup.sh\n",
            )
            .with_file(
                "/etc/cron.d/logrotate",
                "# Run logrotate daily at 3am\n0 3 * * * root /usr/sbin/logrotate /etc/logrotate.conf\n",
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::ScheduledTasks(ref section) = output.section {
            // Two cron jobs from /etc/cron.d
            let cron_d_jobs: Vec<_> = section
                .cron_jobs
                .iter()
                .filter(|j| j.source == "cron.d")
                .collect();
            assert_eq!(cron_d_jobs.len(), 2);
            assert_eq!(cron_d_jobs[0].path, "etc/cron.d/custom-backup");
            assert_eq!(cron_d_jobs[1].path, "etc/cron.d/logrotate");

            // Two generated timer units from the cron entries
            assert!(
                section.generated_timer_units.len() >= 2,
                "expected at least 2 generated timer units, got {}",
                section.generated_timer_units.len()
            );
        } else {
            panic!("expected ScheduledTasks section");
        }
    }

    // ---- Test 2: test_scan_cron_file_system_crontab ----

    #[test]
    fn test_scan_cron_file_system_crontab() {
        let crontab_content = "SHELL=/bin/bash\n\
            PATH=/sbin:/bin:/usr/sbin:/usr/bin\n\
            MAILTO=root\n\
            \n\
            # For details see man 4 crontabs\n\
            0  4  *  *  * root       /usr/local/bin/daily-maintenance.sh\n";

        let exec = MockExecutor::new().with_file("/etc/crontab", crontab_content);

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::ScheduledTasks(ref section) = output.section {
            // One cron job entry for /etc/crontab
            let crontab_jobs: Vec<_> = section
                .cron_jobs
                .iter()
                .filter(|j| j.source == "crontab")
                .collect();
            assert_eq!(crontab_jobs.len(), 1);
            assert_eq!(crontab_jobs[0].path, "etc/crontab");

            // One generated timer for the maintenance job
            let generated = &section.generated_timer_units;
            assert_eq!(generated.len(), 1);
            assert_eq!(generated[0].cron_expr, "0 4 * * *");
            assert!(generated[0].command.contains("daily-maintenance.sh"));
        } else {
            panic!("expected ScheduledTasks section");
        }
    }

    // ---- Test 3: test_scan_user_crontab ----

    #[test]
    fn test_scan_user_crontab() {
        let user_crontab = "# Health check every 15 minutes\n\
            */15 * * * * /home/app/check-health.sh\n\
            # Run startup script on reboot\n\
            @reboot /home/app/startup.sh\n";

        let exec = MockExecutor::new()
            .with_dir("/var/spool/cron", vec!["appuser"])
            .with_file("/var/spool/cron/appuser", user_crontab);

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::ScheduledTasks(ref section) = output.section {
            // User crontab entry
            let user_jobs: Vec<_> = section
                .cron_jobs
                .iter()
                .filter(|j| j.source.starts_with("spool/cron"))
                .collect();
            assert_eq!(user_jobs.len(), 1);
            assert!(user_jobs[0].source.contains("appuser"));

            // Two generated units: one timer for */15 and one advisory for @reboot
            assert_eq!(section.generated_timer_units.len(), 2);
            assert_eq!(section.generated_timer_units[0].cron_expr, "*/15 * * * *");
            assert_eq!(section.generated_timer_units[1].cron_expr, "@reboot");
            assert!(
                section.generated_timer_units[0]
                    .command
                    .contains("check-health.sh")
            );

            // @reboot entry must NOT have a timer with a fake OnCalendar
            let reboot_unit = &section.generated_timer_units[1];
            assert!(
                reboot_unit.timer_content.is_empty(),
                "@reboot must not generate a timer unit, got: {}",
                reboot_unit.timer_content
            );
            assert!(
                reboot_unit
                    .service_content
                    .contains("WantedBy=multi-user.target"),
                "@reboot service must use WantedBy=multi-user.target"
            );
            assert!(
                !reboot_unit.service_content.contains("OnCalendar="),
                "@reboot service must not contain an OnCalendar= directive"
            );
            assert!(
                reboot_unit.command.contains("startup.sh"),
                "@reboot must preserve the original command"
            );
        } else {
            panic!("expected ScheduledTasks section");
        }
    }

    // ---- Test 4: test_scan_cron_period_dir ----

    #[test]
    fn test_scan_cron_period_dir() {
        let exec =
            MockExecutor::new().with_dir("/etc/cron.daily", vec!["logrotate", "rpm-owned-script"]);

        let rpm_state = rpm_state_with_owned(vec!["/etc/cron.daily/rpm-owned-script"]);
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::ScheduledTasks(ref section) = output.section {
            let daily_jobs: Vec<_> = section
                .cron_jobs
                .iter()
                .filter(|j| j.source == "cron.daily")
                .collect();
            assert_eq!(daily_jobs.len(), 2);

            // rpm-owned-script should be marked as rpm_owned
            let rpm_job = daily_jobs
                .iter()
                .find(|j| j.path.contains("rpm-owned"))
                .expect("should find rpm-owned job");
            assert!(rpm_job.rpm_owned);

            // logrotate should NOT be rpm_owned (not in our state)
            let log_job = daily_jobs
                .iter()
                .find(|j| j.path.contains("logrotate"))
                .expect("should find logrotate job");
            assert!(!log_job.rpm_owned);

            // Only logrotate should generate a timer (rpm-owned skipped)
            let daily_timers: Vec<_> = section
                .generated_timer_units
                .iter()
                .filter(|t| t.cron_expr == "@daily")
                .collect();
            assert_eq!(daily_timers.len(), 1);
            assert!(daily_timers[0].source_path.contains("logrotate"));
        } else {
            panic!("expected ScheduledTasks section");
        }
    }

    // ---- Test 5: test_cron_to_on_calendar_basic ----

    #[test]
    fn test_cron_to_on_calendar_basic() {
        // 0 3 * * * -> *-*-* 03:00:00
        let (cal, converted) = cron_to_on_calendar("0 3 * * *");
        assert!(converted);
        assert_eq!(cal, "*-*-* 03:00:00");

        // 30 2 * * * -> *-*-* 02:30:00
        let (cal, converted) = cron_to_on_calendar("30 2 * * *");
        assert!(converted);
        assert_eq!(cal, "*-*-* 02:30:00");
    }

    // ---- Test 6: test_cron_to_on_calendar_complex ----

    #[test]
    fn test_cron_to_on_calendar_complex() {
        // */15 1-5 * * 1-5 -> Mon..Fri *-*-* 1..5:*/15:00
        let (cal, converted) = cron_to_on_calendar("*/15 1-5 * * 1-5");
        assert!(converted);
        assert!(cal.contains("*/15"), "expected */15 in minute, got: {cal}");
        assert!(
            cal.contains("Mon..Fri"),
            "expected Mon..Fri dow, got: {cal}"
        );
        assert!(cal.contains("1..5"), "expected 1..5 hour range, got: {cal}");
    }

    // ---- Test 7: test_cron_to_on_calendar_shortcuts ----

    #[test]
    fn test_cron_to_on_calendar_shortcuts() {
        let (cal, converted) = cron_to_on_calendar("@daily");
        assert!(converted);
        assert_eq!(cal, "*-*-* 00:00:00");

        let (cal, converted) = cron_to_on_calendar("@hourly");
        assert!(converted);
        assert_eq!(cal, "*-*-* *:00:00");

        let (cal, converted) = cron_to_on_calendar("@weekly");
        assert!(converted);
        assert_eq!(cal, "Mon *-*-* 00:00:00");

        let (cal, converted) = cron_to_on_calendar("@yearly");
        assert!(converted);
        assert_eq!(cal, "*-01-01 00:00:00");

        let (cal, converted) = cron_to_on_calendar("@monthly");
        assert!(converted);
        assert_eq!(cal, "*-*-01 00:00:00");
    }

    // ---- Test 8: test_cron_to_on_calendar_reboot ----

    #[test]
    fn test_cron_to_on_calendar_reboot() {
        let (cal, converted) = cron_to_on_calendar("@reboot");
        assert!(!converted);
        assert_eq!(cal, "@reboot");
    }

    // ---- Test 9: test_make_timer_service ----

    #[test]
    fn test_make_timer_service() {
        let (timer, service) = make_timer_service(
            "cron-backup",
            "0 2 * * *",
            "etc/cron.d/backup",
            "/opt/backup.sh",
        );

        assert!(timer.contains("[Unit]"));
        assert!(timer.contains("[Timer]"));
        assert!(timer.contains("OnCalendar="));
        assert!(timer.contains("[Install]"));
        assert!(timer.contains("WantedBy=timers.target"));
        assert!(timer.contains("Persistent=true"));

        assert!(service.contains("[Unit]"));
        assert!(service.contains("[Service]"));
        assert!(service.contains("Type=oneshot"));
        assert!(service.contains("ExecStart=/opt/backup.sh"));
    }

    // ---- Test 9b: @reboot never produces a timer with OnCalendar ----

    #[test]
    fn test_reboot_advisory_no_fake_timer() {
        let (service, timer) =
            make_reboot_advisory("cron-startup", "var/spool/cron/appuser", "/opt/init.sh");

        // Timer content must be empty — @reboot is not a calendar schedule
        assert!(
            timer.is_empty(),
            "@reboot must produce empty timer_content, got: {timer}"
        );

        // Service must use WantedBy=multi-user.target for boot activation
        assert!(
            service.contains("WantedBy=multi-user.target"),
            "service must use WantedBy=multi-user.target, got: {service}"
        );
        assert!(
            service.contains("ExecStart=/opt/init.sh"),
            "service must contain the original command"
        );
        assert!(
            !service.contains("OnCalendar="),
            "service must not contain an OnCalendar= directive"
        );
        assert!(
            service.contains("@reboot"),
            "service must reference @reboot origin for traceability"
        );
    }

    /// T37, the collector half: `Unit=` is read from `[Timer]` by logical
    /// line, the last assignment winning and an empty one resetting it,
    /// and expanded for the timer's own name: `%i`, `%I`, `%p` and `%N`,
    /// for a concrete instance timer and for a plain one. Any other
    /// specifier, or a result that is not a unit name, is `Unresolved`: a
    /// directory specifier the runtime set expands (`%t`, `%E`) gives a
    /// path, never a unit name.
    #[test]
    fn a_timers_unit_assignment_is_read_and_expanded() {
        assert_eq!(
            timer_unit_assignments(
                "[Timer]\nUnit=a.service\n[Unit]\nUnit=b.service\n[Timer]\nUnit=\\\n c.service\n"
            ),
            ["a.service", "c.service"]
        );
        let resolved = |value: &str, name: &str| {
            expand_unit_specifiers(value, name, SpecifierSet::Runtime)
                .and_then(|unit| UnitName::try_from(unit).ok())
                .map(|unit| unit.as_str().to_owned())
        };
        for (value, name, expected) in [
            ("worker@%i.service", "worker@a", Some("worker@a.service")),
            (
                "%p-run@%i.service",
                "worker@a",
                Some("worker-run@a.service"),
            ),
            ("%N.service", "worker@a", Some("worker@a.service")),
            ("%N.service", "report", Some("report.service")),
            ("%p.service", "report", Some("report.service")),
            ("x@%I.service", "worker@db", Some("x@db.service")),
            ("x@%I.service", "worker@a-b", None),
            ("report-%H.service", "report", None),
            ("report%", "report", None),
            // A directory specifier expands to a path, never a unit name.
            ("%t.service", "report", None),
            ("x@%E.service", "worker@a", None),
        ] {
            assert_eq!(
                resolved(value, name).as_deref(),
                expected,
                "{value} for {name}"
            );
        }
    }

    /// T37, the collector half through the inspector: a drop-in under
    /// `/etc` that assigns `Unit=` is recorded and wins over the timer's
    /// own file; a vendor drop-in is applied and not recorded; a drop-in
    /// that assigns nothing is not recorded; with no `Unit=` the trigger is
    /// the same-named service.
    #[test]
    fn a_timers_trigger_and_its_drop_ins_are_recorded() {
        let exec = MockExecutor::new()
            .with_file(
                "/etc/systemd/system/report.timer.d/10-unit.conf",
                "[Timer]\nUnit=other.service\n",
            )
            .with_file(
                "/etc/systemd/system/report.timer.d/20-calendar.conf",
                "[Timer]\nOnCalendar=weekly\n",
            )
            .with_dir(
                "/etc/systemd/system/report.timer.d",
                vec!["10-unit.conf", "20-calendar.conf"],
            )
            .with_file(
                "/usr/lib/systemd/system/report.timer.d/30-vendor.conf",
                "[Timer]\nUnit=vendor.service\n",
            )
            .with_dir(
                "/usr/lib/systemd/system/report.timer.d",
                vec!["30-vendor.conf"],
            );
        let unit = |name: &str| TimerTrigger::Unit(name.to_owned().try_into().unwrap());
        assert_eq!(
            timer_trigger(&exec, "report", "[Timer]\nOnCalendar=daily\n"),
            (
                unit("vendor.service"),
                vec!["/etc/systemd/system/report.timer.d/10-unit.conf".to_owned()]
            )
        );
        assert_eq!(
            timer_trigger(&MockExecutor::new(), "plain", "[Timer]\nOnCalendar=daily\n"),
            (unit("plain.service"), Vec::new())
        );
        assert_eq!(
            timer_trigger(
                &MockExecutor::new(),
                "plain",
                "[Timer]\nUnit=x.service\nUnit=\n"
            ),
            (unit("plain.service"), Vec::new())
        );
    }

    /// The timer descriptions the inspector records on `exec`.
    fn timers_on(exec: &MockExecutor) -> Vec<SystemdTimer> {
        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let ctx = InspectionContext {
            source_system: &source,
            executor: exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };
        let output = match ScheduledTasksInspector::new().inspect(&ctx, &NullProgress) {
            Ok(output) => output,
            Err(InspectorError::Degraded { partial, .. }) => *partial,
            Err(e) => panic!("the inspector fails: {e:?}"),
        };
        match output.section {
            SectionData::ScheduledTasks(section) => section.systemd_timers,
            _ => panic!("expected ScheduledTasks section"),
        }
    }

    const WORKER_TEMPLATE: &str = "/etc/systemd/system/worker@.timer";
    const WORKER_LINK: &str = "/etc/systemd/system/timers.target.wants/worker@a.timer";

    /// A host with the local template `worker@.timer`, whose `[Timer]`
    /// adds `timer_extra`, and its instance `worker@a.timer` enabled by a
    /// link in `timers.target.wants`.
    fn worker_host(timer_extra: &str) -> MockExecutor {
        MockExecutor::new()
            .with_dir(
                "/etc/systemd/system",
                vec!["worker@.timer", "worker@.service", "timers.target.wants"],
            )
            .with_file(
                WORKER_TEMPLATE,
                &format!(
                    "[Unit]\nDescription=Worker %i\n[Timer]\nOnCalendar=daily\n{timer_extra}\
                     [Install]\nWantedBy=timers.target\n"
                ),
            )
            .with_file(
                "/etc/systemd/system/worker@.service",
                "[Service]\nExecStart=/usr/local/bin/worker %i\n",
            )
            .with_dir(
                "/etc/systemd/system/timers.target.wants",
                vec!["worker@a.timer"],
            )
            .with_link(WORKER_LINK, WORKER_TEMPLATE)
    }

    /// The description recorded for `worker@a.timer`.
    fn worker_a(timers: &[SystemdTimer]) -> &SystemdTimer {
        timers
            .iter()
            .find(|t| t.name == "worker@a")
            .unwrap_or_else(|| panic!("presence: worker@a's description in {timers:?}"))
    }

    /// § 8, Task 5.1a: an instance timer linked under `/etc/systemd/system`
    /// has no timer file of its own, so its description is recorded from
    /// its template's file, with `Unit=` expanded for the instance. With no
    /// `Unit=` the trigger is the instance's same-named service.
    #[test]
    fn an_instance_timers_description_is_read_from_its_template() {
        let unit = |name: &str| TimerTrigger::Unit(name.to_owned().try_into().unwrap());
        for (extra, trigger) in [
            ("Unit=worker-run@%i.service\n", "worker-run@a.service"),
            ("", "worker@a.service"),
        ] {
            let timers = timers_on(&worker_host(extra));
            let timer = worker_a(&timers);
            assert_eq!(timer.trigger, unit(trigger), "{extra:?}");
            assert!(timer.trigger_drop_ins.is_empty(), "{extra:?}");
            assert_eq!(timer.description, "Worker %i");
            assert_eq!(timer.on_calendar, "daily");
            assert_eq!(timer.exec_start, "/usr/local/bin/worker %i");
            assert_eq!(timer.source, "local");
            assert_eq!(timer.path, "etc/systemd/system/worker@.timer");
        }
    }

    /// § 8: `Unit=` set only in a drop-in of the template's
    /// `<prefix>@.timer.d` under `/etc` is read and that drop-in is the
    /// trigger's evidence; a specifier the reader does not expand is
    /// `Unresolved`.
    #[test]
    fn an_instance_timers_trigger_is_read_from_the_template_directory() {
        const DROP_IN: &str = "/etc/systemd/system/worker@.timer.d/10-unit.conf";
        let exec = worker_host("")
            .with_dir("/etc/systemd/system/worker@.timer.d", vec!["10-unit.conf"])
            .with_file(DROP_IN, "[Timer]\nUnit=other@%i.service\n");
        let timers = timers_on(&exec);
        let timer = worker_a(&timers);
        assert_eq!(
            timer.trigger,
            TimerTrigger::Unit("other@a.service".to_owned().try_into().unwrap())
        );
        assert_eq!(timer.trigger_drop_ins, [DROP_IN]);

        let timers = timers_on(&worker_host("Unit=worker-%H.service\n"));
        assert_eq!(worker_a(&timers).trigger, TimerTrigger::Unresolved);
    }

    /// § 8 with G2's order: the instance's own `.d` and the template's are
    /// read in file-name order across both, and of two files of one name
    /// the instance's directory wins over the template's whatever the
    /// root, then `/etc` over the vendor directory. A hidden `/etc` file is
    /// not evidence; one overridden by a later file name still is.
    #[test]
    fn an_instance_timers_drop_ins_follow_the_load_order() {
        let unit = |name: &str| TimerTrigger::Unit(name.to_owned().try_into().unwrap());
        let exec = MockExecutor::new()
            .with_dir(
                "/etc/systemd/system/worker@.timer.d",
                vec!["10-a.conf", "20-b.conf"],
            )
            .with_file(
                "/etc/systemd/system/worker@.timer.d/10-a.conf",
                "[Timer]\nUnit=hidden@%i.service\n",
            )
            .with_file(
                "/etc/systemd/system/worker@.timer.d/20-b.conf",
                "[Timer]\nUnit=etc-template@%i.service\n",
            )
            .with_dir(
                "/usr/lib/systemd/system/worker@a.timer.d",
                vec!["10-a.conf", "30-c.conf"],
            )
            .with_file(
                "/usr/lib/systemd/system/worker@a.timer.d/10-a.conf",
                "[Timer]\nOnCalendar=weekly\n",
            )
            .with_file(
                "/usr/lib/systemd/system/worker@a.timer.d/30-c.conf",
                "[Timer]\nUnit=vendor-instance.service\n",
            );
        assert_eq!(
            timer_trigger(&exec, "worker@a", "[Timer]\nOnCalendar=daily\n"),
            (
                unit("vendor-instance.service"),
                vec!["/etc/systemd/system/worker@.timer.d/20-b.conf".to_owned()]
            )
        );
        // The plain timer `worker` reads neither directory.
        assert_eq!(
            timer_trigger(&exec, "worker", "[Timer]\nOnCalendar=daily\n"),
            (unit("worker.service"), Vec::new())
        );
    }

    // ---- Test 10: test_scan_systemd_timers ----

    #[test]
    fn test_scan_systemd_timers() {
        let timer_content = "[Unit]\nDescription=Daily cleanup of temp files\n\n\
            [Timer]\nOnCalendar=daily\nPersistent=true\n\n\
            [Install]\nWantedBy=timers.target\n";

        let service_content = "[Unit]\nDescription=Cleanup temp files\n\n\
            [Service]\nType=oneshot\nExecStart=/usr/local/bin/cleanup.sh\n";

        let exec = MockExecutor::new()
            .with_dir(
                "/etc/systemd/system",
                vec!["cleanup.timer", "cleanup.service"],
            )
            .with_file("/etc/systemd/system/cleanup.timer", timer_content)
            .with_file("/etc/systemd/system/cleanup.service", service_content);

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::ScheduledTasks(ref section) = output.section {
            assert_eq!(section.systemd_timers.len(), 1);
            let timer = &section.systemd_timers[0];
            assert_eq!(timer.name, "cleanup");
            assert_eq!(timer.on_calendar, "daily");
            assert_eq!(timer.exec_start, "/usr/local/bin/cleanup.sh");
            assert_eq!(timer.description, "Daily cleanup of temp files");
            assert_eq!(timer.source, "local");
        } else {
            panic!("expected ScheduledTasks section");
        }
    }

    // ---- Test 11: test_parse_at_job ----

    #[test]
    fn test_parse_at_job() {
        let at_content = "#!/bin/sh\n\
            # atrun uid=1000 gid=1000\n\
            # mail appuser 0\n\
            umask 22\n\
            SHELL=/bin/sh; export SHELL\n\
            HOME=/home/appuser; export HOME\n\
            PATH=/usr/local/bin:/usr/bin:/bin; export PATH\n\
            cd /home/appuser || {\n\
            \techo 'Execution directory inaccessible' >&2\n\
            \texit 1\n\
            }\n\
            /usr/local/bin/run-migration.sh --stage=final\n\
            echo \"migration complete\"\n";

        let job = parse_at_job(at_content, "var/spool/at/a00001");
        assert_eq!(job.user, "appuser");
        assert_eq!(job.working_dir, "/home/appuser");
        assert!(
            job.command.contains("run-migration.sh"),
            "command should contain run-migration.sh, got: {}",
            job.command
        );
        assert!(
            job.command.contains("migration complete"),
            "command should contain echo output, got: {}",
            job.command
        );
    }

    // ---- Test 12: test_scan_at_jobs_empty ----

    #[test]
    fn test_scan_at_jobs_empty() {
        let exec = MockExecutor::new().with_dir("/var/spool/at", vec![]);

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::ScheduledTasks(ref section) = output.section {
            assert!(section.at_jobs.is_empty());
        } else {
            panic!("expected ScheduledTasks section");
        }
    }

    // ---- Test 13: test_rpm_owned_classification ----

    #[test]
    fn test_rpm_owned_classification() {
        let exec = MockExecutor::new()
            .with_dir("/etc/cron.d", vec!["rpm-package-job", "custom-job"])
            .with_file(
                "/etc/cron.d/rpm-package-job",
                "0 1 * * * root /usr/bin/rpm-task\n",
            )
            .with_file(
                "/etc/cron.d/custom-job",
                "0 2 * * * root /opt/custom-task\n",
            );

        let rpm_state = rpm_state_with_owned(vec!["/etc/cron.d/rpm-package-job"]);
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::ScheduledTasks(ref section) = output.section {
            let rpm_job = section
                .cron_jobs
                .iter()
                .find(|j| j.path.contains("rpm-package-job"))
                .expect("should find rpm job");
            assert!(rpm_job.rpm_owned, "rpm-package-job should be rpm_owned");

            let custom_job = section
                .cron_jobs
                .iter()
                .find(|j| j.path.contains("custom-job"))
                .expect("should find custom job");
            assert!(!custom_job.rpm_owned, "custom-job should not be rpm_owned");

            // Only custom-job should generate a timer (rpm-owned skipped)
            let gen_timers: Vec<_> = section
                .generated_timer_units
                .iter()
                .filter(|t| t.source_path.starts_with("etc/cron.d/"))
                .collect();
            assert_eq!(
                gen_timers.len(),
                1,
                "only custom-job should generate a timer"
            );
            assert!(gen_timers[0].source_path.contains("custom-job"));
        } else {
            panic!("expected ScheduledTasks section");
        }
    }

    // ---- Test 14: test_scheduled_empty_system ----

    #[test]
    fn test_scheduled_empty_system() {
        // No cron dirs, no timers, no at -> empty section
        let exec = MockExecutor::new();

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed on empty system");
        if let SectionData::ScheduledTasks(ref section) = output.section {
            assert!(section.cron_jobs.is_empty());
            assert!(section.systemd_timers.is_empty());
            assert!(section.at_jobs.is_empty());
            assert!(section.generated_timer_units.is_empty());
        } else {
            panic!("expected ScheduledTasks section");
        }
    }

    // ---- Test 15: test_scheduled_degraded_permission_denied ----

    #[test]
    fn test_scheduled_degraded_permission_denied() {
        let exec =
            MockExecutor::new().with_dir_error("/etc/cron.d", std::io::ErrorKind::PermissionDenied);

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Degraded { reason, partial }) => {
                assert!(
                    reason.contains("permission denied"),
                    "expected permission denied in reason, got: {reason}"
                );
                // Should still have a valid partial output
                if let SectionData::ScheduledTasks(ref _section) = partial.section {
                    // OK - section exists even if degraded
                } else {
                    panic!("expected ScheduledTasks section in degraded output");
                }
            }
            other => panic!("expected Degraded error for permission denied, got: {other:?}"),
        }
    }

    // ---- Additional: RPM state None -> Failed ----

    #[test]
    fn test_rpm_state_none_returns_failed() {
        let exec = MockExecutor::new();
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Failed { reason }) => {
                assert!(
                    reason.contains("RPM prerequisite"),
                    "expected RPM prerequisite message, got: {reason}"
                );
            }
            other => panic!("expected Failed error for None rpm_state, got: {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // include: true collector defaults
    // -----------------------------------------------------------------------

    #[test]
    fn collected_cron_jobs_have_include_true() {
        let exec = MockExecutor::new()
            .with_dir("/etc/cron.d", vec!["backup"])
            .with_file("/etc/cron.d/backup", "0 2 * * * root /opt/backup.sh\n");

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::ScheduledTasks(ref section) = output.section {
            for job in &section.cron_jobs {
                assert!(
                    job.disposition.is_included(),
                    "collected CronJob '{}' should have include: true",
                    job.path
                );
            }
            for timer in &section.generated_timer_units {
                assert!(
                    timer.disposition.is_included(),
                    "generated timer '{}' should have include: true",
                    timer.name
                );
            }
        } else {
            panic!("expected ScheduledTasks section");
        }
    }

    #[test]
    fn collected_at_jobs_have_include_true() {
        let at_content = "#!/bin/sh\n# mail appuser 0\n/opt/run.sh\n";
        let exec = MockExecutor::new()
            .with_dir("/var/spool/at", vec!["a00001"])
            .with_file("/var/spool/at/a00001", at_content);

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let output = result.expect("should succeed");
        if let SectionData::ScheduledTasks(ref section) = output.section {
            for job in &section.at_jobs {
                assert!(
                    job.disposition.is_included(),
                    "collected AtJob '{}' should have include: true",
                    job.file
                );
            }
        } else {
            panic!("expected ScheduledTasks section");
        }
    }
}
