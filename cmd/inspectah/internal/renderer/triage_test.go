package renderer

import (
	"encoding/json"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
)

func TestClassifyPackage_BaseImage(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded:        []schema.PackageEntry{{Name: "coreutils", Arch: "x86_64", State: "installed", SourceRepo: "baseos", Include: true}},
		BaselinePackageNames: &[]string{"coreutils"},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, 1, items[0].Tier)
	assert.Equal(t, "packages", items[0].Section)
}

func TestClassifyPackage_ThirdParty(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{{Name: "epel-pkg", Arch: "x86_64", SourceRepo: "epel", Include: true}},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, 2, items[0].Tier)
	assert.Contains(t, items[0].Reason, "Third-party")
}

func TestClassifyPackage_LocalInstall(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{{Name: "mystery", Arch: "x86_64", State: "local_install", Include: true}},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, 3, items[0].Tier)
}

func TestClassifyPrecedence_HighestTierWins(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "local-and-base", Arch: "x86_64", State: "local_install", SourceRepo: "baseos", Include: true},
		},
		BaselinePackageNames: &[]string{"local-and-base"},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, 3, items[0].Tier, "local_install (tier 3) must win over baseline (tier 1)")
}

func TestClassifySecretPrecedence(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/secret.conf", Kind: "non_rpm", Include: true},
		},
	}
	snap.Redactions = []json.RawMessage{
		json.RawMessage(`{"path":"/etc/secret.conf","finding_type":"api_key"}`),
	}
	items := ClassifySnapshot(snap, nil)

	// Secret-flagged item should appear in secrets section only
	for _, item := range items {
		if item.Name == "/etc/secret.conf" {
			assert.Equal(t, "secrets", item.Section, "secret-flagged item must appear in secrets, not config")
			assert.Equal(t, 3, item.Tier)
		}
	}
	// Should NOT appear in config section
	for _, item := range items {
		if item.Section == "config" && item.Name == "/etc/secret.conf" {
			t.Error("secret-flagged item must not appear in config section")
		}
	}
}

func TestClassifyConfig_RpmDefault(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/default.conf", Kind: schema.ConfigFileKindRpmOwnedDefault, Include: true},
		},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, 1, items[0].Tier)
}

func TestClassifyConfig_Modified(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/modified.conf", Kind: schema.ConfigFileKindRpmOwnedModified, Include: true},
		},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, 2, items[0].Tier)
}

func TestClassifyConfig_QuadletExcluded(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/containers/systemd/app.container", Kind: "non_rpm", Include: true},
		},
	}
	items := ClassifySnapshot(snap, nil)
	for _, item := range items {
		if item.Section == "config" {
			t.Error("quadlet files must not appear in config section")
		}
	}
}

func TestClassifyIdentity_SystemUser(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.UsersGroups = &schema.UserGroupSection{
		Users: []map[string]interface{}{
			{"name": "root", "uid": float64(0)},
		},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, 1, items[0].Tier)
}

func TestClassifyIdentity_UserCreated(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.UsersGroups = &schema.UserGroupSection{
		Users: []map[string]interface{}{
			{"name": "appuser", "uid": float64(1001)},
		},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, 2, items[0].Tier)
}

func TestClassifyContainer_WithoutQuadlet(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		RunningContainers: []schema.RunningContainer{
			{Name: "orphan", Image: "nginx"},
		},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, 3, items[0].Tier)
	assert.Contains(t, items[0].Reason, "without quadlet")
}

func TestClassifySnapshot_EmptySnapshot(t *testing.T) {
	snap := schema.NewSnapshot()
	items := ClassifySnapshot(snap, nil)
	assert.Empty(t, items)
}

func TestClassifySecret_SourcePathPopulated(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/secret.conf", Kind: "non_rpm", Include: true},
			{Path: "/etc/normal.conf", Kind: "non_rpm", Include: true},
		},
	}
	snap.Redactions = []json.RawMessage{
		json.RawMessage(`{"path":"/etc/secret.conf","finding_type":"api_key"}`),
		json.RawMessage(`{"path":"/some/other/path","finding_type":"password"}`),
	}
	items := ClassifySnapshot(snap, nil)

	// Find the secret items
	var secretWithConfig, secretWithoutConfig *TriageItem
	for i := range items {
		if items[i].Section == "secrets" {
			if items[i].Name == "/etc/secret.conf" {
				secretWithConfig = &items[i]
			}
			if items[i].Name == "/some/other/path" {
				secretWithoutConfig = &items[i]
			}
		}
	}

	assert.NotNil(t, secretWithConfig, "secret backed by config file must exist")
	assert.Equal(t, "/etc/secret.conf", secretWithConfig.SourcePath,
		"SourcePath must point to the backing config file")

	assert.NotNil(t, secretWithoutConfig, "secret without config file must exist")
	assert.Empty(t, secretWithoutConfig.SourcePath,
		"SourcePath must be empty when no backing config file")
}

func TestIsIncluded(t *testing.T) {
	tr := true
	fa := false
	assert.True(t, isIncluded(nil), "nil should be included (default)")
	assert.True(t, isIncluded(&tr), "true should be included")
	assert.False(t, isIncluded(&fa), "false should be excluded")
}

func TestClassifySnapshot_DefaultInclude_NoOriginal(t *testing.T) {
	// Without an original snapshot, DefaultInclude should match current include value.
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: false, SourceRepo: "appstream"},
		},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.False(t, items[0].DefaultInclude, "without original, default should match current (false)")
}

func TestClassifySnapshot_DefaultInclude_OriginalAlsoFalse(t *testing.T) {
	// Both current and original have include=false (fleet below-threshold).
	// DefaultInclude should be false → SPA treats as undecided.
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: false, SourceRepo: "appstream"},
		},
	}
	orig := schema.NewSnapshot()
	orig.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: false, SourceRepo: "appstream"},
		},
	}
	items := ClassifySnapshot(snap, orig)
	assert.Len(t, items, 1)
	assert.False(t, items[0].DefaultInclude, "original had false, default should be false")
}

func TestClassifySnapshot_DefaultInclude_UserReIncluded(t *testing.T) {
	// Current is include=true (user re-included), original was include=false.
	// DefaultInclude should still be false (from original) → SPA sees decided.
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
		},
	}
	orig := schema.NewSnapshot()
	orig.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: false, SourceRepo: "appstream"},
		},
	}
	items := ClassifySnapshot(snap, orig)
	assert.Len(t, items, 1)
	assert.False(t, items[0].DefaultInclude, "original had false, default should still be false even though current is true")
}

func TestClassifySnapshot_DefaultInclude_UserExcluded(t *testing.T) {
	// Current is include=false (user excluded), original was include=true.
	// DefaultInclude should be true (from original) → SPA sees decided.
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: false, SourceRepo: "appstream"},
		},
	}
	orig := schema.NewSnapshot()
	orig.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
		},
	}
	items := ClassifySnapshot(snap, orig)
	assert.Len(t, items, 1)
	assert.True(t, items[0].DefaultInclude, "original had true, default should be true")
}

func TestClassifySnapshot_DefaultInclude_NewItem(t *testing.T) {
	// Item exists in current but not in original (new item).
	// DefaultInclude should keep the current value (true).
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "newpkg", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
		},
	}
	orig := schema.NewSnapshot() // empty original — no packages
	items := ClassifySnapshot(snap, orig)
	assert.Len(t, items, 1)
	assert.True(t, items[0].DefaultInclude, "new item not in original should keep current default (true)")
}

func TestMapInclude(t *testing.T) {
	assert.True(t, mapInclude(map[string]interface{}{"name": "test"}), "missing include key should default true")
	assert.True(t, mapInclude(map[string]interface{}{"include": true}), "include=true should return true")
	assert.False(t, mapInclude(map[string]interface{}{"include": false}), "include=false should return false")
	assert.True(t, mapInclude(map[string]interface{}{"include": "yes"}), "non-bool include should default true")
}

func TestClassifyModuleStream_DefaultInclude_False(t *testing.T) {
	// Test that module stream with Include: false gets DefaultInclude: false
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		ModuleStreams: []schema.EnabledModuleStream{
			{ModuleName: "nodejs", Stream: "18", Include: false},
		},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, "ms-nodejs-18", items[0].Key)
	assert.False(t, items[0].DefaultInclude, "module stream with Include=false should have DefaultInclude=false")
}

func TestClassifyModuleStream_DefaultInclude_True(t *testing.T) {
	// Test that module stream with Include: true gets DefaultInclude: true
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		ModuleStreams: []schema.EnabledModuleStream{
			{ModuleName: "nodejs", Stream: "18", Include: true},
		},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, "ms-nodejs-18", items[0].Key)
	assert.True(t, items[0].DefaultInclude, "module stream with Include=true should have DefaultInclude=true")
}

func TestClassifySelinuxPortLabel_DefaultInclude_False(t *testing.T) {
	// Test that selinux port label with Include: false gets DefaultInclude: false
	snap := schema.NewSnapshot()
	snap.Selinux = &schema.SelinuxSection{
		PortLabels: []schema.SelinuxPortLabel{
			{Protocol: "tcp", Port: "8080", Type: "http_port_t", Include: false},
		},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, "seport-tcp-8080", items[0].Key)
	assert.False(t, items[0].DefaultInclude, "selinux port label with Include=false should have DefaultInclude=false")
}

func TestClassifySelinuxPortLabel_DefaultInclude_True(t *testing.T) {
	// Test that selinux port label with Include: true gets DefaultInclude: true
	snap := schema.NewSnapshot()
	snap.Selinux = &schema.SelinuxSection{
		PortLabels: []schema.SelinuxPortLabel{
			{Protocol: "tcp", Port: "8080", Type: "http_port_t", Include: true},
		},
	}
	items := ClassifySnapshot(snap, nil)
	assert.Len(t, items, 1)
	assert.Equal(t, "seport-tcp-8080", items[0].Key)
	assert.True(t, items[0].DefaultInclude, "selinux port label with Include=true should have DefaultInclude=true")
}

func TestModuleStream_UserExclusionSurvivesNormalization(t *testing.T) {
	// v12 snapshot: user-excluded module stream must NOT be re-included.
	snap := schema.NewSnapshot()
	snap.SchemaVersion = schema.SchemaVersion // current version
	snap.Rpm = &schema.RpmSection{
		ModuleStreams: []schema.EnabledModuleStream{
			{ModuleName: "nodejs", Stream: "18", Profiles: []string{"common"}, Include: false},
		},
	}

	schema.NormalizeSnapshot(snap)

	assert.False(t, snap.Rpm.ModuleStreams[0].Include,
		"user-excluded module stream in v12 must NOT be re-included by NormalizeSnapshot")
}

// v11 migration test lives in schema/snapshot_test.go (TestLoadSnapshot_V11MigratesModuleStreamInclude)
// since the migration now runs at the LoadSnapshot boundary, not in NormalizeSnapshot.
