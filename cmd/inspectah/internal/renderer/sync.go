package renderer

import "github.com/marrusl/inspectah/cmd/inspectah/internal/schema"

// SyncServiceDecisions rebuilds EnabledUnits and DisabledUnits from
// StateChanges so that the Containerfile renderer sees correct service
// state after user toggles in the SPA.
//
// For each StateChange entry:
//   - Include == false: remove from both EnabledUnits and DisabledUnits
//   - Include == true, Action "enable":    add to EnabledUnits if absent
//   - Include == true, Action "disable":   add to DisabledUnits if absent
//   - Include == true, Action "mask":      remove from EnabledUnits, add to DisabledUnits
//   - Include == true, Action "unchanged": no-op
func SyncServiceDecisions(snap *schema.InspectionSnapshot) {
	if snap.Services == nil {
		return
	}

	for _, sc := range snap.Services.StateChanges {
		if !sc.Include {
			snap.Services.EnabledUnits = removeFromSlice(snap.Services.EnabledUnits, sc.Unit)
			snap.Services.DisabledUnits = removeFromSlice(snap.Services.DisabledUnits, sc.Unit)
			continue
		}

		switch sc.Action {
		case "enable":
			if !containsString(snap.Services.EnabledUnits, sc.Unit) {
				snap.Services.EnabledUnits = append(snap.Services.EnabledUnits, sc.Unit)
			}
		case "disable":
			if !containsString(snap.Services.DisabledUnits, sc.Unit) {
				snap.Services.DisabledUnits = append(snap.Services.DisabledUnits, sc.Unit)
			}
		case "mask":
			// Collapse mask to disable — no separate mask render path.
			snap.Services.EnabledUnits = removeFromSlice(snap.Services.EnabledUnits, sc.Unit)
			if !containsString(snap.Services.DisabledUnits, sc.Unit) {
				snap.Services.DisabledUnits = append(snap.Services.DisabledUnits, sc.Unit)
			}
		case "unchanged":
			// No-op — unit stays wherever it already is.
		}
	}
}

// SyncCronDecisions propagates CronJob.Include state to all
// GeneratedTimerUnit entries sharing the same SourcePath.
func SyncCronDecisions(snap *schema.InspectionSnapshot) {
	if snap.ScheduledTasks == nil {
		return
	}

	// Build a map of cron path → Include state.
	cronInclude := make(map[string]bool, len(snap.ScheduledTasks.CronJobs))
	for _, cj := range snap.ScheduledTasks.CronJobs {
		cronInclude[cj.Path] = cj.Include
	}

	// Propagate to matching generated timer units.
	for i := range snap.ScheduledTasks.GeneratedTimerUnits {
		tu := &snap.ScheduledTasks.GeneratedTimerUnits[i]
		if include, ok := cronInclude[tu.SourcePath]; ok {
			tu.Include = include
		}
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// removeFromSlice returns a new slice with all occurrences of target removed.
func removeFromSlice(slice []string, target string) []string {
	result := make([]string, 0, len(slice))
	for _, s := range slice {
		if s != target {
			result = append(result, s)
		}
	}
	return result
}

// containsString reports whether slice contains target.
func containsString(slice []string, target string) bool {
	for _, s := range slice {
		if s == target {
			return true
		}
	}
	return false
}
