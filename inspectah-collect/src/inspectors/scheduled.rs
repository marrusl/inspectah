use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput, RpmState,
};
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::redaction::{Confidence, RedactionHint};
use inspectah_core::types::scheduled::{
    AtJob, CronJob, GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer,
};
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

    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
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
        scan_systemd_timers(
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

        // --- At jobs ---
        scan_at_jobs(exec, &mut section, &mut hints, &mut degraded_reasons);

        // Emit redaction hints for timer ExecStart fields
        for timer in &section.systemd_timers {
            check_command_redaction(&timer.exec_start, &timer.path, &mut hints);
        }

        if !warnings.is_empty() || !degraded_reasons.is_empty() {
            // Add degraded file count warning if applicable
            if !degraded_reasons.is_empty() {
                warnings.push(Warning {
                    inspector: "scheduled_tasks".into(),
                    message: format!(
                        "degraded: {}",
                        degraded_reasons.join("; ")
                    ),
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
            degraded_reasons
                .push(format!("permission denied reading {dir_path}"));
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
            ..Default::default()
        });

        if is_rpm_owned {
            continue;
        }

        let content = match exec.read_file(Path::new(&file_path)) {
            Ok(c) => c,
            Err(_) => continue,
        };

        parse_cron_entries(section, &content, &file_path, &cron_source, entry_name, hints);
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
        ..Default::default()
    });

    let content = match exec.read_file(Path::new(file_path)) {
        Ok(c) => c,
        Err(_) => return,
    };

    let name = file_path
        .rsplit('/')
        .next()
        .unwrap_or("crontab");

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

                let (timer_content, service_content) =
                    make_timer_service(&safe_name, shortcut, &rel_path, &command);

                check_command_redaction(&command, file_path, hints);

                section.generated_timer_units.push(GeneratedTimerUnit {
                    name: safe_name,
                    timer_content,
                    service_content,
                    cron_expr: shortcut.into(),
                    source_path: rel_path,
                    command,
                    ..Default::default()
                });
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
            degraded_reasons
                .push(format!("permission denied reading {dir_path}"));
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
    if kind == "month" {
        if let Some(v) = month_names().get(low.as_str()) {
            return v.to_string();
        }
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
    if field.contains('-') && field.contains('/') {
        if let Some((range_part, step)) = field.split_once('/') {
            if let Some((lo_raw, hi_raw)) = range_part.split_once('-') {
                let lo = normalise_cron_token(lo_raw.trim(), kind);
                let hi = normalise_cron_token(hi_raw.trim(), kind);
                return format!("{lo}..{hi}/{step}");
            }
        }
    }

    // Ranges: 1-5 -> 1..5
    if field.contains('-') {
        if let Some((lo_raw, hi_raw)) = field.split_once('-') {
            let lo = normalise_cron_token(lo_raw.trim(), kind);
            let hi = normalise_cron_token(hi_raw.trim(), kind);
            return format!("{lo}..{hi}");
        }
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
    if is_digits(field) && (kind == "minute" || kind == "hour") {
        if let Ok(n) = field.parse::<u32>() {
            return format!("{n:02}");
        }
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
fn make_timer_service(
    _name: &str,
    cron_expr: &str,
    path: &str,
    command: &str,
) -> (String, String) {
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

/// Scans a systemd unit directory for .timer files and their paired .service units.
fn scan_systemd_timers(
    exec: &dyn Executor,
    section: &mut ScheduledTaskSection,
    base_dir: &str,
    source_label: &str,
    degraded_reasons: &mut Vec<String>,
) {
    let dir_path = format!("/{base_dir}");
    let entries = match exec.read_dir(Path::new(&dir_path)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            degraded_reasons
                .push(format!("permission denied reading {dir_path}"));
            return;
        }
        Err(_) => return,
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

        section.systemd_timers.push(SystemdTimer {
            name: name.to_string(),
            on_calendar,
            exec_start,
            description,
            source: source_label.to_string(),
            path: strip_leading_slash(&timer_path),
            timer_content: timer_text,
            service_content: service_text,
            ..Default::default()
        });
    }
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
        if stripped.starts_with("# atrun uid=") {
            if let Some(uid_part) = stripped.strip_prefix("# atrun uid=") {
                let uid = uid_part.split_whitespace().next().unwrap_or("");
                if !uid.is_empty() {
                    user = format!("uid={uid}");
                }
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
            degraded_reasons
                .push("permission denied reading /var/spool/at".into());
            return;
        }
        Err(_) => return,
    };

    for entry_name in &entries {
        if entry_name.starts_with('.') {
            continue;
        }
        let file_path = format!("/var/spool/at/{entry_name}");
        let content = exec
            .read_file(Path::new(&file_path))
            .unwrap_or_default();
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
        };

        let result = inspector.inspect(&ctx);
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

        let exec = MockExecutor::new()
            .with_file("/etc/crontab", crontab_content);

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
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
            let gen = &section.generated_timer_units;
            assert_eq!(gen.len(), 1);
            assert_eq!(gen[0].cron_expr, "0 4 * * *");
            assert!(gen[0].command.contains("daily-maintenance.sh"));
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
        };

        let result = inspector.inspect(&ctx);
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

            // Two generated timers: one for */15 and one for @reboot
            assert_eq!(section.generated_timer_units.len(), 2);
            assert_eq!(section.generated_timer_units[0].cron_expr, "*/15 * * * *");
            assert_eq!(section.generated_timer_units[1].cron_expr, "@reboot");
            assert!(section.generated_timer_units[0]
                .command
                .contains("check-health.sh"));
        } else {
            panic!("expected ScheduledTasks section");
        }
    }

    // ---- Test 4: test_scan_cron_period_dir ----

    #[test]
    fn test_scan_cron_period_dir() {
        let exec = MockExecutor::new()
            .with_dir("/etc/cron.daily", vec!["logrotate", "rpm-owned-script"]);

        let rpm_state = rpm_state_with_owned(vec!["/etc/cron.daily/rpm-owned-script"]);
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let output = result.expect("should succeed");
        if let SectionData::ScheduledTasks(ref section) = output.section {
            let daily_jobs: Vec<_> = section
                .cron_jobs
                .iter()
                .filter(|j| j.source == "cron.daily")
                .collect();
            assert_eq!(daily_jobs.len(), 2);

            // rpm-owned-script should be marked as rpm_owned
            let rpm_job = daily_jobs.iter().find(|j| j.path.contains("rpm-owned")).expect("should find rpm-owned job");
            assert!(rpm_job.rpm_owned);

            // logrotate should NOT be rpm_owned (not in our state)
            let log_job = daily_jobs.iter().find(|j| j.path.contains("logrotate")).expect("should find logrotate job");
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
        assert!(
            cal.contains("1..5"),
            "expected 1..5 hour range, got: {cal}"
        );
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
        let (timer, service) =
            make_timer_service("cron-backup", "0 2 * * *", "etc/cron.d/backup", "/opt/backup.sh");

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
        };

        let result = inspector.inspect(&ctx);
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
        };

        let result = inspector.inspect(&ctx);
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
        };

        let result = inspector.inspect(&ctx);
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
        };

        let result = inspector.inspect(&ctx);
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
        let exec = MockExecutor::new()
            .with_dir_error("/etc/cron.d", std::io::ErrorKind::PermissionDenied);

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = ScheduledTasksInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
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
            other => panic!(
                "expected Degraded error for permission denied, got: {other:?}"
            ),
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
        };

        let result = inspector.inspect(&ctx);
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
}
