package renderer

import (
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// SyncServiceDecisions tests
// ---------------------------------------------------------------------------

func TestSyncServiceDecisions_ExcludeRemovesFromBothLists(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Services = &schema.ServiceSection{
		EnabledUnits:  []string{"foo.service"},
		DisabledUnits: []string{"bar.service"},
		StateChanges: []schema.ServiceStateChange{
			{Unit: "foo.service", Action: "enable", Include: false},
			{Unit: "bar.service", Action: "disable", Include: false},
		},
	}

	SyncServiceDecisions(snap)

	assert.Empty(t, snap.Services.EnabledUnits)
	assert.Empty(t, snap.Services.DisabledUnits)
}

func TestSyncServiceDecisions_EnableAddsToEnabledUnits(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Services = &schema.ServiceSection{
		EnabledUnits:  []string{},
		DisabledUnits: []string{},
		StateChanges: []schema.ServiceStateChange{
			{Unit: "new.service", Action: "enable", Include: true},
		},
	}

	SyncServiceDecisions(snap)

	assert.Equal(t, []string{"new.service"}, snap.Services.EnabledUnits)
	assert.Empty(t, snap.Services.DisabledUnits)
}

func TestSyncServiceDecisions_DisableAddsToDisabledUnits(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Services = &schema.ServiceSection{
		EnabledUnits:  []string{},
		DisabledUnits: []string{},
		StateChanges: []schema.ServiceStateChange{
			{Unit: "stopped.service", Action: "disable", Include: true},
		},
	}

	SyncServiceDecisions(snap)

	assert.Empty(t, snap.Services.EnabledUnits)
	assert.Equal(t, []string{"stopped.service"}, snap.Services.DisabledUnits)
}

func TestSyncServiceDecisions_MaskCollapsesToDisable(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Services = &schema.ServiceSection{
		EnabledUnits:  []string{"masked.service"},
		DisabledUnits: []string{},
		StateChanges: []schema.ServiceStateChange{
			{Unit: "masked.service", Action: "mask", Include: true},
		},
	}

	SyncServiceDecisions(snap)

	assert.Empty(t, snap.Services.EnabledUnits, "mask should remove from EnabledUnits")
	assert.Equal(t, []string{"masked.service"}, snap.Services.DisabledUnits, "mask should add to DisabledUnits")
}

func TestSyncServiceDecisions_UnchangedIsNoop(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Services = &schema.ServiceSection{
		EnabledUnits:  []string{"keep.service"},
		DisabledUnits: []string{"also-keep.service"},
		StateChanges: []schema.ServiceStateChange{
			{Unit: "keep.service", Action: "unchanged", Include: true},
			{Unit: "also-keep.service", Action: "unchanged", Include: true},
		},
	}

	SyncServiceDecisions(snap)

	assert.Equal(t, []string{"keep.service"}, snap.Services.EnabledUnits)
	assert.Equal(t, []string{"also-keep.service"}, snap.Services.DisabledUnits)
}

func TestSyncServiceDecisions_NilServicesIsGraceful(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Services = nil

	require.NotPanics(t, func() {
		SyncServiceDecisions(snap)
	})
}

func TestSyncServiceDecisions_EnableDoesNotDuplicate(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Services = &schema.ServiceSection{
		EnabledUnits:  []string{"already.service"},
		DisabledUnits: []string{},
		StateChanges: []schema.ServiceStateChange{
			{Unit: "already.service", Action: "enable", Include: true},
		},
	}

	SyncServiceDecisions(snap)

	assert.Equal(t, []string{"already.service"}, snap.Services.EnabledUnits,
		"should not duplicate a unit already present")
}

func TestSyncServiceDecisions_MixedActions(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Services = &schema.ServiceSection{
		EnabledUnits:  []string{"will-mask.service"},
		DisabledUnits: []string{"will-exclude.service"},
		StateChanges: []schema.ServiceStateChange{
			{Unit: "new-enable.service", Action: "enable", Include: true},
			{Unit: "new-disable.service", Action: "disable", Include: true},
			{Unit: "will-mask.service", Action: "mask", Include: true},
			{Unit: "will-exclude.service", Action: "disable", Include: false},
			{Unit: "noop.service", Action: "unchanged", Include: true},
		},
	}

	SyncServiceDecisions(snap)

	assert.Contains(t, snap.Services.EnabledUnits, "new-enable.service")
	assert.NotContains(t, snap.Services.EnabledUnits, "will-mask.service")
	assert.Contains(t, snap.Services.DisabledUnits, "new-disable.service")
	assert.Contains(t, snap.Services.DisabledUnits, "will-mask.service")
	assert.NotContains(t, snap.Services.DisabledUnits, "will-exclude.service")
}

// ---------------------------------------------------------------------------
// SyncCronDecisions tests
// ---------------------------------------------------------------------------

func TestSyncCronDecisions_ExcludedCronPropagatesToTimers(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.ScheduledTasks = &schema.ScheduledTaskSection{
		CronJobs: []schema.CronJob{
			{Path: "/etc/cron.d/backup", Include: false},
		},
		GeneratedTimerUnits: []schema.GeneratedTimerUnit{
			{Name: "backup-1.timer", SourcePath: "/etc/cron.d/backup", Include: true},
			{Name: "backup-2.timer", SourcePath: "/etc/cron.d/backup", Include: true},
		},
	}

	SyncCronDecisions(snap)

	for _, tu := range snap.ScheduledTasks.GeneratedTimerUnits {
		assert.False(t, tu.Include, "timer %s should be excluded", tu.Name)
	}
}

func TestSyncCronDecisions_IncludedCronPropagatesToTimers(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.ScheduledTasks = &schema.ScheduledTaskSection{
		CronJobs: []schema.CronJob{
			{Path: "/etc/cron.d/backup", Include: true},
		},
		GeneratedTimerUnits: []schema.GeneratedTimerUnit{
			{Name: "backup-1.timer", SourcePath: "/etc/cron.d/backup", Include: false},
		},
	}

	SyncCronDecisions(snap)

	assert.True(t, snap.ScheduledTasks.GeneratedTimerUnits[0].Include,
		"timer should be included when cron is included")
}

func TestSyncCronDecisions_MultipleFilesIndependent(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.ScheduledTasks = &schema.ScheduledTaskSection{
		CronJobs: []schema.CronJob{
			{Path: "/etc/cron.d/backup", Include: false},
			{Path: "/etc/cron.d/cleanup", Include: true},
		},
		GeneratedTimerUnits: []schema.GeneratedTimerUnit{
			{Name: "backup.timer", SourcePath: "/etc/cron.d/backup", Include: true},
			{Name: "cleanup.timer", SourcePath: "/etc/cron.d/cleanup", Include: false},
		},
	}

	SyncCronDecisions(snap)

	assert.False(t, snap.ScheduledTasks.GeneratedTimerUnits[0].Include,
		"backup timer should follow its cron (excluded)")
	assert.True(t, snap.ScheduledTasks.GeneratedTimerUnits[1].Include,
		"cleanup timer should follow its cron (included)")
}

func TestSyncCronDecisions_NilScheduledTasksIsGraceful(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.ScheduledTasks = nil

	require.NotPanics(t, func() {
		SyncCronDecisions(snap)
	})
}

func TestSyncCronDecisions_NoMatchingSourcePathLeavesTimerUntouched(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.ScheduledTasks = &schema.ScheduledTaskSection{
		CronJobs: []schema.CronJob{
			{Path: "/etc/cron.d/backup", Include: false},
		},
		GeneratedTimerUnits: []schema.GeneratedTimerUnit{
			{Name: "other.timer", SourcePath: "/etc/cron.d/other", Include: true},
		},
	}

	SyncCronDecisions(snap)

	assert.True(t, snap.ScheduledTasks.GeneratedTimerUnits[0].Include,
		"timer with unmatched SourcePath should be left untouched")
}

func TestSyncCronDecisions_EmptyCronJobsLeavesTimersUntouched(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.ScheduledTasks = &schema.ScheduledTaskSection{
		CronJobs: []schema.CronJob{},
		GeneratedTimerUnits: []schema.GeneratedTimerUnit{
			{Name: "orphan.timer", SourcePath: "/etc/cron.d/gone", Include: true},
		},
	}

	SyncCronDecisions(snap)

	assert.True(t, snap.ScheduledTasks.GeneratedTimerUnits[0].Include)
}

// ---------------------------------------------------------------------------
// Helper tests
// ---------------------------------------------------------------------------

func TestRemoveFromSlice(t *testing.T) {
	tests := []struct {
		name   string
		input  []string
		remove string
		want   []string
	}{
		{"removes existing", []string{"a", "b", "c"}, "b", []string{"a", "c"}},
		{"not found", []string{"a", "b"}, "z", []string{"a", "b"}},
		{"empty slice", []string{}, "a", []string{}},
		{"removes all occurrences", []string{"a", "b", "a"}, "a", []string{"b"}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := removeFromSlice(tt.input, tt.remove)
			assert.Equal(t, tt.want, got)
		})
	}
}

func TestContainsString(t *testing.T) {
	assert.True(t, containsString([]string{"a", "b"}, "a"))
	assert.False(t, containsString([]string{"a", "b"}, "z"))
	assert.False(t, containsString([]string{}, "a"))
}
