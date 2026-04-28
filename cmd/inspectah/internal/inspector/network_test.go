package inspector

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

func loadNetworkFixture(t *testing.T, name string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "network", name))
	require.NoError(t, err, "loading fixture %s", name)
	return string(data)
}

// ---------------------------------------------------------------------------
// NM connection classification
// ---------------------------------------------------------------------------

func TestClassifyConnection(t *testing.T) {
	tests := []struct {
		name       string
		input      string
		wantMethod string
		wantType   string
	}{
		{
			name:       "DHCP ethernet",
			input:      loadNetworkFixture(t, "eth0.nmconnection"),
			wantMethod: "dhcp",
			wantType:   "ethernet",
		},
		{
			name:       "static bond",
			input:      loadNetworkFixture(t, "static-bond.nmconnection"),
			wantMethod: "static",
			wantType:   "bond",
		},
		{
			name:       "empty input",
			input:      "",
			wantMethod: "unknown",
			wantType:   "",
		},
		{
			name:       "no ipv4 section",
			input:      "[connection]\ntype=wifi\n",
			wantMethod: "unknown",
			wantType:   "wifi",
		},
		{
			name:       "shared method",
			input:      "[connection]\ntype=ethernet\n[ipv4]\nmethod=shared\n",
			wantMethod: "shared",
			wantType:   "ethernet",
		},
		{
			name:       "disabled method",
			input:      "[connection]\ntype=ethernet\n[ipv4]\nmethod=disabled\n",
			wantMethod: "disabled",
			wantType:   "ethernet",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			method, connType := classifyConnection(tt.input)
			assert.Equal(t, tt.wantMethod, method)
			assert.Equal(t, tt.wantType, connType)
		})
	}
}

// ---------------------------------------------------------------------------
// Firewall zone XML parsing
// ---------------------------------------------------------------------------

func TestParseZoneXML(t *testing.T) {
	tests := []struct {
		name          string
		input         string
		wantServices  []string
		wantPorts     []string
		wantRichRules int
	}{
		{
			name:          "public zone with services ports and rules",
			input:         loadNetworkFixture(t, "public-zone.xml"),
			wantServices:  []string{"ssh", "dhcpv6-client"},
			wantPorts:     []string{"8080/tcp", "443/tcp"},
			wantRichRules: 1,
		},
		{
			name:          "minimal zone",
			input:         `<zone><service name="http"/></zone>`,
			wantServices:  []string{"http"},
			wantPorts:     nil,
			wantRichRules: 0,
		},
		{
			name:          "port without protocol",
			input:         `<zone><port port="9090"/></zone>`,
			wantServices:  nil,
			wantPorts:     []string{"9090"},
			wantRichRules: 0,
		},
		{
			name:          "invalid XML",
			input:         "not xml at all",
			wantServices:  nil,
			wantPorts:     nil,
			wantRichRules: 0,
		},
		{
			name:          "empty zone",
			input:         `<zone></zone>`,
			wantServices:  nil,
			wantPorts:     nil,
			wantRichRules: 0,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			services, ports, richRules := parseZoneXML(tt.input)
			if tt.wantServices == nil {
				assert.Empty(t, services)
			} else {
				assert.Equal(t, tt.wantServices, services)
			}
			if tt.wantPorts == nil {
				assert.Empty(t, ports)
			} else {
				assert.Equal(t, tt.wantPorts, ports)
			}
			assert.Len(t, richRules, tt.wantRichRules)
		})
	}
}

// ---------------------------------------------------------------------------
// Firewall direct rules
// ---------------------------------------------------------------------------

func TestCollectFirewallDirectRules(t *testing.T) {
	directXML := loadNetworkFixture(t, "direct.xml")
	exec := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/firewalld/direct.xml": directXML,
	})

	section := &schema.NetworkSection{
		FirewallDirectRules: []schema.FirewallDirectRule{},
	}
	collectFirewallDirectRules(exec, section)

	require.Len(t, section.FirewallDirectRules, 2)
	assert.Equal(t, "ipv4", section.FirewallDirectRules[0].Ipv)
	assert.Equal(t, "filter", section.FirewallDirectRules[0].Table)
	assert.Equal(t, "INPUT", section.FirewallDirectRules[0].Chain)
	assert.Equal(t, "0", section.FirewallDirectRules[0].Priority)
	assert.Contains(t, section.FirewallDirectRules[0].Args, "--dport 9090")

	assert.Equal(t, "FORWARD", section.FirewallDirectRules[1].Chain)
	assert.Equal(t, "1", section.FirewallDirectRules[1].Priority)
}

func TestCollectFirewallDirectRulesMissing(t *testing.T) {
	exec := NewFakeExecutor(nil)
	section := &schema.NetworkSection{
		FirewallDirectRules: []schema.FirewallDirectRule{},
	}
	collectFirewallDirectRules(exec, section)
	assert.Empty(t, section.FirewallDirectRules)
}

// ---------------------------------------------------------------------------
// resolv.conf provenance
// ---------------------------------------------------------------------------

func TestDetectResolvProvenance(t *testing.T) {
	tests := []struct {
		name    string
		fixture string
		want    string
	}{
		{
			name:    "NetworkManager managed",
			fixture: "resolv-nm.conf",
			want:    "networkmanager",
		},
		{
			name:    "systemd-resolved",
			fixture: "resolv-systemd.conf",
			want:    "systemd-resolved",
		},
		{
			name:    "hand-edited",
			fixture: "resolv-manual.conf",
			want:    "hand-edited",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			content := loadNetworkFixture(t, tt.fixture)
			exec := NewFakeExecutor(nil).WithFiles(map[string]string{
				"/etc/resolv.conf": content,
			})
			got := detectResolvProvenance(exec)
			assert.Equal(t, tt.want, got)
		})
	}
}

func TestDetectResolvProvenanceMissing(t *testing.T) {
	exec := NewFakeExecutor(nil)
	got := detectResolvProvenance(exec)
	assert.Equal(t, "", got)
}

// ---------------------------------------------------------------------------
// Hosts additions
// ---------------------------------------------------------------------------

func TestCollectHostsAdditions(t *testing.T) {
	hostsContent := loadNetworkFixture(t, "hosts")
	exec := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/hosts": hostsContent,
	})
	section := &schema.NetworkSection{HostsAdditions: []string{}}
	collectHostsAdditions(exec, section)

	// Should exclude localhost lines and include the three custom entries.
	assert.Len(t, section.HostsAdditions, 3)
	assert.Contains(t, section.HostsAdditions[0], "db-primary")
	assert.Contains(t, section.HostsAdditions[1], "db-replica")
	assert.Contains(t, section.HostsAdditions[2], "app-server")
}

func TestCollectHostsAdditionsEmpty(t *testing.T) {
	exec := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/hosts": "127.0.0.1 localhost\n::1 localhost\n",
	})
	section := &schema.NetworkSection{HostsAdditions: []string{}}
	collectHostsAdditions(exec, section)
	assert.Empty(t, section.HostsAdditions)
}

func TestCollectHostsAdditionsMissing(t *testing.T) {
	exec := NewFakeExecutor(nil)
	section := &schema.NetworkSection{HostsAdditions: []string{}}
	collectHostsAdditions(exec, section)
	assert.Empty(t, section.HostsAdditions)
}

// ---------------------------------------------------------------------------
// IP route / IP rule parsing
// ---------------------------------------------------------------------------

func TestParseIPRoutes(t *testing.T) {
	input := loadNetworkFixture(t, "ip-route.txt")
	routes := parseIPRoutes(input)
	assert.Len(t, routes, 4)
	assert.Contains(t, routes[0], "default via 10.0.0.1")
	assert.Contains(t, routes[2], "docker0")
}

func TestParseIPRoutesEmpty(t *testing.T) {
	routes := parseIPRoutes("")
	assert.Empty(t, routes)
}

func TestParseIPRules(t *testing.T) {
	input := loadNetworkFixture(t, "ip-rule.txt")
	rules := parseIPRules(input)
	// Should filter out local, main, default — leaving only the custom table.
	assert.Len(t, rules, 1)
	assert.Contains(t, rules[0], "custom_table")
}

func TestParseIPRulesEmpty(t *testing.T) {
	rules := parseIPRules("")
	assert.Empty(t, rules)
}

func TestParseIPRulesNoCustom(t *testing.T) {
	input := "0:\tfrom all lookup local\n32766:\tfrom all lookup main\n32767:\tfrom all lookup default\n"
	rules := parseIPRules(input)
	assert.Empty(t, rules)
}

// ---------------------------------------------------------------------------
// Proxy scanning
// ---------------------------------------------------------------------------

func TestCollectProxy(t *testing.T) {
	proxyEnv := loadNetworkFixture(t, "proxy-environment")
	exec := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/environment": proxyEnv,
	})
	section := &schema.NetworkSection{Proxy: []schema.ProxyEntry{}}
	collectProxy(exec, section)

	// Should capture http_proxy, https_proxy, no_proxy but not PATH.
	assert.Len(t, section.Proxy, 3)
	assert.Equal(t, "etc/environment", section.Proxy[0].Source)
	assert.Contains(t, section.Proxy[0].Line, "http_proxy")
}

func TestCollectProxyFromDirectory(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/profile.d": {"proxy.sh", "other.sh"},
		}).
		WithFiles(map[string]string{
			"/etc/profile.d/proxy.sh": "export HTTP_PROXY=http://proxy:3128\nexport NO_PROXY=localhost\n",
			"/etc/profile.d/other.sh": "# nothing proxy related\nexport EDITOR=vim\n",
		})
	section := &schema.NetworkSection{Proxy: []schema.ProxyEntry{}}
	collectProxy(exec, section)

	assert.Len(t, section.Proxy, 2)
	assert.Equal(t, "etc/profile.d/proxy.sh", section.Proxy[0].Source)
}

func TestCollectDNFProxy(t *testing.T) {
	dnfConf := loadNetworkFixture(t, "dnf-proxy.conf")
	exec := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/dnf/dnf.conf": dnfConf,
	})
	section := &schema.NetworkSection{Proxy: []schema.ProxyEntry{}}
	collectDNFProxy(exec, section)

	// Should capture proxy and proxy_username, not gpgcheck or installonly_limit.
	assert.Len(t, section.Proxy, 2)
	assert.Equal(t, "etc/dnf/dnf.conf", section.Proxy[0].Source)
	assert.Contains(t, section.Proxy[0].Line, "proxy=")
	assert.Contains(t, section.Proxy[1].Line, "proxy_username=")
}

// ---------------------------------------------------------------------------
// Static routes
// ---------------------------------------------------------------------------

func TestCollectStaticRoutes(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/sysconfig/network-scripts": {"route-eth0", "route-bond0", "ifcfg-eth0"},
		}).
		WithFiles(map[string]string{
			"/etc/sysconfig/network-scripts/route-eth0":  "10.0.0.0/8 via 10.0.0.1",
			"/etc/sysconfig/network-scripts/route-bond0": "172.16.0.0/12 via 10.0.0.1",
			"/etc/sysconfig/network-scripts/ifcfg-eth0":  "TYPE=Ethernet",
		})

	section := &schema.NetworkSection{StaticRoutes: []schema.StaticRouteFile{}}
	collectStaticRoutes(exec, section)

	// Should capture route-eth0 and route-bond0, not ifcfg-eth0.
	assert.Len(t, section.StaticRoutes, 2)
	assert.Equal(t, "route-bond0", section.StaticRoutes[0].Name)
	assert.Equal(t, "route-eth0", section.StaticRoutes[1].Name)
}

func TestCollectStaticRoutesIproute2(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/iproute2": {"rt_tables", "rt_protos"},
		}).
		WithFiles(map[string]string{
			"/etc/iproute2/rt_tables": "200 custom_table",
			"/etc/iproute2/rt_protos": "# defaults",
		})

	section := &schema.NetworkSection{StaticRoutes: []schema.StaticRouteFile{}}
	collectStaticRoutes(exec, section)

	assert.Len(t, section.StaticRoutes, 2)
	assert.Equal(t, "rt_protos", section.StaticRoutes[0].Name)
	assert.Equal(t, "rt_tables", section.StaticRoutes[1].Name)
}

// ---------------------------------------------------------------------------
// RunNetwork integration test
// ---------------------------------------------------------------------------

func TestRunNetworkIntegration(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"ip route": {
			Stdout:   "default via 10.0.0.1 dev eth0\n10.0.0.0/24 dev eth0 proto kernel\n",
			ExitCode: 0,
		},
		"ip rule": {
			Stdout:   "0:\tfrom all lookup local\n100:\tfrom 10.0.0.0/24 lookup custom\n32766:\tfrom all lookup main\n32767:\tfrom all lookup default\n",
			ExitCode: 0,
		},
	}).
		WithDirs(map[string][]string{
			"/etc/NetworkManager/system-connections": {"eth0.nmconnection"},
			"/etc/firewalld/zones":                  {"public.xml"},
		}).
		WithFiles(map[string]string{
			"/etc/NetworkManager/system-connections/eth0.nmconnection": "[connection]\ntype=ethernet\n[ipv4]\nmethod=auto\n",
			"/etc/firewalld/zones/public.xml":                         `<zone><service name="ssh"/><port port="443" protocol="tcp"/></zone>`,
			"/etc/resolv.conf":                                        "# Generated by NetworkManager\nnameserver 10.0.0.1\n",
			"/etc/hosts":                                              "127.0.0.1 localhost\n10.0.0.50 db.internal\n",
			"/etc/environment":                                        "http_proxy=http://proxy:8080\n",
		})

	section, warnings, err := RunNetwork(exec, NetworkOptions{})

	require.NoError(t, err)
	assert.Empty(t, warnings)

	// NM connections.
	require.Len(t, section.Connections, 1)
	assert.Equal(t, "eth0", section.Connections[0].Name)
	assert.Equal(t, "dhcp", section.Connections[0].Method)
	assert.Equal(t, "ethernet", section.Connections[0].Type)

	// Firewall zones.
	require.Len(t, section.FirewallZones, 1)
	assert.Equal(t, "public", section.FirewallZones[0].Name)
	assert.Equal(t, []string{"ssh"}, section.FirewallZones[0].Services)
	assert.Equal(t, []string{"443/tcp"}, section.FirewallZones[0].Ports)

	// DNS provenance.
	assert.Equal(t, "networkmanager", section.ResolvProvenance)

	// Hosts additions.
	require.Len(t, section.HostsAdditions, 1)
	assert.Contains(t, section.HostsAdditions[0], "db.internal")

	// IP routes.
	assert.Len(t, section.IPRoutes, 2)

	// IP rules — only custom, not local/main/default.
	require.Len(t, section.IPRules, 1)
	assert.Contains(t, section.IPRules[0], "custom")

	// Proxy.
	require.Len(t, section.Proxy, 1)
	assert.Contains(t, section.Proxy[0].Line, "http_proxy")
}

func TestRunNetworkEmptySystem(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"ip route": {ExitCode: 127, Stderr: "command not found"},
		"ip rule":  {ExitCode: 127, Stderr: "command not found"},
	})

	section, warnings, err := RunNetwork(exec, NetworkOptions{})

	require.NoError(t, err)
	// Two warnings for failed ip route and ip rule.
	assert.Len(t, warnings, 2)

	// All slices should be non-nil (empty, not null in JSON).
	assert.NotNil(t, section.Connections)
	assert.NotNil(t, section.FirewallZones)
	assert.NotNil(t, section.FirewallDirectRules)
	assert.NotNil(t, section.StaticRoutes)
	assert.NotNil(t, section.HostsAdditions)
	assert.NotNil(t, section.Proxy)
	assert.Empty(t, section.Connections)
	assert.Equal(t, "", section.ResolvProvenance)
}

func TestRunNetworkIPCommandsSucceedWithEmptyOutput(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"ip route": {Stdout: "", ExitCode: 0},
		"ip rule":  {Stdout: "", ExitCode: 0},
	})

	section, warnings, err := RunNetwork(exec, NetworkOptions{})
	require.NoError(t, err)
	assert.Empty(t, warnings)
	assert.Empty(t, section.IPRoutes)
	assert.Empty(t, section.IPRules)
}

// ---------------------------------------------------------------------------
// extractRichRules
// ---------------------------------------------------------------------------

func TestExtractRichRules(t *testing.T) {
	tests := []struct {
		name  string
		input string
		want  int
	}{
		{
			name:  "single rule",
			input: `<zone><rule family="ipv4"><accept/></rule></zone>`,
			want:  1,
		},
		{
			name:  "multiple rules",
			input: `<zone><rule family="ipv4"><accept/></rule><rule family="ipv6"><drop/></rule></zone>`,
			want:  2,
		},
		{
			name:  "no rules",
			input: `<zone><service name="ssh"/></zone>`,
			want:  0,
		},
		{
			name:  "malformed XML no close tag",
			input: `<zone><rule family="ipv4">`,
			want:  0,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			rules := extractRichRules(tt.input)
			assert.Len(t, rules, tt.want)
		})
	}
}

// ---------------------------------------------------------------------------
// isProxyLine
// ---------------------------------------------------------------------------

func TestIsProxyLine(t *testing.T) {
	tests := []struct {
		line string
		want bool
	}{
		{"http_proxy=http://proxy:8080", true},
		{"HTTPS_PROXY=http://proxy:8080", true},
		{"export no_proxy=localhost", true},
		{"FTP_PROXY=http://proxy:8080", true},
		{"PATH=/usr/local/bin", false},
		{"EDITOR=vim", false},
		{"# http_proxy comment", true}, // comment lines with proxy keywords still match
		{"", false},
	}
	for _, tt := range tests {
		t.Run(tt.line, func(t *testing.T) {
			assert.Equal(t, tt.want, isProxyLine(tt.line))
		})
	}
}

// ---------------------------------------------------------------------------
// NM connection collection with FakeExecutor
// ---------------------------------------------------------------------------

func TestCollectNMConnectionsMultipleDirs(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/NetworkManager/system-connections": {"eth0.nmconnection", ".hidden"},
			"/etc/sysconfig/network-scripts":         {"ifcfg-eth0"},
		}).
		WithFiles(map[string]string{
			"/etc/NetworkManager/system-connections/eth0.nmconnection": "[connection]\ntype=ethernet\n[ipv4]\nmethod=auto\n",
			"/etc/NetworkManager/system-connections/.hidden":           "[connection]\ntype=wifi\n",
			"/etc/sysconfig/network-scripts/ifcfg-eth0":               "TYPE=Ethernet\nBOOTPROTO=dhcp\n",
		})

	section := &schema.NetworkSection{Connections: []schema.NMConnection{}}
	collectNMConnections(exec, section)

	// Should skip .hidden, capture eth0.nmconnection and ifcfg-eth0.
	assert.Len(t, section.Connections, 2)
	assert.Equal(t, "eth0", section.Connections[0].Name)
	// ifcfg-eth0 won't parse as NM keyfile, so method stays "unknown".
	assert.Equal(t, "ifcfg-eth0", section.Connections[1].Name)
}

// ---------------------------------------------------------------------------
// indexOf helper
// ---------------------------------------------------------------------------

func TestIndexOf(t *testing.T) {
	assert.Equal(t, 2, indexOf([]string{"from", "all", "lookup", "main"}, "lookup"))
	assert.Equal(t, -1, indexOf([]string{"from", "all"}, "lookup"))
	assert.Equal(t, -1, indexOf([]string{}, "lookup"))
}
