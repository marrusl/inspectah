// Package inspector — Scheduled Tasks inspector.
//
// Scans all cron locations, existing systemd .timer units (both vendor and
// local), at spool files, and generates systemd timer units from cron
// entries for the migration story.
package inspector

import (
	"fmt"
	"regexp"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// ScheduledTaskOptions configures the Scheduled Tasks inspector.
type ScheduledTaskOptions struct {
	// RpmOwnedPaths is a pre-built set of RPM-owned paths. If nil,
	// BuildRpmOwnedPaths is called internally.
	RpmOwnedPaths map[string]bool

	// SystemType is the detected source system type.
	SystemType schema.SystemType
}

// RunScheduledTasks runs the full scheduled-tasks inspection: cron
// directories, user crontabs, systemd timers, at jobs, and cron-to-timer
// generation.
func RunScheduledTasks(exec Executor, opts ScheduledTaskOptions) (*schema.ScheduledTaskSection, []Warning, error) {
	var warnings []Warning
	section := &schema.ScheduledTaskSection{
		CronJobs:            []schema.CronJob{},
		SystemdTimers:       []schema.SystemdTimer{},
		AtJobs:              []schema.AtJob{},
		GeneratedTimerUnits: []schema.GeneratedTimerUnit{},
	}

	rpmOwned := opts.RpmOwnedPaths
	if rpmOwned == nil {
		var ownedWarnings []Warning
		rpmOwned, ownedWarnings = BuildRpmOwnedPaths(exec)
		warnings = append(warnings, ownedWarnings...)
	}

	// --- Cron ---
	scanCronDir(exec, section, "/etc/cron.d", "cron.d", rpmOwned)
	scanCronFile(exec, section, "/etc/crontab", "crontab", rpmOwned)

	for _, period := range []string{"hourly", "daily", "weekly", "monthly"} {
		scanCronPeriodDir(exec, section, period, rpmOwned)
	}

	// User crontabs
	scanCronDir(exec, section, "/var/spool/cron", "spool/cron", nil)

	// --- Existing systemd timers ---
	section.SystemdTimers = append(section.SystemdTimers,
		scanSystemdTimers(exec, "etc/systemd/system", "local", opts.SystemType)...)
	section.SystemdTimers = append(section.SystemdTimers,
		scanSystemdTimers(exec, "usr/lib/systemd/system", "vendor", opts.SystemType)...)

	// --- At jobs ---
	scanAtJobs(exec, section)

	return section, warnings, nil
}

// ---------------------------------------------------------------------------
// Cron scanning
// ---------------------------------------------------------------------------

// cronLineRe matches lines starting with a digit or asterisk (cron entries).
var cronLineRe = regexp.MustCompile(`^[\d*]`)

// scanCronDir scans a directory for cron job files.
func scanCronDir(exec Executor, section *schema.ScheduledTaskSection, dirPath, source string, rpmOwned map[string]bool) {
	entries, err := exec.ReadDir(dirPath)
	if err != nil {
		return
	}
	for _, e := range entries {
		if e.IsDir() || strings.HasPrefix(e.Name(), ".") {
			continue
		}
		filePath := dirPath + "/" + e.Name()

		isRpmOwned := rpmOwned != nil && rpmOwned[filePath]

		// For spool/cron, include user name in source
		cronSource := source
		if source == "spool/cron" {
			cronSource = fmt.Sprintf("spool/cron (%s)", e.Name())
		}

		section.CronJobs = append(section.CronJobs, schema.CronJob{
			Path:     strings.TrimPrefix(filePath, "/"),
			Source:   cronSource,
			RpmOwned: isRpmOwned,
		})

		if isRpmOwned {
			continue
		}

		content, err := exec.ReadFile(filePath)
		if err != nil {
			continue
		}

		parseCronEntries(section, content, filePath, cronSource, e.Name())
	}
}

// scanCronFile scans a single cron file (e.g. /etc/crontab).
func scanCronFile(exec Executor, section *schema.ScheduledTaskSection, filePath, source string, rpmOwned map[string]bool) {
	if !exec.FileExists(filePath) {
		return
	}

	isRpmOwned := rpmOwned != nil && rpmOwned[filePath]
	section.CronJobs = append(section.CronJobs, schema.CronJob{
		Path:     strings.TrimPrefix(filePath, "/"),
		Source:   source,
		RpmOwned: isRpmOwned,
	})

	if isRpmOwned {
		return
	}

	content, err := exec.ReadFile(filePath)
	if err != nil {
		return
	}

	name := "crontab"
	if idx := strings.LastIndex(filePath, "/"); idx >= 0 {
		name = filePath[idx+1:]
	}
	parseCronEntries(section, content, filePath, source, name)
}

// parseCronEntries extracts cron expressions from file content and generates
// timer units for each entry.
func parseCronEntries(section *schema.ScheduledTaskSection, content, filePath, source, fileName string) {
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		if !cronLineRe.MatchString(line) {
			continue
		}

		parts := strings.Fields(line)
		if len(parts) < 6 {
			continue
		}

		cronExpr := strings.Join(parts[:5], " ")

		// System crontabs (cron.d, crontab) have a user field at position 5
		var command string
		if source == "cron.d" || source == "crontab" {
			if len(parts) > 6 {
				command = strings.Join(parts[6:], " ")
			}
		} else {
			command = strings.Join(parts[5:], " ")
		}

		safeName := "cron-" + strings.ReplaceAll(fileName, ".", "-")
		relPath := strings.TrimPrefix(filePath, "/")

		timerContent, serviceContent := makeTimerService(safeName, cronExpr, relPath, command)
		section.GeneratedTimerUnits = append(section.GeneratedTimerUnits, schema.GeneratedTimerUnit{
			Name:           safeName,
			TimerContent:   timerContent,
			ServiceContent: serviceContent,
			CronExpr:       cronExpr,
			SourcePath:     relPath,
			Command:        command,
		})
	}
}

// ---------------------------------------------------------------------------
// Cron period directories (hourly, daily, weekly, monthly)
// ---------------------------------------------------------------------------

// periodSchedules maps cron period names to systemd OnCalendar values.
var periodSchedules = map[string]string{
	"hourly":  "*-*-* *:01:00",
	"daily":   "*-*-* 03:00:00",
	"weekly":  "Mon *-*-* 03:00:00",
	"monthly": "*-*-01 03:00:00",
}

// scanCronPeriodDir scans a cron.{period} directory and generates timer
// units with the standard period schedule.
func scanCronPeriodDir(exec Executor, section *schema.ScheduledTaskSection, period string, rpmOwned map[string]bool) {
	dirPath := fmt.Sprintf("/etc/cron.%s", period)
	entries, err := exec.ReadDir(dirPath)
	if err != nil {
		return
	}

	onCalendar := periodSchedules[period]

	for _, e := range entries {
		if e.IsDir() || strings.HasPrefix(e.Name(), ".") {
			continue
		}

		filePath := dirPath + "/" + e.Name()
		relPath := strings.TrimPrefix(filePath, "/")
		isRpmOwned := rpmOwned != nil && rpmOwned[filePath]

		section.CronJobs = append(section.CronJobs, schema.CronJob{
			Path:     relPath,
			Source:   fmt.Sprintf("cron.%s", period),
			RpmOwned: isRpmOwned,
		})

		if isRpmOwned {
			continue
		}

		safeName := strings.ReplaceAll(fmt.Sprintf("cron-%s-%s", period, e.Name()), ".", "-")
		command := "/" + relPath

		timerContent := fmt.Sprintf(
			"[Unit]\nDescription=Generated from cron.%s: %s\n"+
				"# Original: cron.%s script\n\n"+
				"[Timer]\nOnCalendar=%s\nPersistent=true\n\n"+
				"[Install]\nWantedBy=timers.target\n",
			period, relPath, period, onCalendar)

		serviceContent := fmt.Sprintf(
			"[Unit]\nDescription=Timer from cron.%s %s\n\n"+
				"[Service]\nType=oneshot\nExecStart=%s\n",
			period, relPath, command)

		section.GeneratedTimerUnits = append(section.GeneratedTimerUnits, schema.GeneratedTimerUnit{
			Name:           safeName,
			TimerContent:   timerContent,
			ServiceContent: serviceContent,
			CronExpr:       "@" + period,
			SourcePath:     relPath,
			Command:        command,
		})
	}
}

// ---------------------------------------------------------------------------
// Cron -> systemd OnCalendar conversion
// ---------------------------------------------------------------------------

// monthNames maps cron month abbreviations to numeric month values.
var monthNames = map[string]string{
	"jan": "1", "feb": "2", "mar": "3", "apr": "4",
	"may": "5", "jun": "6", "jul": "7", "aug": "8",
	"sep": "9", "oct": "10", "nov": "11", "dec": "12",
}

// dowNamesToSystemd maps cron day-of-week abbreviations to systemd names.
var dowNamesToSystemd = map[string]string{
	"sun": "Sun", "mon": "Mon", "tue": "Tue", "wed": "Wed",
	"thu": "Thu", "fri": "Fri", "sat": "Sat",
}

// dowNumeric maps numeric day-of-week values to systemd names.
var dowNumeric = map[string]string{
	"0": "Sun", "1": "Mon", "2": "Tue", "3": "Wed",
	"4": "Thu", "5": "Fri", "6": "Sat", "7": "Sun",
}

// cronShortcuts maps cron shorthand expressions to OnCalendar values.
var cronShortcuts = map[string]struct {
	calendar  string
	converted bool
}{
	"@yearly":   {"*-01-01 00:00:00", true},
	"@annually": {"*-01-01 00:00:00", true},
	"@monthly":  {"*-*-01 00:00:00", true},
	"@weekly":   {"Mon *-*-* 00:00:00", true},
	"@daily":    {"*-*-* 00:00:00", true},
	"@midnight": {"*-*-* 00:00:00", true},
	"@hourly":   {"*-*-* *:00:00", true},
}

// normaliseCronToken maps a single cron name/number to its canonical form.
func normaliseCronToken(token, kind string) string {
	low := strings.ToLower(token)
	if kind == "month" {
		if v, ok := monthNames[low]; ok {
			return v
		}
	}
	if kind == "dow" {
		if v, ok := dowNamesToSystemd[low]; ok {
			return v
		}
		if v, ok := dowNumeric[token]; ok {
			return v
		}
	}
	return token
}

// cronFieldToCalendar converts a single cron field to its systemd
// OnCalendar equivalent.
func cronFieldToCalendar(field, kind string) string {
	if field == "*" {
		return "*"
	}

	// Step values: */5
	if strings.HasPrefix(field, "*/") {
		step := field[2:]
		if isDigits(step) {
			switch kind {
			case "minute":
				return "*/" + step
			case "hour":
				return "00/" + step
			default:
				return field
			}
		}
		return field
	}

	// Range+step: 1-10/2 -> 1..10/2
	if strings.Contains(field, "-") && strings.Contains(field, "/") {
		rangePart, step, ok := strings.Cut(field, "/")
		if ok {
			parts := strings.SplitN(rangePart, "-", 2)
			if len(parts) == 2 {
				lo := normaliseCronToken(strings.TrimSpace(parts[0]), kind)
				hi := normaliseCronToken(strings.TrimSpace(parts[1]), kind)
				return fmt.Sprintf("%s..%s/%s", lo, hi, step)
			}
		}
	}

	// Ranges: 1-5 -> 1..5
	if strings.Contains(field, "-") {
		parts := strings.SplitN(field, "-", 2)
		if len(parts) == 2 {
			lo := normaliseCronToken(strings.TrimSpace(parts[0]), kind)
			hi := normaliseCronToken(strings.TrimSpace(parts[1]), kind)
			return fmt.Sprintf("%s..%s", lo, hi)
		}
	}

	// Lists: 1,3,5
	if strings.Contains(field, ",") {
		elems := strings.Split(field, ",")
		normalised := make([]string, len(elems))
		for i, e := range elems {
			normalised[i] = normaliseCronToken(strings.TrimSpace(e), kind)
		}
		return strings.Join(normalised, ",")
	}

	// Named or numeric tokens
	normalised := normaliseCronToken(field, kind)
	if normalised != field {
		return normalised
	}

	// Plain digit: zero-pad for minute/hour
	if isDigits(field) && (kind == "minute" || kind == "hour") {
		return fmt.Sprintf("%02d", mustAtoi(field))
	}

	return field
}

// CronToOnCalendar converts a 5-field cron expression to a systemd
// OnCalendar value. Returns (onCalendar, converted) where converted is
// true if the expression was fully handled.
func CronToOnCalendar(cronExpr string) (string, bool) {
	expr := strings.TrimSpace(cronExpr)

	// Named shortcuts
	if sc, ok := cronShortcuts[strings.ToLower(expr)]; ok {
		return sc.calendar, sc.converted
	}

	// @reboot has no calendar equivalent
	if strings.ToLower(expr) == "@reboot" {
		return "@reboot", false
	}

	parts := strings.Fields(expr)
	if len(parts) < 5 {
		return "*-*-* 02:00:00", false
	}

	minute, hour, dom, month, dow := parts[0], parts[1], parts[2], parts[3], parts[4]

	calMin := cronFieldToCalendar(minute, "minute")
	calHour := cronFieldToCalendar(hour, "hour")
	calDom := cronFieldToCalendar(dom, "dom")
	calMonth := cronFieldToCalendar(month, "month")
	calDow := cronFieldToCalendar(dow, "dow")

	datePart := fmt.Sprintf("*-%s-%s", calMonth, calDom)
	timePart := fmt.Sprintf("%s:%s:00", calHour, calMin)

	if calDow != "*" {
		return fmt.Sprintf("%s %s %s", calDow, datePart, timePart), true
	}
	return fmt.Sprintf("%s %s", datePart, timePart), true
}

// makeTimerService generates systemd .timer and .service unit content
// from a cron expression.
func makeTimerService(name, cronExpr, path, command string) (timerContent, serviceContent string) {
	onCalendar, converted := CronToOnCalendar(cronExpr)

	fixmeLines := ""
	if !converted {
		if onCalendar == "@reboot" {
			fixmeLines = "# FIXME: @reboot has no OnCalendar equivalent.\n" +
				"# Use a oneshot service with WantedBy=multi-user.target instead.\n"
			onCalendar = "*-*-* 02:00:00"
		} else {
			fixmeLines = fmt.Sprintf("# FIXME: cron expression '%s' could not be fully converted.\n"+
				"# Review and correct the OnCalendar value below.\n", cronExpr)
		}
	}

	timerContent = fmt.Sprintf("[Unit]\nDescription=Generated from cron: %s\n"+
		"# Original cron: %s\n"+
		"%s\n"+
		"[Timer]\nOnCalendar=%s\nPersistent=true\n\n"+
		"[Install]\nWantedBy=timers.target\n",
		path, cronExpr, fixmeLines, onCalendar)

	var execLine string
	if command != "" {
		execLine = "ExecStart=" + command
	} else {
		execLine = "ExecStart=/bin/true\n# FIXME: could not extract command from cron entry"
	}

	serviceContent = fmt.Sprintf("[Unit]\nDescription=Timer from cron %s\n\n"+
		"[Service]\nType=oneshot\n%s\n",
		path, execLine)

	return timerContent, serviceContent
}

// ---------------------------------------------------------------------------
// Systemd timer scanning
// ---------------------------------------------------------------------------

// parseUnitField extracts the first value of field= from unit file text.
func parseUnitField(text, field string) string {
	prefix := field + "="
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, prefix) {
			return strings.TrimSpace(strings.SplitN(line, "=", 2)[1])
		}
	}
	return ""
}

// scanSystemdTimers scans a systemd unit directory for .timer files and
// their paired .service units.
func scanSystemdTimers(exec Executor, baseDir, sourceLabel string, systemType schema.SystemType) []schema.SystemdTimer {
	var results []schema.SystemdTimer

	dirPath := "/" + baseDir
	entries, err := exec.ReadDir(dirPath)
	if err != nil {
		return results
	}

	isOstree := systemType == schema.SystemTypeRpmOstree || systemType == schema.SystemTypeBootc

	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".timer") {
			continue
		}

		timerPath := dirPath + "/" + e.Name()
		timerText, err := exec.ReadFile(timerPath)
		if err != nil || timerText == "" {
			continue
		}

		name := strings.TrimSuffix(e.Name(), ".timer")
		onCalendar := parseUnitField(timerText, "OnCalendar")
		description := parseUnitField(timerText, "Description")

		servicePath := dirPath + "/" + name + ".service"
		serviceText := ""
		if exec.FileExists(servicePath) {
			serviceText, _ = exec.ReadFile(servicePath)
		}
		execStart := parseUnitField(serviceText, "ExecStart")

		// On ostree systems, timers under /usr/lib/systemd/ are always vendor
		source := sourceLabel
		if isOstree && strings.HasPrefix(baseDir, "usr/lib/systemd") {
			source = "vendor"
		}

		results = append(results, schema.SystemdTimer{
			Name:           name,
			OnCalendar:     onCalendar,
			ExecStart:      execStart,
			Description:    description,
			Source:         source,
			Path:           strings.TrimPrefix(timerPath, "/"),
			TimerContent:   timerText,
			ServiceContent: serviceText,
		})
	}
	return results
}

// ---------------------------------------------------------------------------
// At job scanning
// ---------------------------------------------------------------------------

// atUidRe matches "# atrun uid=NNNN" lines in at spool files.
var atUidRe = regexp.MustCompile(`^# atrun uid=(\d+)`)

// parseAtJob parses an at spool file to extract command, user, and working dir.
func parseAtJob(content, relPath string) schema.AtJob {
	if content == "" {
		return schema.AtJob{File: relPath}
	}

	var (
		user       string
		workingDir string
		cmdLines   []string
		inPreamble = true
	)

	for _, line := range strings.Split(content, "\n") {
		stripped := strings.TrimSpace(line)

		if m := atUidRe.FindStringSubmatch(stripped); m != nil {
			user = "uid=" + m[1]
		}
		if strings.HasPrefix(stripped, "# mail ") {
			parts := strings.Fields(stripped)
			if len(parts) >= 3 {
				user = parts[2]
			}
		}
		if strings.HasPrefix(stripped, "cd ") && inPreamble {
			fields := strings.Fields(stripped)
			if len(fields) > 1 {
				wd := strings.TrimRight(fields[1], "|")
				// "cd /root || {" -> extract /root
				if idx := strings.Index(wd, "||"); idx >= 0 {
					wd = strings.TrimSpace(wd[:idx])
				}
				workingDir = wd
			}
			continue
		}
		if inPreamble && isPreambleLine(stripped) {
			continue
		}
		inPreamble = false
		if stripped != "" {
			cmdLines = append(cmdLines, stripped)
		}
	}

	command := strings.Join(cmdLines, "; ")
	return schema.AtJob{
		File:       relPath,
		Command:    command,
		User:       user,
		WorkingDir: workingDir,
	}
}

// isPreambleLine returns true for at-spool preamble lines that should be
// skipped when extracting the actual command.
func isPreambleLine(line string) bool {
	if line == "" || line == "}" {
		return true
	}
	if strings.HasPrefix(line, "#!/") || strings.HasPrefix(line, "#") {
		return true
	}
	if strings.HasPrefix(line, "umask") {
		return true
	}
	if strings.HasPrefix(line, "cd ") {
		return true
	}
	if strings.Contains(line, "export") {
		return true
	}
	if strings.HasPrefix(line, "SHELL=") {
		return true
	}
	if strings.HasPrefix(line, "echo") && strings.Contains(line, "inaccessible") {
		return true
	}
	if strings.HasPrefix(line, "exit") {
		return true
	}
	return false
}

// scanAtJobs scans /var/spool/at for at job files.
func scanAtJobs(exec Executor, section *schema.ScheduledTaskSection) {
	entries, err := exec.ReadDir("/var/spool/at")
	if err != nil {
		return
	}
	for _, e := range entries {
		if e.IsDir() || strings.HasPrefix(e.Name(), ".") {
			continue
		}
		filePath := "/var/spool/at/" + e.Name()
		content, _ := exec.ReadFile(filePath)
		relPath := strings.TrimPrefix(filePath, "/")
		section.AtJobs = append(section.AtJobs, parseAtJob(content, relPath))
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// isDigits is defined in rpm.go (shared helper).

// mustAtoi converts a digit-only string to int. Callers must pre-check
// with isDigits.
func mustAtoi(s string) int {
	n := 0
	for _, c := range s {
		n = n*10 + int(c-'0')
	}
	return n
}
