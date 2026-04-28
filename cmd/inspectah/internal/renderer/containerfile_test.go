package renderer

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
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
