package schema

import (
	"encoding/json"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// TestSystemTypeJSON verifies round-trip JSON marshalling for all three
// SystemType enum values and rejection of unknown values.
func TestSystemTypeJSON(t *testing.T) {
	tests := []struct {
		val  SystemType
		json string
	}{
		{SystemTypePackageMode, `"package-mode"`},
		{SystemTypeRpmOstree, `"rpm-ostree"`},
		{SystemTypeBootc, `"bootc"`},
	}
	for _, tc := range tests {
		t.Run(string(tc.val), func(t *testing.T) {
			// Marshal
			got, err := json.Marshal(tc.val)
			require.NoError(t, err)
			assert.Equal(t, tc.json, string(got))

			// Unmarshal
			var st SystemType
			err = json.Unmarshal([]byte(tc.json), &st)
			require.NoError(t, err)
			assert.Equal(t, tc.val, st)
		})
	}

	// Unknown value must fail.
	var st SystemType
	err := json.Unmarshal([]byte(`"unknown"`), &st)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "unknown SystemType")
}

// TestOsReleaseJSON verifies round-trip with all fields populated.
func TestOsReleaseJSON(t *testing.T) {
	orig := OsRelease{
		Name:       "Red Hat Enterprise Linux",
		VersionID:  "9.4",
		Version:    "9.4 (Plow)",
		ID:         "rhel",
		IDLike:     "fedora",
		PrettyName: "Red Hat Enterprise Linux 9.4 (Plow)",
		VariantID:  "workstation",
	}
	data, err := json.Marshal(orig)
	require.NoError(t, err)

	// Verify snake_case keys.
	var raw map[string]interface{}
	require.NoError(t, json.Unmarshal(data, &raw))
	assert.Contains(t, raw, "version_id")
	assert.Contains(t, raw, "id_like")
	assert.Contains(t, raw, "pretty_name")
	assert.Contains(t, raw, "variant_id")

	// Round-trip.
	var decoded OsRelease
	require.NoError(t, json.Unmarshal(data, &decoded))
	assert.Equal(t, orig, decoded)
}

// TestRpmSectionJSON verifies round-trip with representative data
// including nullable fields.
func TestRpmSectionJSON(t *testing.T) {
	baseImg := "quay.io/centos-bootc/centos-bootc:stream9"
	vlCmd := "versionlock list"
	leafPkgs := []string{"httpd", "nginx"}

	orig := RpmSection{
		PackagesAdded: []PackageEntry{
			{
				Name:    "httpd",
				Epoch:   "0",
				Version: "2.4.57",
				Release: "5.el9",
				Arch:    "x86_64",
				State:   PackageStateAdded,
				Include: true,
			},
		},
		BaseImageOnly: []PackageEntry{},
		RpmVa: []RpmVaEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Flags: "S.5....T."},
		},
		RepoFiles:              []RepoFile{},
		GpgKeys:                []RepoFile{},
		DnfHistoryRemoved:      []string{"removed-pkg"},
		VersionChanges:         []VersionChange{},
		LeafPackages:           &leafPkgs,
		AutoPackages:           nil,
		LeafDepTree:            nil,
		ModuleStreams:          []EnabledModuleStream{},
		VersionLocks:          []VersionLockEntry{},
		ModuleStreamConflicts: []string{},
		BaselineModuleStreams:  nil,
		VersionlockCommandOutput: &vlCmd,
		MultiarchPackages:      []string{},
		DuplicatePackages:      []string{},
		RepoProvidingPackages:  []string{},
		OstreeOverrides:        []OstreePackageOverride{},
		OstreeRemovals:         []string{},
		BaseImage:              &baseImg,
		BaselinePackageNames:   nil,
		NoBaseline:             false,
	}

	data, err := json.Marshal(orig)
	require.NoError(t, err)

	// Verify key JSON field names.
	var raw map[string]interface{}
	require.NoError(t, json.Unmarshal(data, &raw))
	assert.Contains(t, raw, "packages_added")
	assert.Contains(t, raw, "base_image_only")
	assert.Contains(t, raw, "rpm_va")
	assert.Contains(t, raw, "dnf_history_removed")
	assert.Contains(t, raw, "leaf_packages")
	assert.Contains(t, raw, "versionlock_command_output")
	assert.Contains(t, raw, "base_image")
	assert.Contains(t, raw, "no_baseline")
	assert.Contains(t, raw, "ostree_overrides")
	assert.Contains(t, raw, "ostree_removals")
	assert.Contains(t, raw, "baseline_package_names")
	// auto_packages should be null
	assert.Nil(t, raw["auto_packages"])

	// Round-trip.
	var decoded RpmSection
	require.NoError(t, json.Unmarshal(data, &decoded))
	assert.Equal(t, orig.BaseImage, decoded.BaseImage)
	assert.Equal(t, *orig.LeafPackages, *decoded.LeafPackages)
	assert.Nil(t, decoded.AutoPackages)
	require.Len(t, decoded.PackagesAdded, 1)
	assert.Equal(t, "httpd", decoded.PackagesAdded[0].Name)
	assert.Equal(t, PackageStateAdded, decoded.PackagesAdded[0].State)
}

// TestConfigSectionJSON verifies round-trip with files.
func TestConfigSectionJSON(t *testing.T) {
	pkg := "httpd"
	flags := "S.5....T."
	orig := ConfigSection{
		Files: []ConfigFileEntry{
			{
				Path:       "/etc/httpd/conf/httpd.conf",
				Kind:       ConfigFileKindRpmOwnedModified,
				Category:   ConfigCategoryOther,
				Content:    "ServerRoot /etc/httpd",
				RpmVaFlags: &flags,
				Package:    &pkg,
				Include:    true,
			},
			{
				Path:    "/etc/custom/app.conf",
				Kind:    ConfigFileKindUnowned,
				Category: ConfigCategoryOther,
				Include: true,
			},
		},
	}

	data, err := json.Marshal(orig)
	require.NoError(t, err)

	// Verify field names.
	var raw map[string]interface{}
	require.NoError(t, json.Unmarshal(data, &raw))
	assert.Contains(t, raw, "files")
	files := raw["files"].([]interface{})
	require.Len(t, files, 2)
	f0 := files[0].(map[string]interface{})
	assert.Contains(t, f0, "rpm_va_flags")
	assert.Contains(t, f0, "diff_against_rpm")
	assert.Contains(t, f0, "tie_winner")

	// Round-trip.
	var decoded ConfigSection
	require.NoError(t, json.Unmarshal(data, &decoded))
	require.Len(t, decoded.Files, 2)
	assert.Equal(t, ConfigFileKindRpmOwnedModified, decoded.Files[0].Kind)
	assert.Equal(t, &pkg, decoded.Files[0].Package)
	assert.Nil(t, decoded.Files[1].RpmVaFlags)
}

// TestPackageEntryDefaults verifies that a zero-value PackageEntry
// produces correct JSON for fields with Python defaults.
func TestPackageEntryDefaults(t *testing.T) {
	pe := PackageEntry{
		Name:    "test-pkg",
		Version: "1.0",
		Release: "1.el9",
		Arch:    "x86_64",
	}

	data, err := json.Marshal(pe)
	require.NoError(t, err)

	var raw map[string]interface{}
	require.NoError(t, json.Unmarshal(data, &raw))

	// epoch defaults to "" (Go zero) but Python defaults to "0" — caller
	// must set it. The JSON field name must be present.
	assert.Contains(t, raw, "epoch")
	assert.Contains(t, raw, "state")
	assert.Contains(t, raw, "include")
	assert.Contains(t, raw, "source_repo")
	assert.Contains(t, raw, "fleet")

	// state zero-value is "" in Go; Python default is "added".
	// Callers constructing PackageEntry must set State explicitly.
	// The field must be present in JSON regardless.
	_, statePresent := raw["state"]
	assert.True(t, statePresent)
}

// TestInspectionSnapshotJSON verifies the root snapshot round-trips.
func TestInspectionSnapshotJSON(t *testing.T) {
	snap := InspectionSnapshot{
		SchemaVersion: SchemaVersion,
		Meta:          map[string]interface{}{"hostname": "test-host"},
		SystemType:    SystemTypePackageMode,
		Preflight: PreflightResult{
			Status: "skipped",
		},
		Warnings:   []map[string]interface{}{},
		Redactions: []json.RawMessage{},
	}

	data, err := json.Marshal(snap)
	require.NoError(t, err)

	var raw map[string]interface{}
	require.NoError(t, json.Unmarshal(data, &raw))
	assert.Equal(t, float64(SchemaVersion), raw["schema_version"])
	assert.Equal(t, "package-mode", raw["system_type"])
	assert.Nil(t, raw["rpm"])
	assert.Nil(t, raw["os_release"])

	var decoded InspectionSnapshot
	require.NoError(t, json.Unmarshal(data, &decoded))
	assert.Equal(t, SystemTypePackageMode, decoded.SystemType)
	assert.Equal(t, "skipped", decoded.Preflight.Status)
}

// TestFleetPrevalenceJSON verifies fleet metadata round-trips.
func TestFleetPrevalenceJSON(t *testing.T) {
	fp := FleetPrevalence{
		Count: 3,
		Total: 5,
		Hosts: []string{"host-a", "host-b", "host-c"},
	}

	data, err := json.Marshal(fp)
	require.NoError(t, err)

	var decoded FleetPrevalence
	require.NoError(t, json.Unmarshal(data, &decoded))
	assert.Equal(t, fp, decoded)
}

// TestServiceSectionJSON verifies service types round-trip.
func TestServiceSectionJSON(t *testing.T) {
	pkg := "httpd"
	orig := ServiceSection{
		StateChanges: []ServiceStateChange{
			{
				Unit:          "httpd.service",
				CurrentState:  "enabled",
				DefaultState:  "disabled",
				Action:        "enable",
				Include:       true,
				OwningPackage: &pkg,
			},
		},
		EnabledUnits:  []string{"httpd.service"},
		DisabledUnits: []string{},
		DropIns: []SystemdDropIn{
			{
				Unit:    "httpd.service",
				Path:    "etc/systemd/system/httpd.service.d/override.conf",
				Content: "[Service]\nTimeoutSec=300",
				Include: true,
			},
		},
	}

	data, err := json.Marshal(orig)
	require.NoError(t, err)

	var decoded ServiceSection
	require.NoError(t, json.Unmarshal(data, &decoded))
	require.Len(t, decoded.StateChanges, 1)
	assert.Equal(t, &pkg, decoded.StateChanges[0].OwningPackage)
	require.Len(t, decoded.DropIns, 1)
	assert.Equal(t, "httpd.service", decoded.DropIns[0].Unit)
}

// TestRedactionFindingJSON verifies redaction types round-trip with
// nullable fields.
func TestRedactionFindingJSON(t *testing.T) {
	line := 42
	repl := "REDACTED"
	conf := "high"
	orig := RedactionFinding{
		Path:            "/etc/shadow",
		Source:          "shadow",
		Kind:            "excluded",
		Pattern:         "shadow_file",
		Remediation:     "provision",
		Line:            &line,
		Replacement:     &repl,
		DetectionMethod: "pattern",
		Confidence:      &conf,
	}

	data, err := json.Marshal(orig)
	require.NoError(t, err)

	var raw map[string]interface{}
	require.NoError(t, json.Unmarshal(data, &raw))
	assert.Contains(t, raw, "detection_method")

	var decoded RedactionFinding
	require.NoError(t, json.Unmarshal(data, &decoded))
	assert.Equal(t, orig, decoded)

	// Null optional fields
	orig2 := RedactionFinding{
		Path:            "/etc/app.conf",
		Source:          "file",
		Kind:            "inline",
		Pattern:         "api_key",
		Remediation:     "regenerate",
		DetectionMethod: "heuristic",
	}
	data2, err := json.Marshal(orig2)
	require.NoError(t, err)

	var decoded2 RedactionFinding
	require.NoError(t, json.Unmarshal(data2, &decoded2))
	assert.Nil(t, decoded2.Line)
	assert.Nil(t, decoded2.Replacement)
	assert.Nil(t, decoded2.Confidence)
}
