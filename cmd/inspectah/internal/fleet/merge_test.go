package fleet

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// makeRichSnapshot creates a snapshot with packages, configs, and services
// for merge testing.
func makeRichSnapshot(hostname string, packages []string, configPaths []string, serviceUnits []string) *schema.InspectionSnapshot {
	snap := makeSnapshot(hostname)

	// Packages
	if len(packages) > 0 {
		var added []schema.PackageEntry
		for _, name := range packages {
			added = append(added, schema.PackageEntry{
				Name:    name,
				Epoch:   "0",
				Version: "1.0",
				Release: "1.el9",
				Arch:    "x86_64",
				State:   schema.PackageStateAdded,
				Include: true,
			})
		}
		bi := "quay.io/centos-bootc/centos-bootc:stream9"
		snap.Rpm = &schema.RpmSection{
			PackagesAdded: added,
			BaseImage:     &bi,
		}
	}

	// Configs
	if len(configPaths) > 0 {
		var files []schema.ConfigFileEntry
		for _, p := range configPaths {
			files = append(files, schema.ConfigFileEntry{
				Path:     p,
				Kind:     schema.ConfigFileKindUnowned,
				Category: schema.ConfigCategoryOther,
				Content:  "# config for " + p,
				Include:  true,
			})
		}
		snap.Config = &schema.ConfigSection{Files: files}
	}

	// Services
	if len(serviceUnits) > 0 {
		var changes []schema.ServiceStateChange
		for _, u := range serviceUnits {
			changes = append(changes, schema.ServiceStateChange{
				Unit:         u,
				CurrentState: "enabled",
				DefaultState: "disabled",
				Action:       "enable",
				Include:      true,
			})
		}
		snap.Services = &schema.ServiceSection{
			StateChanges: changes,
		}
	}

	return snap
}

func TestMergeSnapshotsMinimumError(t *testing.T) {
	snap := makeSnapshot("host1")
	_, err := MergeSnapshots([]*schema.InspectionSnapshot{snap}, 100)
	if err == nil {
		t.Fatal("expected error for < 2 snapshots")
	}
}

func TestMergeSnapshotsPackagePrevalence(t *testing.T) {
	snap1 := makeRichSnapshot("host1", []string{"httpd", "vim", "curl"}, nil, nil)
	snap2 := makeRichSnapshot("host2", []string{"httpd", "vim", "nginx"}, nil, nil)
	snap3 := makeRichSnapshot("host3", []string{"httpd", "nginx"}, nil, nil)

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2, snap3}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	if merged.Rpm == nil {
		t.Fatal("expected rpm section in merged snapshot")
	}

	// httpd on all 3 hosts
	httpd := findPackageByName(merged.Rpm.PackagesAdded, "httpd")
	if httpd == nil {
		t.Fatal("expected httpd in merged packages")
	}
	if httpd.Fleet == nil || httpd.Fleet.Count != 3 || httpd.Fleet.Total != 3 {
		t.Errorf("httpd prevalence: got count=%d total=%d, want count=3 total=3",
			safeCount(httpd.Fleet), safeTotal(httpd.Fleet))
	}

	// vim on 2 of 3 hosts
	vim := findPackageByName(merged.Rpm.PackagesAdded, "vim")
	if vim == nil {
		t.Fatal("expected vim in merged packages")
	}
	if vim.Fleet == nil || vim.Fleet.Count != 2 {
		t.Errorf("vim prevalence: got count=%d, want 2", safeCount(vim.Fleet))
	}

	// curl only on 1 of 3 hosts
	curl := findPackageByName(merged.Rpm.PackagesAdded, "curl")
	if curl == nil {
		t.Fatal("expected curl in merged packages")
	}
	if curl.Fleet == nil || curl.Fleet.Count != 1 {
		t.Errorf("curl prevalence: got count=%d, want 1", safeCount(curl.Fleet))
	}
}

func TestMergeSnapshotsPrevalenceFiltering(t *testing.T) {
	snap1 := makeRichSnapshot("host1", []string{"httpd", "vim", "curl"}, nil, nil)
	snap2 := makeRichSnapshot("host2", []string{"httpd", "vim", "nginx"}, nil, nil)
	snap3 := makeRichSnapshot("host3", []string{"httpd", "nginx"}, nil, nil)

	// min_prevalence=100 means item must be on ALL hosts
	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2, snap3}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	// httpd is on all 3 → include=true
	httpd := findPackageByName(merged.Rpm.PackagesAdded, "httpd")
	if httpd == nil || !httpd.Include {
		t.Error("httpd should be included at 100% prevalence")
	}

	// vim is on 2/3 → include=false at 100% threshold
	vim := findPackageByName(merged.Rpm.PackagesAdded, "vim")
	if vim == nil || vim.Include {
		t.Error("vim should NOT be included at 100% prevalence (2/3 hosts)")
	}

	// curl is on 1/3 → include=false
	curl := findPackageByName(merged.Rpm.PackagesAdded, "curl")
	if curl == nil || curl.Include {
		t.Error("curl should NOT be included at 100% prevalence (1/3 hosts)")
	}
}

func TestMergeSnapshotsPrevalenceThreshold50(t *testing.T) {
	snap1 := makeRichSnapshot("host1", []string{"httpd", "vim", "curl"}, nil, nil)
	snap2 := makeRichSnapshot("host2", []string{"httpd", "vim", "nginx"}, nil, nil)
	snap3 := makeRichSnapshot("host3", []string{"httpd", "nginx"}, nil, nil)

	// 50% threshold: 2/3 = 66.7% passes, 1/3 = 33.3% fails
	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2, snap3}, 50)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	httpd := findPackageByName(merged.Rpm.PackagesAdded, "httpd")
	if httpd == nil || !httpd.Include {
		t.Error("httpd should be included at 50% (3/3)")
	}

	vim := findPackageByName(merged.Rpm.PackagesAdded, "vim")
	if vim == nil || !vim.Include {
		t.Error("vim should be included at 50% (2/3 = 66%)")
	}

	curl := findPackageByName(merged.Rpm.PackagesAdded, "curl")
	if curl == nil || curl.Include {
		t.Error("curl should NOT be included at 50% (1/3 = 33%)")
	}
}

func TestMergeSnapshotsConfigVariants(t *testing.T) {
	snap1 := makeSnapshot("host1")
	snap1.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/app.conf", Kind: schema.ConfigFileKindUnowned, Content: "setting=A", Include: true},
		},
	}
	snap2 := makeSnapshot("host2")
	snap2.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/app.conf", Kind: schema.ConfigFileKindUnowned, Content: "setting=B", Include: true},
		},
	}

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	if merged.Config == nil {
		t.Fatal("expected config section")
	}
	// Two different content variants for the same path → 2 entries
	if len(merged.Config.Files) != 2 {
		t.Fatalf("expected 2 config file entries (variants), got %d", len(merged.Config.Files))
	}

	// Both should have path /etc/app.conf
	for _, f := range merged.Config.Files {
		if f.Path != "/etc/app.conf" {
			t.Errorf("unexpected path: %s", f.Path)
		}
	}
}

func TestMergeSnapshotsConfigAutoSelectVariants(t *testing.T) {
	// 3 hosts: 2 have variant A, 1 has variant B → A wins
	snap1 := makeSnapshot("host1")
	snap1.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/app.conf", Kind: schema.ConfigFileKindUnowned, Content: "winner-content", Include: true},
		},
	}
	snap2 := makeSnapshot("host2")
	snap2.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/app.conf", Kind: schema.ConfigFileKindUnowned, Content: "winner-content", Include: true},
		},
	}
	snap3 := makeSnapshot("host3")
	snap3.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/app.conf", Kind: schema.ConfigFileKindUnowned, Content: "loser-content", Include: true},
		},
	}

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2, snap3}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	if len(merged.Config.Files) != 2 {
		t.Fatalf("expected 2 config entries (variants), got %d", len(merged.Config.Files))
	}

	// Find the winner and loser
	var winner, loser *schema.ConfigFileEntry
	for i := range merged.Config.Files {
		if merged.Config.Files[i].Content == "winner-content" {
			winner = &merged.Config.Files[i]
		} else {
			loser = &merged.Config.Files[i]
		}
	}
	if winner == nil || loser == nil {
		t.Fatal("expected both winner and loser variants")
	}
	if !winner.Include {
		t.Error("winner variant should have include=true")
	}
	if loser.Include {
		t.Error("loser variant should have include=false")
	}
}

func TestMergeSnapshotsServices(t *testing.T) {
	snap1 := makeRichSnapshot("host1", nil, nil, []string{"httpd.service", "sshd.service"})
	snap2 := makeRichSnapshot("host2", nil, nil, []string{"httpd.service", "nginx.service"})

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	if merged.Services == nil {
		t.Fatal("expected services section")
	}

	// httpd on both hosts
	httpd := findServiceByUnit(merged.Services.StateChanges, "httpd.service")
	if httpd == nil || httpd.Fleet == nil || httpd.Fleet.Count != 2 {
		t.Error("httpd.service should be present on 2 hosts")
	}

	// sshd on 1 host
	sshd := findServiceByUnit(merged.Services.StateChanges, "sshd.service")
	if sshd == nil || sshd.Fleet == nil || sshd.Fleet.Count != 1 {
		t.Error("sshd.service should be present on 1 host")
	}
}

func TestMergeSnapshotsFleetMetadata(t *testing.T) {
	snap1 := makeRichSnapshot("host1.example.com", []string{"httpd"}, nil, nil)
	snap2 := makeRichSnapshot("host2.example.com", []string{"httpd"}, nil, nil)

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2}, 80)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	// Check fleet metadata in meta
	fleetRaw, ok := merged.Meta["fleet"]
	if !ok {
		t.Fatal("expected fleet metadata in meta")
	}
	fleetMap, ok := fleetRaw.(map[string]interface{})
	if !ok {
		t.Fatalf("fleet metadata is not a map: %T", fleetRaw)
	}

	totalHosts, ok := fleetMap["total_hosts"].(float64)
	if !ok || int(totalHosts) != 2 {
		t.Errorf("total_hosts = %v, want 2", fleetMap["total_hosts"])
	}

	minPrev, ok := fleetMap["min_prevalence"].(float64)
	if !ok || int(minPrev) != 80 {
		t.Errorf("min_prevalence = %v, want 80", fleetMap["min_prevalence"])
	}

	sourceHosts, ok := fleetMap["source_hosts"].([]interface{})
	if !ok || len(sourceHosts) != 2 {
		t.Errorf("source_hosts length = %v, want 2", len(sourceHosts))
	}

	// hostname should be fleet-merged
	if merged.Meta["hostname"] != "fleet-merged" {
		t.Errorf("hostname = %v, want fleet-merged", merged.Meta["hostname"])
	}
}

func TestMergeSnapshotsOsRelease(t *testing.T) {
	snap1 := makeRichSnapshot("host1", []string{"httpd"}, nil, nil)
	snap2 := makeRichSnapshot("host2", []string{"nginx"}, nil, nil)

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	if merged.OsRelease == nil {
		t.Fatal("expected os_release in merged snapshot")
	}
	if merged.OsRelease.ID != "rhel" {
		t.Errorf("os_release.id = %s, want rhel", merged.OsRelease.ID)
	}
}

func TestMergeSnapshotsNilSections(t *testing.T) {
	// Snapshots with no sections should merge without error
	snap1 := makeSnapshot("host1")
	snap2 := makeSnapshot("host2")

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}
	if merged.Rpm != nil {
		t.Error("expected nil rpm section when no snapshots have rpm")
	}
	if merged.Config != nil {
		t.Error("expected nil config section when no snapshots have config")
	}
	if merged.Services != nil {
		t.Error("expected nil services section when no snapshots have services")
	}
}

func TestMergeSnapshotsNetwork(t *testing.T) {
	snap1 := makeSnapshot("host1")
	snap1.Network = &schema.NetworkSection{
		FirewallZones: []schema.FirewallZone{
			{Name: "public", Path: "/etc/firewalld/zones/public.xml", Content: "<zone/>", Include: true},
		},
	}
	snap2 := makeSnapshot("host2")
	snap2.Network = &schema.NetworkSection{
		FirewallZones: []schema.FirewallZone{
			{Name: "public", Path: "/etc/firewalld/zones/public.xml", Content: "<zone/>", Include: true},
			{Name: "internal", Path: "/etc/firewalld/zones/internal.xml", Content: "<zone/>", Include: true},
		},
	}

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	if merged.Network == nil {
		t.Fatal("expected network section")
	}
	if len(merged.Network.FirewallZones) != 2 {
		t.Errorf("expected 2 firewall zones, got %d", len(merged.Network.FirewallZones))
	}
}

func TestMergeSnapshotsScheduledTasks(t *testing.T) {
	snap1 := makeSnapshot("host1")
	snap1.ScheduledTasks = &schema.ScheduledTaskSection{
		CronJobs: []schema.CronJob{
			{Path: "/etc/cron.d/backup", Source: "file", Include: true},
		},
		GeneratedTimerUnits: []schema.GeneratedTimerUnit{
			{Name: "backup.timer", Include: true},
		},
	}
	snap2 := makeSnapshot("host2")
	snap2.ScheduledTasks = &schema.ScheduledTaskSection{
		CronJobs: []schema.CronJob{
			{Path: "/etc/cron.d/backup", Source: "file", Include: true},
			{Path: "/etc/cron.d/logrotate", Source: "file", Include: true},
		},
	}

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	if merged.ScheduledTasks == nil {
		t.Fatal("expected scheduled_tasks section")
	}
	if len(merged.ScheduledTasks.CronJobs) != 2 {
		t.Errorf("expected 2 cron jobs, got %d", len(merged.ScheduledTasks.CronJobs))
	}
}

func TestMergeSnapshotsGoldenFile(t *testing.T) {
	// Build 3 overlapping snapshots
	bi := "quay.io/centos-bootc/centos-bootc:stream9"
	snap1 := makeSnapshot("host1.example.com")
	snap1.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", Epoch: "0", Version: "2.4.57", Release: "5.el9", Arch: "x86_64", State: schema.PackageStateAdded, Include: true},
			{Name: "vim", Epoch: "2", Version: "9.0", Release: "1.el9", Arch: "x86_64", State: schema.PackageStateAdded, Include: true},
		},
		BaseImage: &bi,
	}
	snap1.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/httpd/httpd.conf", Kind: schema.ConfigFileKindRpmOwnedModified, Category: schema.ConfigCategoryOther, Content: "ServerName host1", Include: true},
		},
	}
	snap1.Services = &schema.ServiceSection{
		StateChanges: []schema.ServiceStateChange{
			{Unit: "httpd.service", CurrentState: "enabled", DefaultState: "disabled", Action: "enable", Include: true},
		},
		EnabledUnits: []string{"httpd.service"},
	}

	snap2 := makeSnapshot("host2.example.com")
	snap2.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", Epoch: "0", Version: "2.4.57", Release: "5.el9", Arch: "x86_64", State: schema.PackageStateAdded, Include: true},
			{Name: "nginx", Epoch: "0", Version: "1.24", Release: "1.el9", Arch: "x86_64", State: schema.PackageStateAdded, Include: true},
		},
		BaseImage: &bi,
	}
	snap2.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/httpd/httpd.conf", Kind: schema.ConfigFileKindRpmOwnedModified, Category: schema.ConfigCategoryOther, Content: "ServerName host2", Include: true},
		},
	}
	snap2.Services = &schema.ServiceSection{
		StateChanges: []schema.ServiceStateChange{
			{Unit: "httpd.service", CurrentState: "enabled", DefaultState: "disabled", Action: "enable", Include: true},
			{Unit: "nginx.service", CurrentState: "enabled", DefaultState: "disabled", Action: "enable", Include: true},
		},
		EnabledUnits: []string{"httpd.service", "nginx.service"},
	}

	snap3 := makeSnapshot("host3.example.com")
	snap3.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", Epoch: "0", Version: "2.4.57", Release: "5.el9", Arch: "x86_64", State: schema.PackageStateAdded, Include: true},
		},
		BaseImage: &bi,
	}
	snap3.Services = &schema.ServiceSection{
		StateChanges: []schema.ServiceStateChange{
			{Unit: "httpd.service", CurrentState: "enabled", DefaultState: "disabled", Action: "enable", Include: true},
		},
		EnabledUnits: []string{"httpd.service"},
	}

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2, snap3}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	mergedJSON, err := json.MarshalIndent(merged, "", "  ")
	if err != nil {
		t.Fatalf("marshal merged: %v", err)
	}

	goldenPath := filepath.Join("testdata", "golden-fleet-merge.json")
	if os.Getenv("UPDATE_GOLDEN") == "1" {
		os.MkdirAll("testdata", 0755)
		if err := os.WriteFile(goldenPath, mergedJSON, 0644); err != nil {
			t.Fatalf("write golden file: %v", err)
		}
		t.Log("Golden file updated")
		return
	}

	golden, err := os.ReadFile(goldenPath)
	if err != nil {
		t.Fatalf("read golden file (run with UPDATE_GOLDEN=1 to create): %v", err)
	}

	// Compare via re-parsed JSON for stable comparison.
	// Normalize the timestamp field since it changes every run.
	var mergedParsed, goldenParsed map[string]interface{}
	json.Unmarshal(mergedJSON, &mergedParsed)
	json.Unmarshal(golden, &goldenParsed)
	normalizeTimestamp(mergedParsed)
	normalizeTimestamp(goldenParsed)

	mergedNorm, _ := json.MarshalIndent(mergedParsed, "", "  ")
	goldenNorm, _ := json.MarshalIndent(goldenParsed, "", "  ")

	if string(mergedNorm) != string(goldenNorm) {
		// Write actual for diffing
		actualPath := filepath.Join("testdata", "golden-fleet-merge.actual.json")
		os.WriteFile(actualPath, mergedJSON, 0644)
		t.Fatalf("merged JSON does not match golden file.\nActual written to: %s\nDiff with: diff %s %s",
			actualPath, goldenPath, actualPath)
	}
}

func TestMergeSnapshotsUsersGroups(t *testing.T) {
	snap1 := makeSnapshot("host1")
	snap1.UsersGroups = &schema.UserGroupSection{
		Users: []map[string]interface{}{
			{"name": "app-user", "uid": float64(1001)},
		},
		Groups: []map[string]interface{}{
			{"name": "app-group", "gid": float64(1001)},
		},
		SudoersRules: []string{"app-user ALL=(ALL) NOPASSWD: ALL"},
	}
	snap2 := makeSnapshot("host2")
	snap2.UsersGroups = &schema.UserGroupSection{
		Users: []map[string]interface{}{
			{"name": "app-user", "uid": float64(1001)},
			{"name": "deploy", "uid": float64(1002)},
		},
		Groups: []map[string]interface{}{
			{"name": "app-group", "gid": float64(1001)},
		},
		SudoersRules: []string{"app-user ALL=(ALL) NOPASSWD: ALL", "deploy ALL=(ALL) NOPASSWD: /usr/bin/systemctl"},
	}

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	if merged.UsersGroups == nil {
		t.Fatal("expected users_groups section")
	}
	if len(merged.UsersGroups.Users) != 2 {
		t.Errorf("expected 2 users, got %d", len(merged.UsersGroups.Users))
	}
	if len(merged.UsersGroups.SudoersRules) != 2 {
		t.Errorf("expected 2 sudoers rules, got %d", len(merged.UsersGroups.SudoersRules))
	}
}

func TestMergeSnapshotsWarnings(t *testing.T) {
	snap1 := makeSnapshot("host1")
	snap1.Warnings = []map[string]interface{}{
		{"message": "disk nearly full", "path": "/var"},
	}
	snap2 := makeSnapshot("host2")
	snap2.Warnings = []map[string]interface{}{
		{"message": "disk nearly full", "path": "/var"},
		{"message": "SELinux permissive", "path": ""},
	}

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	// Deduplicated: first warning appears on both hosts but should appear once
	if len(merged.Warnings) != 2 {
		t.Errorf("expected 2 deduplicated warnings, got %d", len(merged.Warnings))
	}
}

func TestMergeSnapshotsSelinux(t *testing.T) {
	snap1 := makeSnapshot("host1")
	snap1.Selinux = &schema.SelinuxSection{
		Mode: "enforcing",
		PortLabels: []schema.SelinuxPortLabel{
			{Protocol: "tcp", Port: "8080", Type: "http_port_t", Include: true},
		},
		CustomModules: []string{"my_module"},
	}
	snap2 := makeSnapshot("host2")
	snap2.Selinux = &schema.SelinuxSection{
		Mode: "enforcing",
		PortLabels: []schema.SelinuxPortLabel{
			{Protocol: "tcp", Port: "8080", Type: "http_port_t", Include: true},
			{Protocol: "tcp", Port: "9090", Type: "http_port_t", Include: true},
		},
		CustomModules: []string{"my_module", "other_module"},
	}

	merged, err := MergeSnapshots([]*schema.InspectionSnapshot{snap1, snap2}, 100)
	if err != nil {
		t.Fatalf("MergeSnapshots: %v", err)
	}

	if merged.Selinux == nil {
		t.Fatal("expected selinux section")
	}
	if len(merged.Selinux.PortLabels) != 2 {
		t.Errorf("expected 2 port labels, got %d", len(merged.Selinux.PortLabels))
	}
	if len(merged.Selinux.CustomModules) != 2 {
		t.Errorf("expected 2 custom modules, got %d", len(merged.Selinux.CustomModules))
	}
}

// Helper functions

func findPackageByName(pkgs []schema.PackageEntry, name string) *schema.PackageEntry {
	for i := range pkgs {
		if pkgs[i].Name == name {
			return &pkgs[i]
		}
	}
	return nil
}

func findServiceByUnit(scs []schema.ServiceStateChange, unit string) *schema.ServiceStateChange {
	for i := range scs {
		if scs[i].Unit == unit {
			return &scs[i]
		}
	}
	return nil
}

func safeCount(fp *schema.FleetPrevalence) int {
	if fp == nil {
		return 0
	}
	return fp.Count
}

func safeTotal(fp *schema.FleetPrevalence) int {
	if fp == nil {
		return 0
	}
	return fp.Total
}

// normalizeTimestamp replaces the meta.timestamp with a fixed value
// so golden file comparisons are deterministic.
func normalizeTimestamp(parsed map[string]interface{}) {
	if meta, ok := parsed["meta"].(map[string]interface{}); ok {
		meta["timestamp"] = "NORMALIZED"
	}
}
