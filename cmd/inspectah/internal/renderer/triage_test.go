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
	items := ClassifySnapshot(snap)
	assert.Len(t, items, 1)
	assert.Equal(t, 1, items[0].Tier)
	assert.Equal(t, "packages", items[0].Section)
}

func TestClassifyPackage_ThirdParty(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{{Name: "epel-pkg", Arch: "x86_64", SourceRepo: "epel", Include: true}},
	}
	items := ClassifySnapshot(snap)
	assert.Len(t, items, 1)
	assert.Equal(t, 2, items[0].Tier)
	assert.Contains(t, items[0].Reason, "Third-party")
}

func TestClassifyPackage_LocalInstall(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{{Name: "mystery", Arch: "x86_64", State: "local_install", Include: true}},
	}
	items := ClassifySnapshot(snap)
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
	items := ClassifySnapshot(snap)
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
	items := ClassifySnapshot(snap)

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
	items := ClassifySnapshot(snap)
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
	items := ClassifySnapshot(snap)
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
	items := ClassifySnapshot(snap)
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
	items := ClassifySnapshot(snap)
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
	items := ClassifySnapshot(snap)
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
	items := ClassifySnapshot(snap)
	assert.Len(t, items, 1)
	assert.Equal(t, 3, items[0].Tier)
	assert.Contains(t, items[0].Reason, "without quadlet")
}

func TestClassifySnapshot_EmptySnapshot(t *testing.T) {
	snap := schema.NewSnapshot()
	items := ClassifySnapshot(snap)
	assert.Empty(t, items)
}

func TestIsIncluded(t *testing.T) {
	tr := true
	fa := false
	assert.True(t, isIncluded(nil), "nil should be included (default)")
	assert.True(t, isIncluded(&tr), "true should be included")
	assert.False(t, isIncluded(&fa), "false should be excluded")
}
