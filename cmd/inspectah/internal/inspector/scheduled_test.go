package inspector

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

func loadScheduledFixture(t *testing.T, name string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "scheduled", name))
	require.NoError(t, err, "loading fixture %s", name)
	return string(data)
}

// ---------------------------------------------------------------------------
// CronToOnCalendar -- table-driven
// ---------------------------------------------------------------------------

func TestCronToOnCalendar(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		wantCal  string
		wantConv bool
	}{
		// Named shortcuts
		{"@daily", "@daily", "*-*-* 00:00:00", true},
		{"@weekly", "@weekly", "Mon *-*-* 00:00:00", true},
		{"@monthly", "@monthly", "*-*-01 00:00:00", true},
		{"@yearly", "@yearly", "*-01-01 00:00:00", true},
		{"@annually", "@annually", "*-01-01 00:00:00", true},
		{"@hourly", "@hourly", "*-*-* *:00:00", true},
		{"@midnight", "@midnight", "*-*-* 00:00:00", true},
		{"@reboot", "@reboot", "@reboot", false},

		// Standard 5-field expressions
		{"every minute", "* * * * *", "*-*-* *:*:00", true},
		{"daily at 3am", "0 3 * * *", "*-*-* 03:00:00", true},
		{"every 15 min", "*/15 * * * *", "*-*-* *:*/15:00", true},
		{"mon-fri 9am", "0 9 * * 1-5", "Mon..Fri *-*-* 09:00:00", true},
		{"first of month", "0 0 1 * *", "*-*-1 00:00:00", true},
		{"step hours", "0 */2 * * *", "*-*-* 00/2:00:00", true},
		{"named dow", "0 8 * * mon", "Mon *-*-* 08:00:00", true},
		{"named month", "0 0 1 jan *", "*-1-1 00:00:00", true},
		{"list minutes", "0,15,30,45 * * * *", "*-*-* *:0,15,30,45:00", true},
		{"range+step", "0 9-17/2 * * *", "*-*-* 9..17/2:00:00", true},
		{"dow numeric 0=Sun", "0 12 * * 0", "Sun *-*-* 12:00:00", true},
		{"dow numeric 7=Sun", "0 12 * * 7", "Sun *-*-* 12:00:00", true},

		// Fallback for too few fields
		{"bad expr", "*/5 *", "*-*-* 02:00:00", false},

		// Case-insensitive shortcuts
		{"@DAILY upper", "@DAILY", "*-*-* 00:00:00", true},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			cal, conv := CronToOnCalendar(tc.input)
			assert.Equal(t, tc.wantCal, cal, "OnCalendar")
			assert.Equal(t, tc.wantConv, conv, "converted")
		})
	}
}

// ---------------------------------------------------------------------------
// cronFieldToCalendar -- unit tests
// ---------------------------------------------------------------------------

func TestCronFieldToCalendar(t *testing.T) {
	tests := []struct {
		name  string
		field string
		kind  string
		want  string
	}{
		{"star", "*", "minute", "*"},
		{"step minute", "*/5", "minute", "*/5"},
		{"step hour", "*/2", "hour", "00/2"},
		{"range", "1-5", "dom", "1..5"},
		{"range+step", "9-17/2", "hour", "9..17/2"},
		{"list", "1,3,5", "minute", "1,3,5"},
		{"named month", "jan", "month", "1"},
		{"named dow", "fri", "dow", "Fri"},
		{"numeric dow", "0", "dow", "Sun"},
		{"plain digit minute", "5", "minute", "05"},
		{"plain digit hour", "3", "hour", "03"},
		{"plain digit dom", "15", "dom", "15"},
		{"passthrough", "L", "dom", "L"},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := cronFieldToCalendar(tc.field, tc.kind)
			assert.Equal(t, tc.want, got)
		})
	}
}

// ---------------------------------------------------------------------------
// makeTimerService
// ---------------------------------------------------------------------------

func TestMakeTimerService(t *testing.T) {
	t.Run("normal cron", func(t *testing.T) {
		timer, service := makeTimerService("cron-backup", "0 3 * * *", "etc/cron.d/backup", "/opt/backup.sh")
		assert.Contains(t, timer, "OnCalendar=*-*-* 03:00:00")
		assert.Contains(t, timer, "Description=Generated from cron: etc/cron.d/backup")
		assert.Contains(t, timer, "Persistent=true")
		assert.Contains(t, timer, "WantedBy=timers.target")
		assert.Contains(t, service, "ExecStart=/opt/backup.sh")
		assert.Contains(t, service, "Type=oneshot")
		assert.NotContains(t, timer, "FIXME")
	})

	t.Run("@reboot adds FIXME", func(t *testing.T) {
		timer, service := makeTimerService("cron-startup", "@reboot", "etc/cron.d/startup", "/opt/init.sh")
		assert.Contains(t, timer, "FIXME: @reboot has no OnCalendar equivalent")
		assert.Contains(t, timer, "OnCalendar=*-*-* 02:00:00")
		assert.Contains(t, service, "ExecStart=/opt/init.sh")
	})

	t.Run("bad expr adds FIXME", func(t *testing.T) {
		timer, _ := makeTimerService("cron-bad", "*/5 *", "etc/cron.d/bad", "/opt/bad.sh")
		assert.Contains(t, timer, "FIXME: cron expression '*/5 *' could not be fully converted")
	})

	t.Run("empty command", func(t *testing.T) {
		_, service := makeTimerService("cron-nocmd", "0 3 * * *", "etc/cron.d/nocmd", "")
		assert.Contains(t, service, "ExecStart=/bin/true")
		assert.Contains(t, service, "FIXME: could not extract command")
	})
}

// ---------------------------------------------------------------------------
// parseUnitField
// ---------------------------------------------------------------------------

func TestParseUnitField(t *testing.T) {
	content := loadScheduledFixture(t, "cleanup-timer")

	assert.Equal(t, "*-*-* 04:00:00", parseUnitField(content, "OnCalendar"))
	assert.Equal(t, "Daily cleanup of temp files", parseUnitField(content, "Description"))
	assert.Equal(t, "", parseUnitField(content, "NonExistent"))
}

// ---------------------------------------------------------------------------
// parseAtJob
// ---------------------------------------------------------------------------

func TestParseAtJob(t *testing.T) {
	content := loadScheduledFixture(t, "at-job-sample")
	job := parseAtJob(content, "var/spool/at/a00001")

	assert.Equal(t, "var/spool/at/a00001", job.File)
	assert.Equal(t, "appuser", job.User)
	assert.Equal(t, "/home/appuser", job.WorkingDir)
	assert.Contains(t, job.Command, "/usr/local/bin/run-migration.sh --stage=final")
}

func TestParseAtJobEmpty(t *testing.T) {
	job := parseAtJob("", "var/spool/at/empty")
	assert.Equal(t, "var/spool/at/empty", job.File)
	assert.Empty(t, job.Command)
	assert.Empty(t, job.User)
}

// ---------------------------------------------------------------------------
// parseCronEntries
// ---------------------------------------------------------------------------

func TestParseCronEntries(t *testing.T) {
	t.Run("system crontab with user field", func(t *testing.T) {
		content := loadScheduledFixture(t, "cron-d-logrotate")
		section := &schema.ScheduledTaskSection{
			GeneratedTimerUnits: []schema.GeneratedTimerUnit{},
		}
		parseCronEntries(section, content, "/etc/cron.d/logrotate", "cron.d", "logrotate")

		require.Len(t, section.GeneratedTimerUnits, 1)
		unit := section.GeneratedTimerUnits[0]
		assert.Equal(t, "cron-logrotate", unit.Name)
		assert.Equal(t, "0 3 * * *", unit.CronExpr)
		assert.Equal(t, "/usr/sbin/logrotate /etc/logrotate.conf", unit.Command)
		assert.Contains(t, unit.TimerContent, "OnCalendar=*-*-* 03:00:00")
	})

	t.Run("multiple entries", func(t *testing.T) {
		content := loadScheduledFixture(t, "cron-d-custom-backup")
		section := &schema.ScheduledTaskSection{
			GeneratedTimerUnits: []schema.GeneratedTimerUnit{},
		}
		parseCronEntries(section, content, "/etc/cron.d/custom-backup", "cron.d", "custom-backup")

		require.Len(t, section.GeneratedTimerUnits, 2)
		assert.Equal(t, "*/15 * * * *", section.GeneratedTimerUnits[0].CronExpr)
		assert.Equal(t, "/opt/backup/run-backup.sh --full", section.GeneratedTimerUnits[0].Command)
		assert.Equal(t, "30 2 * * 0", section.GeneratedTimerUnits[1].CronExpr)
	})

	t.Run("user crontab no user field", func(t *testing.T) {
		content := loadScheduledFixture(t, "user-crontab")
		section := &schema.ScheduledTaskSection{
			GeneratedTimerUnits: []schema.GeneratedTimerUnit{},
		}
		parseCronEntries(section, content, "/var/spool/cron/appuser", "spool/cron (appuser)", "appuser")

		require.Len(t, section.GeneratedTimerUnits, 2)
		// User crontab: command starts at field 5 (no user field)
		assert.Equal(t, "/home/appuser/bin/healthcheck.sh", section.GeneratedTimerUnits[0].Command)
		assert.Equal(t, "/home/appuser/bin/weekday-report.sh", section.GeneratedTimerUnits[1].Command)
	})
}

// ---------------------------------------------------------------------------
// RPM-owned filtering
// ---------------------------------------------------------------------------

func TestRpmOwnedFiltering(t *testing.T) {
	rpmOwned := map[string]bool{
		"/etc/cron.d/logrotate":    true,
		"/etc/cron.daily/logwatch": true,
	}

	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/cron.d":     {"logrotate", "custom-backup"},
			"/etc/cron.daily": {"logwatch", "my-cleanup"},
		}).
		WithFiles(map[string]string{
			"/etc/cron.d/logrotate":      "0 3 * * * root /usr/sbin/logrotate /etc/logrotate.conf\n",
			"/etc/cron.d/custom-backup":  "*/30 * * * * root /opt/backup.sh\n",
			"/etc/cron.daily/logwatch":   "#!/bin/sh\n/usr/sbin/logwatch\n",
			"/etc/cron.daily/my-cleanup": "#!/bin/sh\n/opt/cleanup.sh\n",
		})

	section := &schema.ScheduledTaskSection{
		CronJobs:            []schema.CronJob{},
		GeneratedTimerUnits: []schema.GeneratedTimerUnit{},
	}

	// Test cron.d: logrotate is RPM-owned, custom-backup is not
	scanCronDir(exec, section, "/etc/cron.d", "cron.d", rpmOwned)

	require.Len(t, section.CronJobs, 2)

	// logrotate should be marked RPM-owned
	var logrotate, custom *schema.CronJob
	for i := range section.CronJobs {
		if section.CronJobs[i].Path == "etc/cron.d/logrotate" {
			logrotate = &section.CronJobs[i]
		}
		if section.CronJobs[i].Path == "etc/cron.d/custom-backup" {
			custom = &section.CronJobs[i]
		}
	}
	require.NotNil(t, logrotate)
	require.NotNil(t, custom)
	assert.True(t, logrotate.RpmOwned)
	assert.False(t, custom.RpmOwned)

	// Only custom-backup should generate a timer (RPM-owned ones are skipped)
	require.Len(t, section.GeneratedTimerUnits, 1)
	assert.Equal(t, "etc/cron.d/custom-backup", section.GeneratedTimerUnits[0].SourcePath)

	// Test cron.daily: logwatch is RPM-owned, my-cleanup is not
	section2 := &schema.ScheduledTaskSection{
		CronJobs:            []schema.CronJob{},
		GeneratedTimerUnits: []schema.GeneratedTimerUnit{},
	}
	scanCronPeriodDir(exec, section2, "daily", rpmOwned)

	require.Len(t, section2.CronJobs, 2)
	// Only my-cleanup generates a timer
	require.Len(t, section2.GeneratedTimerUnits, 1)
	assert.Contains(t, section2.GeneratedTimerUnits[0].Name, "my-cleanup")
}

// ---------------------------------------------------------------------------
// scanSystemdTimers
// ---------------------------------------------------------------------------

func TestScanSystemdTimers(t *testing.T) {
	timerContent := loadScheduledFixture(t, "cleanup-timer")
	serviceContent := loadScheduledFixture(t, "cleanup-service")

	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/systemd/system": {"cleanup.timer", "cleanup.service", "other.service"},
		}).
		WithFiles(map[string]string{
			"/etc/systemd/system/cleanup.timer":   timerContent,
			"/etc/systemd/system/cleanup.service":  serviceContent,
		})

	timers := scanSystemdTimers(exec, "etc/systemd/system", "local", schema.SystemTypePackageMode)

	require.Len(t, timers, 1)
	assert.Equal(t, "cleanup", timers[0].Name)
	assert.Equal(t, "*-*-* 04:00:00", timers[0].OnCalendar)
	assert.Equal(t, "/usr/local/bin/cleanup-temp.sh", timers[0].ExecStart)
	assert.Equal(t, "Daily cleanup of temp files", timers[0].Description)
	assert.Equal(t, "local", timers[0].Source)
	assert.Equal(t, timerContent, timers[0].TimerContent)
	assert.Equal(t, serviceContent, timers[0].ServiceContent)
}

func TestScanSystemdTimersOstreeVendor(t *testing.T) {
	timerContent := loadScheduledFixture(t, "cleanup-timer")

	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/usr/lib/systemd/system": {"cleanup.timer"},
		}).
		WithFiles(map[string]string{
			"/usr/lib/systemd/system/cleanup.timer": timerContent,
		})

	// On ostree systems, timers under /usr/lib/systemd are always vendor
	timers := scanSystemdTimers(exec, "usr/lib/systemd/system", "local", schema.SystemTypeBootc)
	require.Len(t, timers, 1)
	assert.Equal(t, "vendor", timers[0].Source)
}

// ---------------------------------------------------------------------------
// scanAtJobs
// ---------------------------------------------------------------------------

func TestScanAtJobs(t *testing.T) {
	atContent := loadScheduledFixture(t, "at-job-sample")

	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/var/spool/at": {"a00001", ".SEQ"},
		}).
		WithFiles(map[string]string{
			"/var/spool/at/a00001": atContent,
		})

	section := &schema.ScheduledTaskSection{
		AtJobs: []schema.AtJob{},
	}
	scanAtJobs(exec, section)

	// .SEQ should be skipped (starts with dot)
	require.Len(t, section.AtJobs, 1)
	assert.Equal(t, "var/spool/at/a00001", section.AtJobs[0].File)
	assert.Equal(t, "appuser", section.AtJobs[0].User)
	assert.Contains(t, section.AtJobs[0].Command, "run-migration.sh")
}

// ---------------------------------------------------------------------------
// Full integration test: RunScheduledTasks
// ---------------------------------------------------------------------------

func TestRunScheduledTasks(t *testing.T) {
	cronDContent := "*/5 * * * * root /usr/local/bin/health-check\n"
	dailyScript := "#!/bin/sh\n/opt/daily-report.sh\n"
	timerContent := loadScheduledFixture(t, "cleanup-timer")
	serviceContent := loadScheduledFixture(t, "cleanup-service")
	atContent := loadScheduledFixture(t, "at-job-sample")

	rpmOwned := map[string]bool{
		"/etc/cron.d/0hourly": true,
	}

	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/cron.d":         {"healthcheck", "0hourly"},
			"/etc/cron.daily":     {"daily-report"},
			"/var/spool/cron":     {"appuser"},
			"/etc/systemd/system": {"cleanup.timer", "cleanup.service"},
			"/var/spool/at":       {"a00001"},
		}).
		WithFiles(map[string]string{
			"/etc/cron.d/healthcheck":             cronDContent,
			"/etc/cron.d/0hourly":                 "0 * * * * root /etc/cron.hourly/*\n",
			"/etc/cron.daily/daily-report":         dailyScript,
			"/var/spool/cron/appuser":              "0 12 * * * /home/appuser/noon.sh\n",
			"/etc/systemd/system/cleanup.timer":    timerContent,
			"/etc/systemd/system/cleanup.service":  serviceContent,
			"/var/spool/at/a00001":                 atContent,
		})

	section, warnings, err := RunScheduledTasks(exec, ScheduledTaskOptions{
		RpmOwnedPaths: rpmOwned,
		SystemType:    schema.SystemTypePackageMode,
	})

	require.NoError(t, err)
	assert.Empty(t, warnings)

	// Cron jobs: healthcheck + 0hourly from cron.d, daily-report from cron.daily, appuser from spool
	assert.GreaterOrEqual(t, len(section.CronJobs), 4)

	// 0hourly should be marked RPM-owned
	var found bool
	for _, cj := range section.CronJobs {
		if cj.Path == "etc/cron.d/0hourly" {
			assert.True(t, cj.RpmOwned)
			found = true
		}
	}
	assert.True(t, found, "0hourly should appear in cron jobs")

	// Generated timer units: healthcheck from cron.d, daily-report from cron.daily,
	// appuser from spool. 0hourly is RPM-owned so no timer.
	assert.GreaterOrEqual(t, len(section.GeneratedTimerUnits), 3)

	// Systemd timers: cleanup
	require.Len(t, section.SystemdTimers, 1)
	assert.Equal(t, "cleanup", section.SystemdTimers[0].Name)

	// At jobs: a00001
	require.Len(t, section.AtJobs, 1)
	assert.Contains(t, section.AtJobs[0].Command, "run-migration.sh")
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

func TestRunScheduledTasksEmptySystem(t *testing.T) {
	exec := NewFakeExecutor(nil)

	section, warnings, err := RunScheduledTasks(exec, ScheduledTaskOptions{
		RpmOwnedPaths: map[string]bool{},
		SystemType:    schema.SystemTypePackageMode,
	})

	require.NoError(t, err)
	assert.Empty(t, warnings)
	assert.Empty(t, section.CronJobs)
	assert.Empty(t, section.SystemdTimers)
	assert.Empty(t, section.AtJobs)
	assert.Empty(t, section.GeneratedTimerUnits)
}

func TestCronPeriodDir(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/cron.weekly": {"cleanup"},
		}).
		WithFiles(map[string]string{
			"/etc/cron.weekly/cleanup": "#!/bin/sh\n/opt/weekly-cleanup.sh\n",
		})

	section := &schema.ScheduledTaskSection{
		CronJobs:            []schema.CronJob{},
		GeneratedTimerUnits: []schema.GeneratedTimerUnit{},
	}
	scanCronPeriodDir(exec, section, "weekly", nil)

	require.Len(t, section.CronJobs, 1)
	assert.Equal(t, "cron.weekly", section.CronJobs[0].Source)

	require.Len(t, section.GeneratedTimerUnits, 1)
	unit := section.GeneratedTimerUnits[0]
	assert.Equal(t, "@weekly", unit.CronExpr)
	assert.Contains(t, unit.TimerContent, "OnCalendar=Mon *-*-* 03:00:00")
	assert.Equal(t, "/etc/cron.weekly/cleanup", unit.Command)
	assert.Contains(t, unit.Name, "cron-weekly-cleanup")
}
