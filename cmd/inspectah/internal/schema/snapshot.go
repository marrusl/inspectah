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

	if snap.SchemaVersion != SchemaVersion {
		return nil, fmt.Errorf("schema version mismatch: file has %d, expected %d",
			snap.SchemaVersion, SchemaVersion)
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
