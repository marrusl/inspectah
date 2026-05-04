package renderer

import (
	"encoding/json"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
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

func TestIsFleetSnapshot(t *testing.T) {
	single := schema.NewSnapshot()
	assert.False(t, isFleetSnapshot(single), "single-machine snapshot should not be fleet")

	fleet := schema.NewSnapshot()
	fleet.Meta["fleet"] = map[string]interface{}{
		"source_hosts":   []interface{}{"host1", "host2"},
		"total_hosts":    float64(2),
		"min_prevalence": float64(50),
	}
	assert.True(t, isFleetSnapshot(fleet), "snapshot with fleet metadata should be fleet")
}

func findItem(items []TriageItem, key string) *TriageItem {
	for i := range items {
		if items[i].Key == key {
			return &items[i]
		}
	}
	return nil
}

func strSlicePtr(s []string) *[]string {
	return &s
}

func TestClassifyPackages_SingleMachine_GroupByRepo(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaselinePackageNames: strSlicePtr([]string{"bash", "coreutils"}),
		PackagesAdded: []schema.PackageEntry{
			{Name: "bash", Arch: "x86_64", Include: true, SourceRepo: "baseos"},
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
			{Name: "htop", Arch: "x86_64", Include: true, SourceRepo: "epel"},
			{Name: "custom", Arch: "x86_64", Include: true, State: "local_install"},
		},
	}

	items := ClassifySnapshot(snap, nil)

	bash := findItem(items, "pkg-bash-x86_64")
	require.NotNil(t, bash)
	assert.Equal(t, 1, bash.Tier)
	assert.Equal(t, "base:baseos", bash.Group)
	assert.True(t, bash.AlwaysIncluded)
	assert.Equal(t, "", bash.CardType)
	assert.False(t, bash.DisplayOnly)

	vim := findItem(items, "pkg-vim-x86_64")
	require.NotNil(t, vim)
	assert.Equal(t, 2, vim.Tier)
	assert.Equal(t, "user:appstream", vim.Group)

	htop := findItem(items, "pkg-htop-x86_64")
	require.NotNil(t, htop)
	assert.Equal(t, 2, htop.Tier)
	assert.Equal(t, "user:epel", htop.Group)

	custom := findItem(items, "pkg-custom-x86_64")
	require.NotNil(t, custom)
	assert.Equal(t, 3, custom.Tier)
	assert.Equal(t, "", custom.Group)
	assert.Equal(t, "notification", custom.CardType)
}

func TestClassifyPackages_Fleet_NoGroups(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Meta["fleet"] = map[string]interface{}{
		"source_hosts":   []interface{}{"host1", "host2"},
		"total_hosts":    float64(2),
		"min_prevalence": float64(50),
	}
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
		},
	}

	items := ClassifySnapshot(snap, nil)

	vim := findItem(items, "pkg-vim-x86_64")
	require.NotNil(t, vim)
	assert.Equal(t, "", vim.Group, "fleet mode should not populate Group")
	assert.Equal(t, "", vim.CardType)
}

func TestClassifyPackages_NoRepo_Acknowledged(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "custom", Arch: "x86_64", Include: true, State: "local_install", Acknowledged: true},
		},
	}

	items := ClassifySnapshot(snap, nil)
	custom := findItem(items, "pkg-custom-x86_64")
	require.NotNil(t, custom)
	assert.True(t, custom.Acknowledged)
	assert.Equal(t, "notification", custom.CardType)
}

func TestClassifyConfigFiles_SingleMachine_Grouping(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/ssh/sshd_config", Kind: schema.ConfigFileKindRpmOwnedDefault, Include: true},
			{Path: "/etc/httpd/conf/httpd.conf", Kind: schema.ConfigFileKindRpmOwnedModified, Include: true},
			{Path: "/etc/systemd/system/foo.service.d/override.conf", Kind: "systemd_dropin", Include: true},
			{Path: "/etc/custom/app.conf", Kind: "", Include: true},
		},
	}

	items := classifyConfigFiles(snap, make(map[string]bool), false)

	ssh := findItem(items, "cfg-/etc/ssh/sshd_config")
	require.NotNil(t, ssh)
	assert.Equal(t, "kind:unchanged", ssh.Group)

	httpd := findItem(items, "cfg-/etc/httpd/conf/httpd.conf")
	require.NotNil(t, httpd)
	assert.Equal(t, "", httpd.Group)

	dropin := findItem(items, "cfg-/etc/systemd/system/foo.service.d/override.conf")
	require.NotNil(t, dropin)
	assert.Equal(t, "kind:drop-in", dropin.Group)

	custom := findItem(items, "cfg-/etc/custom/app.conf")
	require.NotNil(t, custom)
	assert.Equal(t, "", custom.Group)
}

func TestClassifyRuntime_SingleMachine_Grouping(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Services = &schema.ServiceSection{
		StateChanges: []schema.ServiceStateChange{
			{Unit: "sshd.service", CurrentState: "enabled", DefaultState: "enabled", Include: true},
			{Unit: "httpd.service", CurrentState: "enabled", DefaultState: "disabled", Include: true},
			{Unit: "dnf-makecache.timer", CurrentState: "enabled", DefaultState: "enabled", Include: true},
		},
	}
	snap.ScheduledTasks = &schema.ScheduledTaskSection{
		CronJobs: []schema.CronJob{
			{Path: "/etc/cron.d/backup", Source: "custom", Include: true},
		},
	}

	items := classifyRuntime(snap, make(map[string]bool), false)

	sshd := findItem(items, "svc-sshd.service")
	require.NotNil(t, sshd)
	assert.Equal(t, "sub:services-default", sshd.Group)
	assert.Equal(t, 1, sshd.Tier)

	httpd := findItem(items, "svc-httpd.service")
	require.NotNil(t, httpd)
	assert.Equal(t, "sub:services-changed", httpd.Group)
	assert.Equal(t, 2, httpd.Tier)

	dnf := findItem(items, "svc-dnf-makecache.timer")
	require.NotNil(t, dnf)
	assert.Equal(t, 3, dnf.Tier)
	assert.Equal(t, "", dnf.Group)
	assert.Contains(t, dnf.Reason, "package management at runtime")

	cron := findItem(items, "cron-/etc/cron.d/backup")
	require.NotNil(t, cron)
	assert.Equal(t, "sub:cron", cron.Group)
}

func TestClassifyRuntime_ImageModeIncompatible(t *testing.T) {
	tests := []struct {
		unit string
	}{
		{"dnf-makecache.service"},
		{"dnf-makecache.timer"},
		{"packagekit.service"},
	}
	for _, tt := range tests {
		t.Run(tt.unit, func(t *testing.T) {
			snap := schema.NewSnapshot()
			snap.Services = &schema.ServiceSection{
				StateChanges: []schema.ServiceStateChange{
					{Unit: tt.unit, CurrentState: "enabled", DefaultState: "enabled", Include: true},
				},
			}
			items := classifyRuntime(snap, make(map[string]bool), false)
			svc := findItem(items, "svc-"+tt.unit)
			require.NotNil(t, svc)
			assert.Equal(t, 3, svc.Tier)
			assert.Equal(t, "", svc.Group)
		})
	}
}

func TestClassifyContainerItems_SingleMachine_Grouping(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		QuadletUnits: []schema.QuadletUnit{
			{Name: "webapp.container", Image: "webapp:latest", Include: true},
		},
		RunningContainers: []schema.RunningContainer{
			{Name: "webapp", Image: "webapp:latest"},
			{Name: "orphan", Image: "orphan:latest"},
		},
	}
	snap.NonRpmSoftware = &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{Path: "/opt/agent/bin/agent", Method: "binary", Include: true},
		},
	}

	items := classifyContainerItems(snap, make(map[string]bool), false)

	quadlet := findItem(items, "quadlet-webapp.container")
	require.NotNil(t, quadlet)
	assert.Equal(t, "sub:quadlet", quadlet.Group)
	assert.False(t, quadlet.DisplayOnly)

	webapp := findItem(items, "container-webapp")
	require.NotNil(t, webapp)
	assert.True(t, webapp.DisplayOnly)

	orphan := findItem(items, "container-orphan")
	require.NotNil(t, orphan)
	assert.True(t, orphan.DisplayOnly)
	assert.Equal(t, 3, orphan.Tier)

	agent := findItem(items, "nonrpm-/opt/agent/bin/agent")
	require.NotNil(t, agent)
	assert.Equal(t, "notification", agent.CardType)
	assert.Contains(t, agent.Reason, "provenance")
}

func TestClassifyIdentity_SingleMachine_NoGroups(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.UsersGroups = &schema.UserGroupSection{
		Users: []map[string]interface{}{
			{"name": "admin", "uid": float64(1001), "include": true},
		},
		Groups: []map[string]interface{}{
			{"name": "developers", "gid": float64(1001), "include": true},
		},
	}

	items := classifyIdentity(snap, make(map[string]bool), false)
	for _, item := range items {
		assert.Equal(t, "", item.Group, "identity items should never be grouped")
	}

	admin := findItem(items, "user-admin")
	require.NotNil(t, admin)
	assert.False(t, admin.DisplayOnly)

	devs := findItem(items, "group-developers")
	require.NotNil(t, devs)
	assert.True(t, devs.DisplayOnly)
}

func TestClassifySystemItems_SingleMachine_Grouping(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.KernelBoot = &schema.KernelBootSection{
		SysctlOverrides: []schema.SysctlOverride{
			{Key: "vm.swappiness", Runtime: "10", Include: true},
		},
	}
	snap.Network = &schema.NetworkSection{
		Connections: []schema.NMConnection{
			{Name: "eth0", Type: "ethernet"},
		},
	}
	snap.Storage = &schema.StorageSection{
		FstabEntries: []schema.FstabEntry{
			{MountPoint: "/data", Fstype: "xfs"},
			{MountPoint: "/var", Fstype: "xfs"},
			{MountPoint: "/usr/local", Fstype: "xfs"},
		},
	}

	items := classifySystemItems(snap, make(map[string]bool), false)

	sysctl := findItem(items, "sysctl-vm.swappiness")
	require.NotNil(t, sysctl)
	assert.Equal(t, "sub:sysctl", sysctl.Group)
	assert.False(t, sysctl.DisplayOnly)

	eth0 := findItem(items, "conn-eth0")
	require.NotNil(t, eth0)
	assert.Equal(t, "sub:network", eth0.Group)
	assert.True(t, eth0.DisplayOnly)

	data := findItem(items, "fstab-/data")
	require.NotNil(t, data)
	assert.Equal(t, "sub:fstab", data.Group)
	assert.True(t, data.DisplayOnly)

	varMount := findItem(items, "fstab-/var")
	require.NotNil(t, varMount)
	assert.Equal(t, "", varMount.Group)
	assert.True(t, varMount.DisplayOnly)

	usrLocal := findItem(items, "fstab-/usr/local")
	require.NotNil(t, usrLocal)
	assert.Equal(t, "", usrLocal.Group)
	assert.True(t, usrLocal.DisplayOnly)
}

func TestClassifySnapshot_FleetVsSingleMachine(t *testing.T) {
	makeSnap := func() *schema.InspectionSnapshot {
		snap := schema.NewSnapshot()
		snap.Rpm = &schema.RpmSection{
			PackagesAdded: []schema.PackageEntry{
				{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
			},
		}
		snap.Network = &schema.NetworkSection{
			Connections: []schema.NMConnection{
				{Name: "eth0", Type: "ethernet"},
			},
		}
		return snap
	}

	t.Run("single-machine populates groups", func(t *testing.T) {
		snap := makeSnap()
		items := ClassifySnapshot(snap, nil)
		vim := findItem(items, "pkg-vim-x86_64")
		require.NotNil(t, vim)
		assert.Equal(t, "user:appstream", vim.Group)
		eth0 := findItem(items, "conn-eth0")
		require.NotNil(t, eth0)
		assert.True(t, eth0.DisplayOnly)
	})

	t.Run("fleet does not populate groups", func(t *testing.T) {
		snap := makeSnap()
		snap.Meta["fleet"] = map[string]interface{}{
			"source_hosts": []interface{}{"h1", "h2"},
			"total_hosts":  float64(2),
		}
		items := ClassifySnapshot(snap, nil)
		vim := findItem(items, "pkg-vim-x86_64")
		require.NotNil(t, vim)
		assert.Equal(t, "", vim.Group)
		eth0 := findItem(items, "conn-eth0")
		require.NotNil(t, eth0)
		assert.False(t, eth0.DisplayOnly)
	})
}

func TestExtractDeps(t *testing.T) {
	tests := []struct {
		name     string
		depTree  map[string]interface{}
		leafName string
		want     []string
	}{
		{"nil tree", nil, "vim", nil},
		{"missing key", map[string]interface{}{"bash": []interface{}{"readline"}}, "vim", nil},
		{"nil value", map[string]interface{}{"vim": nil}, "vim", nil},
		{"empty array interface", map[string]interface{}{"vim": []interface{}{}}, "vim", nil},
		{"valid interface slice", map[string]interface{}{
			"vim": []interface{}{"vim-common", "gpm-libs", "vim-filesystem"},
		}, "vim", []string{"vim-common", "gpm-libs", "vim-filesystem"}},
		{"string slice (Go-native)", map[string]interface{}{
			"vim": []string{"vim-common", "gpm-libs"},
		}, "vim", []string{"vim-common", "gpm-libs"}},
		{"empty string slice", map[string]interface{}{"vim": []string{}}, "vim", nil},
		{"mixed types in interface slice", map[string]interface{}{
			"vim": []interface{}{"vim-common", 42, "gpm-libs"},
		}, "vim", []string{"vim-common", "gpm-libs"}},
		{"wrong type value", map[string]interface{}{"vim": "not-a-slice"}, "vim", nil},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := extractDeps(tt.depTree, tt.leafName)
			assert.Equal(t, tt.want, got)
		})
	}
}

func TestClassifyPackages_LeafOnly_SingleMachine(t *testing.T) {
	leafNames := []string{"vim", "htop"}
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		LeafPackages: &leafNames,
		AutoPackages: &[]string{"vim-common", "gpm-libs"},
		LeafDepTree: map[string]interface{}{
			"vim":  []interface{}{"vim-common", "gpm-libs"},
			"htop": []interface{}{},
		},
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "vim-common", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "gpm-libs", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "1.20", Release: "1.el9"},
			{Name: "htop", Arch: "x86_64", Include: true, SourceRepo: "epel", Version: "3.3", Release: "1.el9"},
		},
	}

	items := classifyPackages(snap, make(map[string]bool), false)

	assert.Equal(t, 2, len(items), "should only have 2 leaf packages, not 4")

	vim := findItem(items, "pkg-vim-x86_64")
	require.NotNil(t, vim)
	assert.Equal(t, "user:appstream", vim.Group)
	assert.Equal(t, []string{"vim-common", "gpm-libs"}, vim.Deps)

	htop := findItem(items, "pkg-htop-x86_64")
	require.NotNil(t, htop)
	assert.Nil(t, htop.Deps, "htop has empty deps, should be nil")

	assert.Nil(t, findItem(items, "pkg-vim-common-x86_64"))
	assert.Nil(t, findItem(items, "pkg-gpm-libs-x86_64"))
}

func TestClassifyPackages_LeafOnly_FleetStillShowsAll(t *testing.T) {
	leafNames := []string{"vim"}
	snap := schema.NewSnapshot()
	snap.Meta["fleet"] = map[string]interface{}{"source_hosts": []interface{}{"h1"}}
	snap.Rpm = &schema.RpmSection{
		LeafPackages: &leafNames,
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
			{Name: "vim-common", Arch: "x86_64", Include: false, SourceRepo: "appstream"},
		},
	}

	items := classifyPackages(snap, make(map[string]bool), true)
	assert.Equal(t, 2, len(items), "fleet mode should show ALL packages including deps")
}

func TestClassifyPackages_NoLeafPackages_ShowsAll(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
			{Name: "vim-common", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
		},
	}

	items := classifyPackages(snap, make(map[string]bool), false)
	assert.Equal(t, 2, len(items), "without LeafPackages, all packages should appear")
}

func TestNormalizeLeafDefaults(t *testing.T) {
	t.Run("sets leaf includes to true", func(t *testing.T) {
		leafNames := []string{"vim", "htop"}
		snap := schema.NewSnapshot()
		snap.Rpm = &schema.RpmSection{
			LeafPackages: &leafNames,
			PackagesAdded: []schema.PackageEntry{
				{Name: "vim", Arch: "x86_64", Include: false},
				{Name: "vim-common", Arch: "x86_64", Include: false},
				{Name: "htop", Arch: "x86_64", Include: false},
			},
		}

		NormalizeLeafDefaults(snap)

		assert.True(t, snap.Rpm.PackagesAdded[0].Include, "leaf vim should be true")
		assert.False(t, snap.Rpm.PackagesAdded[1].Include, "dep vim-common should stay false")
		assert.True(t, snap.Rpm.PackagesAdded[2].Include, "leaf htop should be true")
	})

	t.Run("skips fleet snapshots", func(t *testing.T) {
		leafNames := []string{"vim"}
		snap := schema.NewSnapshot()
		snap.Meta["fleet"] = map[string]interface{}{"source_hosts": []interface{}{"h1"}}
		snap.Rpm = &schema.RpmSection{
			LeafPackages: &leafNames,
			PackagesAdded: []schema.PackageEntry{
				{Name: "vim", Arch: "x86_64", Include: false},
			},
		}

		NormalizeLeafDefaults(snap)
		assert.False(t, snap.Rpm.PackagesAdded[0].Include, "fleet snapshot should not be normalized")
	})

	t.Run("no-op when LeafPackages nil", func(t *testing.T) {
		snap := schema.NewSnapshot()
		snap.Rpm = &schema.RpmSection{
			PackagesAdded: []schema.PackageEntry{
				{Name: "vim", Arch: "x86_64", Include: false},
			},
		}

		NormalizeLeafDefaults(snap)
		assert.False(t, snap.Rpm.PackagesAdded[0].Include)
	})
}

func TestClassifyVersionChanges_SingleMachine(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		VersionChanges: []schema.VersionChange{
			{Name: "bash", Arch: "x86_64", HostVersion: "5.2.26", BaseVersion: "5.2.32", Direction: schema.VersionChangeUpgrade},
			{Name: "openssl", Arch: "x86_64", HostVersion: "3.2.2", BaseVersion: "3.2.1", Direction: schema.VersionChangeDowngrade},
		},
	}

	items := classifyVersionChanges(snap, false)
	assert.Equal(t, 2, len(items))

	bash := findItem(items, "verchg-bash-x86_64")
	require.NotNil(t, bash)
	assert.Equal(t, "version-changes", bash.Section)
	assert.Equal(t, 1, bash.Tier)
	assert.True(t, bash.DisplayOnly)
	assert.Equal(t, "sub:version-upgrades", bash.Group)
	assert.Contains(t, bash.Meta, "→")

	openssl := findItem(items, "verchg-openssl-x86_64")
	require.NotNil(t, openssl)
	assert.Equal(t, "sub:version-downgrades", openssl.Group)
}

func TestClassifyVersionChanges_Fleet_ReturnsNil(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		VersionChanges: []schema.VersionChange{
			{Name: "bash", Arch: "x86_64", Direction: schema.VersionChangeUpgrade},
		},
	}
	items := classifyVersionChanges(snap, true)
	assert.Nil(t, items)
}

func TestClassifySELinux_InSystemSection(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Selinux = &schema.SelinuxSection{
		BooleanOverrides: []map[string]interface{}{
			{"name": "httpd_can_network_connect", "current_value": "on", "include": true},
		},
		CustomModules: []string{"myapp"},
		PortLabels: []schema.SelinuxPortLabel{
			{Protocol: "tcp", Port: "8443", Type: "http_port_t", Include: true},
		},
	}

	items := ClassifySnapshot(snap, nil)

	sebool := findItem(items, "sebool-httpd_can_network_connect")
	require.NotNil(t, sebool)
	assert.Equal(t, "system", sebool.Section)
	assert.Equal(t, "sub:selinux", sebool.Group)
	assert.Equal(t, 2, sebool.Tier)
	assert.Equal(t, "on", sebool.Meta)

	semod := findItem(items, "semod-myapp")
	require.NotNil(t, semod)
	assert.Equal(t, "system", semod.Section)
	assert.Equal(t, "", semod.Group, "semod-* must be ungrouped to route to buildNotificationCard")
	assert.Equal(t, 3, semod.Tier)
	assert.Equal(t, "notification", semod.CardType)

	seport := findItem(items, "seport-tcp-8443")
	require.NotNil(t, seport)
	assert.Equal(t, "system", seport.Section)
	assert.Equal(t, "sub:selinux", seport.Group)

	for _, item := range items {
		if item.Section == "identity" {
			assert.NotContains(t, item.Key, "sebool-")
			assert.NotContains(t, item.Key, "semod-")
			assert.NotContains(t, item.Key, "seport-")
		}
	}
}

// ── NormalizeIncludeDefaults tests ──

func TestNormalizeIncludeDefaults_SingleMachine_AllSurfaces(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Kind: schema.ConfigFileKindRpmOwnedModified, Include: false},
		},
	}
	snap.Services = &schema.ServiceSection{
		StateChanges: []schema.ServiceStateChange{
			{Unit: "httpd.service", CurrentState: "enabled", DefaultState: "disabled", Include: false},
		},
		EnabledUnits: []string{"httpd.service"},
	}
	snap.ScheduledTasks = &schema.ScheduledTaskSection{
		CronJobs: []schema.CronJob{
			{Path: "/etc/cron.d/backup", Source: "custom", Include: false},
		},
		SystemdTimers: []schema.SystemdTimer{
			{Name: "fstrim.timer", Include: boolPtr(false)},
		},
	}
	snap.Containers = &schema.ContainerSection{
		QuadletUnits: []schema.QuadletUnit{
			{Name: "webapp.container", Image: "webapp:latest", Include: false},
		},
	}
	snap.Network = &schema.NetworkSection{
		FirewallZones: []schema.FirewallZone{
			{Name: "public", Path: "/etc/firewalld/zones/public.xml", Include: false},
		},
	}
	snap.KernelBoot = &schema.KernelBootSection{
		SysctlOverrides: []schema.SysctlOverride{
			{Key: "vm.swappiness", Runtime: "10", Include: false},
		},
	}

	NormalizeIncludeDefaults(snap, false)

	assert.True(t, snap.Config.Files[0].Include, "config file must be included")
	assert.True(t, snap.Services.StateChanges[0].Include, "service must be included")
	assert.True(t, snap.ScheduledTasks.CronJobs[0].Include, "cron job must be included")
	assert.True(t, *snap.ScheduledTasks.SystemdTimers[0].Include, "systemd timer must be included")
	assert.True(t, snap.Containers.QuadletUnits[0].Include, "quadlet unit must be included")
	assert.True(t, snap.Network.FirewallZones[0].Include, "firewall zone must be included")
	assert.True(t, snap.KernelBoot.SysctlOverrides[0].Include, "sysctl override must be included")
}

func TestNormalizeIncludeDefaults_FleetMode_Untouched(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Meta["fleet"] = map[string]interface{}{
		"source_hosts":   []interface{}{"host1", "host2"},
		"total_hosts":    float64(2),
		"min_prevalence": float64(50),
	}
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Include: false},
		},
	}
	snap.Services = &schema.ServiceSection{
		StateChanges: []schema.ServiceStateChange{
			{Unit: "httpd.service", Include: false},
		},
	}

	NormalizeIncludeDefaults(snap, true)

	assert.False(t, snap.Config.Files[0].Include, "fleet mode must not change config include")
	assert.False(t, snap.Services.StateChanges[0].Include, "fleet mode must not change service include")
}

func TestNormalizeIncludeDefaults_Idempotent(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Include: true},
		},
	}
	snap.Services = &schema.ServiceSection{
		StateChanges: []schema.ServiceStateChange{
			{Unit: "httpd.service", Include: true},
		},
	}

	NormalizeIncludeDefaults(snap, false)

	assert.True(t, snap.Config.Files[0].Include, "already-true must stay true")
	assert.True(t, snap.Services.StateChanges[0].Include, "already-true must stay true")
}

func TestNormalizeIncludeDefaults_IncompatibleServices(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Services = &schema.ServiceSection{
		StateChanges: []schema.ServiceStateChange{
			{Unit: "httpd.service", CurrentState: "enabled", DefaultState: "disabled", Include: false},
			{Unit: "dnf-makecache.service", CurrentState: "enabled", DefaultState: "enabled", Include: true},
			{Unit: "dnf-makecache.timer", CurrentState: "enabled", DefaultState: "enabled", Include: true},
			{Unit: "packagekit.service", CurrentState: "enabled", DefaultState: "enabled", Include: true},
		},
		EnabledUnits: []string{"httpd.service", "dnf-makecache.service", "dnf-makecache.timer", "packagekit.service"},
	}

	NormalizeIncludeDefaults(snap, false)

	// Normal service gets included
	assert.True(t, snap.Services.StateChanges[0].Include, "httpd must be included")

	// Incompatible services get excluded
	assert.False(t, snap.Services.StateChanges[1].Include, "dnf-makecache.service must be excluded")
	assert.False(t, snap.Services.StateChanges[2].Include, "dnf-makecache.timer must be excluded")
	assert.False(t, snap.Services.StateChanges[3].Include, "packagekit.service must be excluded")

	// Incompatible services removed from EnabledUnits
	assert.Equal(t, []string{"httpd.service"}, snap.Services.EnabledUnits,
		"incompatible services must be removed from EnabledUnits")
}

func TestNormalizeIncludeDefaults_NilSections(t *testing.T) {
	snap := schema.NewSnapshot()
	// All section pointers are nil — must not panic
	NormalizeIncludeDefaults(snap, false)
}

func TestNormalizeIncludeDefaults_SystemdTimerNilInclude(t *testing.T) {
	// SystemdTimer.Include is *bool — nil means "not set"
	snap := schema.NewSnapshot()
	snap.ScheduledTasks = &schema.ScheduledTaskSection{
		SystemdTimers: []schema.SystemdTimer{
			{Name: "fstrim.timer"}, // Include is nil
		},
	}

	NormalizeIncludeDefaults(snap, false)

	require.NotNil(t, snap.ScheduledTasks.SystemdTimers[0].Include, "nil *bool must be set")
	assert.True(t, *snap.ScheduledTasks.SystemdTimers[0].Include, "nil *bool must be set to true")
}

func TestClassifyPackages_FedoraRepoAlwaysIncluded(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaselinePackageNames: strSlicePtr([]string{"bash"}),
		PackagesAdded: []schema.PackageEntry{
			{Name: "bash", Arch: "x86_64", Include: true, SourceRepo: "fedora"},
		},
	}

	items := ClassifySnapshot(snap, nil)

	bash := findItem(items, "pkg-bash-x86_64")
	require.NotNil(t, bash)
	assert.Equal(t, 1, bash.Tier)
	assert.True(t, bash.AlwaysIncluded, "fedora repo packages must have AlwaysIncluded=true")
	assert.Equal(t, "base:fedora", bash.Group)
}

func TestClassifyPackages_TierLabels(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaselinePackageNames: strSlicePtr([]string{"coreutils"}),
		PackagesAdded: []schema.PackageEntry{
			// Tier 1: baseline package from baseos
			{Name: "coreutils", Arch: "x86_64", Include: true, SourceRepo: "baseos"},
			// Tier 2: non-baseline from appstream (standard repo, not third-party)
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
			// Tier 2: third-party repo
			{Name: "htop", Arch: "x86_64", Include: true, SourceRepo: "epel"},
		},
	}

	items := ClassifySnapshot(snap, nil)

	// Tier 1 items use "base:" prefix
	coreutils := findItem(items, "pkg-coreutils-x86_64")
	require.NotNil(t, coreutils)
	assert.Equal(t, 1, coreutils.Tier)
	assert.Equal(t, "base:baseos", coreutils.Group, "tier 1 packages use 'base:' group prefix")
	assert.True(t, coreutils.AlwaysIncluded)

	// Tier 2 standard repo uses "user:" prefix
	vim := findItem(items, "pkg-vim-x86_64")
	require.NotNil(t, vim)
	assert.Equal(t, 2, vim.Tier)
	assert.Equal(t, "user:appstream", vim.Group, "tier 2 standard repo packages use 'user:' group prefix")
	assert.False(t, vim.AlwaysIncluded)

	// Tier 2 third-party repo also uses "user:" prefix
	htop := findItem(items, "pkg-htop-x86_64")
	require.NotNil(t, htop)
	assert.Equal(t, 2, htop.Tier)
	assert.Equal(t, "user:epel", htop.Group, "tier 2 third-party repo packages use 'user:' group prefix")
	assert.False(t, htop.AlwaysIncluded)
}

func TestClassifyVersionChanges_UsesVersionChangesSection(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		VersionChanges: []schema.VersionChange{
			{Name: "bash", Arch: "x86_64", HostVersion: "5.2.26", BaseVersion: "5.2.32", Direction: schema.VersionChangeUpgrade},
		},
	}

	items := classifyVersionChanges(snap, false)
	require.Len(t, items, 1)
	assert.Equal(t, "version-changes", items[0].Section, "version changes must use 'version-changes' section, not 'packages'")
}

func TestClassifyKernelModules_OnlyModulesLoadD(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.KernelBoot = &schema.KernelBootSection{
		NonDefaultModules: []schema.KernelModule{
			{Name: "br_netfilter", Size: "32768", UsedBy: "0", Include: true},
			{Name: "nf_conntrack", Size: "180224", UsedBy: "2 br_netfilter", Include: true},
			{Name: "overlay", Size: "155648", UsedBy: "0", Include: true},
		},
		ModulesLoadD: []schema.ConfigSnippet{
			{Path: "/etc/modules-load.d/k8s.conf", Content: "br_netfilter\noverlay"},
		},
	}

	items := classifySystemItems(snap, make(map[string]bool), false)

	// br_netfilter should appear (in modules-load.d)
	brItem := findItem(items, "kmod-br_netfilter")
	require.NotNil(t, brItem, "br_netfilter must appear (listed in modules-load.d)")
	assert.True(t, brItem.DisplayOnly, "kmod items must be display-only")
	assert.Equal(t, "sub:kmod", brItem.Group)

	// overlay should appear (in modules-load.d)
	ovItem := findItem(items, "kmod-overlay")
	require.NotNil(t, ovItem, "overlay must appear (listed in modules-load.d)")
	assert.True(t, ovItem.DisplayOnly, "kmod items must be display-only")

	// nf_conntrack should NOT appear (auto-loaded, not in modules-load.d)
	assert.Nil(t, findItem(items, "kmod-nf_conntrack"),
		"nf_conntrack must not appear (auto-loaded, not in modules-load.d)")
}

func TestClassifyKernelModules_CommentsAndBlanks(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.KernelBoot = &schema.KernelBootSection{
		NonDefaultModules: []schema.KernelModule{
			{Name: "br_netfilter", Size: "32768", UsedBy: "0", Include: true},
			{Name: "ip_tables", Size: "28672", UsedBy: "0", Include: true},
		},
		ModulesLoadD: []schema.ConfigSnippet{
			{Path: "/etc/modules-load.d/k8s.conf", Content: "# Kubernetes networking\nbr_netfilter\n\n# blank lines above"},
		},
	}

	items := classifySystemItems(snap, make(map[string]bool), false)

	assert.NotNil(t, findItem(items, "kmod-br_netfilter"), "br_netfilter must appear")
	assert.Nil(t, findItem(items, "kmod-ip_tables"), "ip_tables must not appear (not in modules-load.d)")
}

func TestClassifyKernelModules_NoModulesLoadD(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.KernelBoot = &schema.KernelBootSection{
		NonDefaultModules: []schema.KernelModule{
			{Name: "br_netfilter", Size: "32768", UsedBy: "0", Include: true},
		},
		// No ModulesLoadD entries — all modules are auto-loaded
	}

	items := classifySystemItems(snap, make(map[string]bool), false)

	// No kmod items should appear when there are no modules-load.d files
	assert.Nil(t, findItem(items, "kmod-br_netfilter"),
		"no kmod items when modules-load.d is empty")
}

func TestClassifyKernelModules_Fleet_NotDisplayOnly(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Meta = map[string]interface{}{"fleet": true}
	snap.KernelBoot = &schema.KernelBootSection{
		NonDefaultModules: []schema.KernelModule{
			{Name: "br_netfilter", Size: "32768", UsedBy: "0", Include: true},
		},
		ModulesLoadD: []schema.ConfigSnippet{
			{Path: "/etc/modules-load.d/k8s.conf", Content: "br_netfilter"},
		},
	}

	items := classifySystemItems(snap, make(map[string]bool), true)

	brItem := findItem(items, "kmod-br_netfilter")
	require.NotNil(t, brItem)
	assert.False(t, brItem.DisplayOnly, "fleet kmod items must not be display-only")
	assert.Empty(t, brItem.Group, "fleet items must not be grouped")
}

func TestNormalizeIncludeDefaults_RespectsExcludedSecrets(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/normal.conf", Include: false},
			{Path: "/etc/secret.conf", Include: false},
		},
	}
	snap.Redactions = []json.RawMessage{
		json.RawMessage(`{"path":"/etc/secret.conf","source":"file","kind":"excluded","pattern":"api_key"}`),
	}

	NormalizeIncludeDefaults(snap, false)

	// normal.conf should be included (no excluded redaction)
	assert.True(t, snap.Config.Files[0].Include,
		"normal config file must be included after normalization")
	// secret.conf should stay excluded (scanner decision preserved)
	assert.False(t, snap.Config.Files[1].Include,
		"scanner-excluded secret file must retain Include=false after normalization")
}

func TestClassifySecretItems_MultiRedactionSamePath(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/app.conf", Kind: "non_rpm", Include: true},
		},
	}
	// Two redaction findings for the same path — must produce distinct items.
	snap.Redactions = []json.RawMessage{
		json.RawMessage(`{"path":"/etc/app.conf","finding_type":"api_key"}`),
		json.RawMessage(`{"path":"/etc/app.conf","finding_type":"password"}`),
	}

	items := classifySecretItems(snap, make(map[string]bool))

	// Must produce two distinct items, not collapsed by path.
	require.Equal(t, 2, len(items))

	// secret-0 maps to redactions[0] (api_key)
	assert.Equal(t, "secret-0", items[0].Key)
	assert.Contains(t, items[0].Reason, "api_key")

	// secret-1 maps to redactions[1] (password)
	assert.Equal(t, "secret-1", items[1].Key)
	assert.Contains(t, items[1].Reason, "password")

	// Keys are distinct even though the path is the same.
	assert.NotEqual(t, items[0].Key, items[1].Key)

	// Both should have SourcePath set since the config file exists.
	assert.Equal(t, "/etc/app.conf", items[0].SourcePath)
	assert.Equal(t, "/etc/app.conf", items[1].SourcePath)
}
