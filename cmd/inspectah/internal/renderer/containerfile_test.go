package renderer

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// --- Section line tests ---

func TestPackagesSectionEmpty(t *testing.T) {
	snap := schema.NewSnapshot()
	lines := packagesSectionLines(snap, "registry.redhat.io/rhel9/rhel-bootc:9.4", nil, false)
	// Empty snapshot should produce FROM line and minimal content
	if len(lines) == 0 {
		t.Error("expected at least FROM line")
	}
	found := false
	for _, l := range lines {
		if strings.HasPrefix(l, "FROM ") {
			found = true
			break
		}
	}
	if !found {
		t.Error("expected FROM line in packages section")
	}
}

func TestPackagesSectionWithAddedPackages(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", Include: true},
			{Name: "nginx", Include: true},
			{Name: "excluded-pkg", Include: false},
		},
	}
	lines := packagesSectionLines(snap, "registry.redhat.io/rhel9/rhel-bootc:9.4", nil, false)
	content := strings.Join(lines, "\n")
	if !strings.Contains(content, "httpd") {
		t.Error("expected httpd in output")
	}
	if !strings.Contains(content, "nginx") {
		t.Error("expected nginx in output")
	}
	if strings.Contains(content, "excluded-pkg") {
		t.Error("excluded package should not appear")
	}
}

func TestServicesSectionEmpty(t *testing.T) {
	snap := schema.NewSnapshot()
	lines := servicesSectionLines(snap)
	if len(lines) != 0 {
		t.Errorf("empty snapshot should produce no service lines, got %d", len(lines))
	}
}

func TestServicesSectionEnableDisable(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Services = &schema.ServiceSection{
		EnabledUnits:  []string{"httpd.service", "nginx.service"},
		DisabledUnits: []string{"firewalld.service"},
	}
	lines := servicesSectionLines(snap)
	content := strings.Join(lines, "\n")
	if !strings.Contains(content, "systemctl enable") {
		t.Error("expected systemctl enable")
	}
	if !strings.Contains(content, "httpd.service") {
		t.Error("expected httpd.service")
	}
	if !strings.Contains(content, "systemctl disable") {
		t.Error("expected systemctl disable")
	}
}

func TestNetworkSectionFirewallOnly(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Network = &schema.NetworkSection{
		FirewallZones: []schema.FirewallZone{
			{Name: "public", Include: true, Content: "<zone/>"},
		},
	}
	lines := networkSectionLines(snap, true)
	content := strings.Join(lines, "\n")
	if !strings.Contains(content, "Firewall") {
		t.Error("expected firewall section")
	}
}

func TestScheduledTasksEmpty(t *testing.T) {
	snap := schema.NewSnapshot()
	lines := scheduledTasksSectionLines(snap)
	if len(lines) != 0 {
		t.Errorf("expected no lines, got %d", len(lines))
	}
}

func TestScheduledTasksWithTimers(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.ScheduledTasks = &schema.ScheduledTaskSection{
		GeneratedTimerUnits: []schema.GeneratedTimerUnit{
			{Name: "cron-backup", Include: true, TimerContent: "[Timer]", ServiceContent: "[Service]"},
		},
	}
	lines := scheduledTasksSectionLines(snap)
	content := strings.Join(lines, "\n")
	if !strings.Contains(content, "Scheduled Tasks") {
		t.Error("expected section header")
	}
	if !strings.Contains(content, "cron-backup") {
		t.Error("expected timer name")
	}
}

func TestContainersSectionEmpty(t *testing.T) {
	snap := schema.NewSnapshot()
	lines := containersSectionLines(snap)
	if len(lines) != 0 {
		t.Errorf("expected no lines, got %d", len(lines))
	}
}

func TestContainersSectionQuadlets(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		QuadletUnits: []schema.QuadletUnit{
			{Name: "webapp.container", Include: true, Content: "[Container]"},
		},
	}
	lines := containersSectionLines(snap)
	content := strings.Join(lines, "\n")
	if !strings.Contains(content, "Container Workloads") {
		t.Error("expected section header")
	}
	if !strings.Contains(content, "COPY quadlet/") {
		t.Error("expected COPY quadlet/")
	}
}

func TestKernelBootSectionEmpty(t *testing.T) {
	snap := schema.NewSnapshot()
	lines := kernelBootSectionLines(snap)
	if len(lines) != 0 {
		t.Errorf("expected no lines, got %d", len(lines))
	}
}

func TestKernelBootSectionWithKargs(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.KernelBoot = &schema.KernelBootSection{
		Cmdline: "ro root=/dev/sda1 net.ifnames=0",
	}
	lines := kernelBootSectionLines(snap)
	content := strings.Join(lines, "\n")
	if !strings.Contains(content, "kargs.d") {
		t.Error("expected kargs.d reference")
	}
}

func TestSelinuxSectionEmpty(t *testing.T) {
	snap := schema.NewSnapshot()
	lines := selinuxSectionLines(snap)
	if len(lines) != 0 {
		t.Errorf("expected no lines, got %d", len(lines))
	}
}

func TestSelinuxSectionWithBooleans(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Selinux = &schema.SelinuxSection{
		BooleanOverrides: []map[string]interface{}{
			{"name": "httpd_can_network_connect", "current_value": "on", "non_default": true},
		},
	}
	lines := selinuxSectionLines(snap)
	content := strings.Join(lines, "\n")
	if !strings.Contains(content, "SELinux") {
		t.Error("expected section header")
	}
	if !strings.Contains(content, "setsebool") {
		t.Error("expected setsebool command")
	}
}

func TestUsersSectionEmpty(t *testing.T) {
	snap := schema.NewSnapshot()
	lines := usersSectionLines(snap)
	if len(lines) != 0 {
		t.Errorf("expected no lines, got %d", len(lines))
	}
}

// --- Config tree tests ---

func TestWriteConfigTreeCreatesDir(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Include: true, Content: "ServerName test"},
		},
	}

	writeConfigTree(snap, outDir)

	path := filepath.Join(outDir, "config", "etc", "httpd", "conf", "httpd.conf")
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("config file not written: %v", err)
	}
	if string(data) != "ServerName test" {
		t.Errorf("content mismatch: got %q", string(data))
	}
}

func TestWriteConfigTreeSkipsExcluded(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/included.conf", Include: true, Content: "yes"},
			{Path: "/etc/excluded.conf", Include: false, Content: "no"},
		},
	}

	writeConfigTree(snap, outDir)

	if _, err := os.Stat(filepath.Join(outDir, "config", "etc", "excluded.conf")); !os.IsNotExist(err) {
		t.Error("excluded file should not be written")
	}
}

func TestWriteConfigTreeDropIns(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()
	snap.Services = &schema.ServiceSection{
		DropIns: []schema.SystemdDropIn{
			{Path: "etc/systemd/system/httpd.service.d/override.conf", Include: true, Content: "[Service]\nLimitNOFILE=65535"},
		},
	}

	writeConfigTree(snap, outDir)

	// Should be in both config/ and drop-ins/
	configPath := filepath.Join(outDir, "config", "etc", "systemd", "system", "httpd.service.d", "override.conf")
	dropinPath := filepath.Join(outDir, "drop-ins", "etc", "systemd", "system", "httpd.service.d", "override.conf")

	for _, p := range []string{configPath, dropinPath} {
		if _, err := os.Stat(p); os.IsNotExist(err) {
			t.Errorf("expected file at %s", p)
		}
	}
}

// --- Full Containerfile render test ---

func TestRenderContainerfileMinimal(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()

	err := RenderContainerfile(snap, outDir)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(outDir, "Containerfile"))
	if err != nil {
		t.Fatalf("read Containerfile: %v", err)
	}
	content := string(data)
	if !strings.Contains(content, "FROM ") {
		t.Error("Containerfile missing FROM line")
	}
	if !strings.Contains(content, "bootc container lint") {
		t.Error("Containerfile missing bootc lint")
	}
}

func TestRenderContainerfileWithPackages(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", Include: true},
			{Name: "mod_ssl", Include: true},
		},
	}
	snap.Services = &schema.ServiceSection{
		EnabledUnits: []string{"httpd.service"},
	}

	err := RenderContainerfile(snap, outDir)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(outDir, "Containerfile"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	content := string(data)
	if !strings.Contains(content, "httpd") {
		t.Error("missing httpd")
	}
	if !strings.Contains(content, "systemctl enable") {
		t.Error("missing systemctl enable")
	}
}

func TestWriteRedactedDir(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()

	// Add a redaction finding via raw JSON
	snap.Redactions = append(snap.Redactions,
		mustMarshal(schema.RedactionFinding{
			Path:   "/etc/pki/tls/private/server.key",
			Source: "file",
			Kind:   "excluded",
			Remediation: "provision",
		}),
	)

	err := WriteRedactedDir(snap, outDir)
	if err != nil {
		t.Fatalf("WriteRedactedDir: %v", err)
	}

	redactedFile := filepath.Join(outDir, "redacted", "etc", "pki", "tls", "private", "server.key.REDACTED")
	data, err := os.ReadFile(redactedFile)
	if err != nil {
		t.Fatalf("read redacted file: %v", err)
	}
	if !strings.Contains(string(data), "REDACTED") {
		t.Error("redacted file should contain REDACTED marker")
	}
}

func TestContainerfile_DisplayOnlyIncludeDoesNotAffectOutput(t *testing.T) {
	makeSnap := func() *schema.InspectionSnapshot {
		snap := schema.NewSnapshot()
		tr, fa := true, false
		snap.Network = &schema.NetworkSection{
			Connections: []schema.NMConnection{
				{Name: "eth0", Include: &tr},
				{Name: "eth1", Include: &fa},
			},
		}
		snap.Storage = &schema.StorageSection{
			FstabEntries: []schema.FstabEntry{
				{MountPoint: "/data", Fstype: "xfs", Include: &tr},
				{MountPoint: "/backup", Fstype: "ext4", Include: &fa},
			},
		}
		return snap
	}

	// Render with all included
	dir1 := t.TempDir()
	snap1 := makeSnap()
	require.NoError(t, RenderContainerfile(snap1, dir1))
	cf1, _ := os.ReadFile(filepath.Join(dir1, "Containerfile"))

	// Render with some excluded
	dir2 := t.TempDir()
	snap2 := makeSnap()
	fa := false
	snap2.Network.Connections[0].Include = &fa
	snap2.Storage.FstabEntries[0].Include = &fa
	require.NoError(t, RenderContainerfile(snap2, dir2))
	cf2, _ := os.ReadFile(filepath.Join(dir2, "Containerfile"))

	assert.Equal(t, string(cf1), string(cf2),
		"display-only Include changes must not affect Containerfile output")
}

func TestPackagesSectionLines_LocalInstallEmitsTODO(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "custom-agent", Arch: "x86_64", Include: true, State: schema.PackageStateLocalInstall, Version: "1.0", Release: "1"},
			{Name: "orphan-tool", Arch: "x86_64", Include: true, State: schema.PackageStateNoRepo, Version: "2.0", Release: "1"},
		},
	}

	lines := packagesSectionLines(snap, "registry.redhat.io/rhel9/rhel-bootc:9.4", nil, false)
	output := strings.Join(lines, "\n")

	// vim should be in dnf install
	assert.Contains(t, output, "vim")
	assert.Contains(t, output, "dnf install")

	// custom-agent and orphan-tool should NOT be in dnf install
	assert.NotContains(t, output, "dnf install -y custom-agent")
	assert.NotContains(t, output, "dnf install -y orphan-tool")

	// They should have TODO comments
	assert.Contains(t, output, "# TODO: 'custom-agent'")
	assert.Contains(t, output, "local_install")
	assert.Contains(t, output, "# TODO: 'orphan-tool'")
	assert.Contains(t, output, "no_repo")
	assert.Contains(t, output, "Manual Follow-up Required")
}

func TestPackagesSectionLines_LeafPackagesFiltersUnreachable(t *testing.T) {
	leafNames := []string{"vim", "custom-agent"}
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		LeafPackages: &leafNames,
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
			{Name: "custom-agent", Arch: "x86_64", Include: true, State: schema.PackageStateLocalInstall},
		},
	}

	lines := packagesSectionLines(snap, "registry.redhat.io/rhel9/rhel-bootc:9.4", nil, false)
	output := strings.Join(lines, "\n")

	// vim should be installed
	assert.Contains(t, output, "vim")

	// custom-agent should be a TODO, not in dnf install
	assert.NotContains(t, output, "dnf install -y custom-agent")
	assert.Contains(t, output, "# TODO: 'custom-agent'")
}

func TestServicesSectionLines_UnreachableOwnerExcluded(t *testing.T) {
	ownerPkg := "custom-agent"
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "custom-agent", Arch: "x86_64", Include: true, State: schema.PackageStateLocalInstall},
			{Name: "httpd", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
		},
	}
	snap.Services = &schema.ServiceSection{
		EnabledUnits: []string{"custom-agent.service", "httpd.service"},
		StateChanges: []schema.ServiceStateChange{
			{Unit: "custom-agent.service", OwningPackage: &ownerPkg},
			{Unit: "httpd.service"},
		},
	}

	lines := servicesSectionLines(snap)
	output := strings.Join(lines, "\n")

	// httpd.service should be enabled (no owner restriction)
	assert.Contains(t, output, "httpd.service")

	// custom-agent.service should NOT be enabled (owner is unreachable)
	assert.NotContains(t, output, "custom-agent.service")
}

// --- Non-RPM section tests ---

func TestNonRpmSectionLines_MigrationPlannedStubs(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.NonRpmSoftware = &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{Path: "opt/deploy.sh", Name: "deploy.sh", Method: "standalone binary", ReviewStatus: "migration_planned", Notes: "Ship as-is", Lang: "shell", Static: false},
			{Path: "usr/local/bin/myapp", Name: "myapp", Method: "go binary", ReviewStatus: "migration_planned", Lang: "go", Static: true},
			{Path: "usr/local/bin/cbridge", Name: "cbridge", Method: "standalone binary", ReviewStatus: "migration_planned", Lang: "c", Static: false, SharedLibs: []string{"libc.so.6", "libssl.so"}},
			{Path: "opt/app/venv", Name: "custom_lib", Method: "pip dist-info", ReviewStatus: "migration_planned", Version: "1.0.0", HasCExtensions: false},
			{Path: "opt/app2/venv", Name: "numpy", Method: "pip dist-info", ReviewStatus: "migration_planned", Version: "1.24.0", HasCExtensions: true},
			{Path: "opt/agent/bin/agent", Name: "agent", Method: "standalone binary", ReviewStatus: "reviewed"},
			{Path: "opt/other", Name: "other", Method: "standalone binary", ReviewStatus: "not_reviewed"},
		},
	}
	lines := nonRpmSectionLines(snap, nil, false)
	content := strings.Join(lines, "\n")

	// Shell script should produce COPY stub
	assert.Contains(t, content, "# COPY opt/deploy.sh /usr/local/bin/", "shell script should produce COPY stub")
	// Static Go binary should produce COPY stub
	assert.Contains(t, content, "# COPY usr/local/bin/myapp /usr/local/bin/", "static Go binary should produce COPY stub")
	// Dynamic binary should list shared libs
	assert.Contains(t, content, "libc.so.6", "dynamic binary should list shared libs")
	// Dynamic binary should NOT produce COPY stub
	assert.NotContains(t, content, "COPY usr/local/bin/cbridge", "dynamic binary should NOT produce COPY stub")
	// Pure pip should produce pip install stub
	assert.Contains(t, content, "pip install", "pure pip should produce pip install stub")
	// C-extension pip should produce warning about native extensions
	assert.Contains(t, content, "native extensions", "c-extension pip should produce warning")
	// Reviewed items should not appear
	assert.NotContains(t, content, "agent", "reviewed items should not appear")
	// Not-reviewed items should not appear
	assert.NotContains(t, content, "other", "not_reviewed items should not appear")
}

func TestNonRpmSectionLines_NoMigrationPlannedItems(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.NonRpmSoftware = &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{Path: "opt/tool", Name: "tool", Method: "standalone binary", ReviewStatus: "reviewed"},
		},
	}
	lines := nonRpmSectionLines(snap, nil, false)
	assert.Empty(t, lines, "should produce no output for non-migration_planned items")
}
