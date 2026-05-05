package schema

import (
	"encoding/json"
	"fmt"
	"os"
)

// InspectionSnapshot is the full inspection snapshot, serialised as
// inspection-snapshot.json. All sections are optional so we can run a
// subset of inspectors.
type InspectionSnapshot struct {
	SchemaVersion  int                    `json:"schema_version"`
	Meta           map[string]interface{} `json:"meta"`
	OsRelease      *OsRelease             `json:"os_release"`
	SystemType     SystemType             `json:"system_type"`
	Rpm            *RpmSection            `json:"rpm"`
	Config         *ConfigSection         `json:"config"`
	Services       *ServiceSection        `json:"services"`
	Network        *NetworkSection        `json:"network"`
	Storage        *StorageSection        `json:"storage"`
	ScheduledTasks *ScheduledTaskSection  `json:"scheduled_tasks"`
	Containers     *ContainerSection      `json:"containers"`
	NonRpmSoftware *NonRpmSoftwareSection `json:"non_rpm_software"`
	KernelBoot     *KernelBootSection     `json:"kernel_boot"`
	Selinux        *SelinuxSection        `json:"selinux"`
	UsersGroups    *UserGroupSection      `json:"users_groups"`
	Preflight      PreflightResult        `json:"preflight"`
	Warnings       []map[string]interface{} `json:"warnings"`
	Redactions     []json.RawMessage      `json:"redactions"`
}

// NewSnapshot returns a properly initialized InspectionSnapshot with
// schema version and empty collections.
func NewSnapshot() *InspectionSnapshot {
	return &InspectionSnapshot{
		SchemaVersion: SchemaVersion,
		Meta:          make(map[string]interface{}),
		SystemType:    SystemTypePackageMode,
		Warnings:      []map[string]interface{}{},
		Redactions:    []json.RawMessage{},
	}
}

// LoadSnapshot reads an inspection snapshot from disk and enforces
// schema version match.
func LoadSnapshot(path string) (*InspectionSnapshot, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read snapshot: %w", err)
	}

	var snap InspectionSnapshot
	if err := json.Unmarshal(data, &snap); err != nil {
		return nil, fmt.Errorf("failed to parse snapshot JSON: %w", err)
	}

	if snap.SchemaVersion != SchemaVersion && snap.SchemaVersion != SchemaVersion-1 {
		return nil, fmt.Errorf("schema version mismatch: file has %d, expected %d or %d",
			snap.SchemaVersion, SchemaVersion-1, SchemaVersion)
	}

	// v11→v12 migration: inspector didn't set Include on module streams,
	// leaving the zero value (false). In v12+ the inspector sets Include=true
	// at creation, so false means "user or fleet excluded."
	// v12→v13 migration: new fields (QuadletUnit.Ports/Volumes/Generated,
	// FlatpakApp.Remote/RemoteURL, NonRpmItem.ReviewStatus/Notes) have
	// zero-value defaults that are correct for existing snapshots.
	if snap.SchemaVersion < SchemaVersion && snap.Rpm != nil {
		for i := range snap.Rpm.ModuleStreams {
			if !snap.Rpm.ModuleStreams[i].Include {
				snap.Rpm.ModuleStreams[i].Include = true
			}
		}
	}
	if snap.SchemaVersion < SchemaVersion {
		snap.SchemaVersion = SchemaVersion
	}

	return &snap, nil
}

// SaveSnapshot writes an inspection snapshot to disk as pretty-printed JSON.
func SaveSnapshot(snap *InspectionSnapshot, path string) error {
	data, err := json.MarshalIndent(snap, "", "  ")
	if err != nil {
		return fmt.Errorf("failed to marshal snapshot: %w", err)
	}

	if err := os.WriteFile(path, data, 0644); err != nil {
		return fmt.Errorf("failed to write snapshot: %w", err)
	}

	return nil
}

// NormalizeSnapshot converts all nil *bool Include fields to explicit
// true and sets SchemaVersion to the current version. Called by the
// refine server after loading a snapshot, so the SPA always sees a
// fully normalized snapshot regardless of the original version.
func NormalizeSnapshot(snap *InspectionSnapshot) {
	t := true

	if snap.ScheduledTasks != nil {
		for i := range snap.ScheduledTasks.SystemdTimers {
			if snap.ScheduledTasks.SystemdTimers[i].Include == nil {
				snap.ScheduledTasks.SystemdTimers[i].Include = &t
			}
		}
		for i := range snap.ScheduledTasks.AtJobs {
			if snap.ScheduledTasks.AtJobs[i].Include == nil {
				snap.ScheduledTasks.AtJobs[i].Include = &t
			}
		}
	}
	if snap.Containers != nil {
		for i := range snap.Containers.RunningContainers {
			if snap.Containers.RunningContainers[i].Include == nil {
				snap.Containers.RunningContainers[i].Include = &t
			}
		}
	}
	if snap.Network != nil {
		for i := range snap.Network.Connections {
			if snap.Network.Connections[i].Include == nil {
				snap.Network.Connections[i].Include = &t
			}
		}
	}
	if snap.Storage != nil {
		for i := range snap.Storage.FstabEntries {
			if snap.Storage.FstabEntries[i].Include == nil {
				snap.Storage.FstabEntries[i].Include = &t
			}
		}
	}
	// Normalize untyped map-based Include keys (users, groups, SELinux booleans)
	if snap.UsersGroups != nil {
		for _, u := range snap.UsersGroups.Users {
			if _, ok := u["include"]; !ok {
				u["include"] = true
			}
		}
		for _, g := range snap.UsersGroups.Groups {
			if _, ok := g["include"]; !ok {
				g["include"] = true
			}
		}
	}
	if snap.Selinux != nil {
		for _, b := range snap.Selinux.BooleanOverrides {
			if _, ok := b["include"]; !ok {
				b["include"] = true
			}
		}
	}
	snap.SchemaVersion = SchemaVersion
}

// ParseRedaction attempts to unmarshal a raw redaction entry into a
// RedactionFinding struct. Returns error if the entry doesn't match
// the schema.
func ParseRedaction(raw json.RawMessage) (*RedactionFinding, error) {
	var finding RedactionFinding
	if err := json.Unmarshal(raw, &finding); err != nil {
		return nil, fmt.Errorf("failed to parse redaction: %w", err)
	}
	return &finding, nil
}
