// Network inspector: NM connections, firewall zones, routing, hosts,
// proxy settings, and DNS provenance detection.
//
// File-based scan under host root, plus "ip route" / "ip rule" via executor.
package inspector

import (
	"encoding/xml"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// NetworkOptions configures the network inspector.
type NetworkOptions struct {
	SystemType schema.SystemType
}

// RunNetwork runs the network inspector and returns the populated section,
// accumulated warnings, and any fatal error.
func RunNetwork(exec Executor, opts NetworkOptions) (*schema.NetworkSection, []Warning, error) {
	var warnings []Warning
	section := &schema.NetworkSection{
		Connections:         []schema.NMConnection{},
		FirewallZones:       []schema.FirewallZone{},
		FirewallDirectRules: []schema.FirewallDirectRule{},
		StaticRoutes:        []schema.StaticRouteFile{},
		IPRoutes:            []string{},
		IPRules:             []string{},
		HostsAdditions:      []string{},
		Proxy:               []schema.ProxyEntry{},
	}

	collectNMConnections(exec, section)
	collectFirewallZones(exec, section, &warnings)
	collectFirewallDirectRules(exec, section)
	section.ResolvProvenance = detectResolvProvenance(exec)
	collectHostsAdditions(exec, section)
	collectStaticRoutes(exec, section)
	collectIPRoutes(exec, section, &warnings)
	collectProxy(exec, section)
	collectDNFProxy(exec, section)

	return section, warnings, nil
}

// ---------------------------------------------------------------------------
// NM connection profiles
// ---------------------------------------------------------------------------

// classifyConnection extracts method and type from a NM keyfile connection
// profile (INI-style format).
func classifyConnection(text string) (method, connType string) {
	method = "unknown"
	section := ""
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "[") && strings.HasSuffix(line, "]") {
			section = line[1 : len(line)-1]
			continue
		}
		if !strings.Contains(line, "=") {
			continue
		}
		parts := strings.SplitN(line, "=", 2)
		key := strings.TrimSpace(parts[0])
		val := strings.TrimSpace(parts[1])

		if section == "ipv4" && key == "method" {
			switch val {
			case "manual":
				method = "static"
			case "auto":
				method = "dhcp"
			default:
				method = val
			}
		}
		if section == "connection" && key == "type" {
			connType = val
		}
	}
	return method, connType
}

// collectNMConnections scans NM system-connections and legacy network-scripts
// directories for connection profile files.
func collectNMConnections(exec Executor, section *schema.NetworkSection) {
	for _, subdir := range []string{
		"/etc/NetworkManager/system-connections",
		"/etc/sysconfig/network-scripts",
	} {
		if !exec.FileExists(subdir) {
			continue
		}
		entries, err := exec.ReadDir(subdir)
		if err != nil {
			continue
		}

		names := sortedEntryNames(entries)
		for _, name := range names {
			if strings.HasPrefix(name, ".") {
				continue
			}
			path := filepath.Join(subdir, name)
			text, err := exec.ReadFile(path)
			if err != nil {
				continue
			}
			method, connType := classifyConnection(text)
			// Strip leading "/" for relative path display.
			relPath := strings.TrimPrefix(path, "/")
			stem := strings.TrimSuffix(name, filepath.Ext(name))
			section.Connections = append(section.Connections, schema.NMConnection{
				Path:   relPath,
				Name:   stem,
				Method: method,
				Type:   connType,
			})
		}
	}
}

// ---------------------------------------------------------------------------
// Firewall zone parsing
// ---------------------------------------------------------------------------

// firewallZoneXML is the minimal XML structure for parsing firewalld zones.
type firewallZoneXML struct {
	XMLName  xml.Name            `xml:"zone"`
	Services []firewallServiceXML `xml:"service"`
	Ports    []firewallPortXML    `xml:"port"`
	Rules    []firewallRuleXML    `xml:"rule"`
}

type firewallServiceXML struct {
	Name string `xml:"name,attr"`
}

type firewallPortXML struct {
	Port     string `xml:"port,attr"`
	Protocol string `xml:"protocol,attr"`
}

type firewallRuleXML struct {
	InnerXML string `xml:",innerxml"`
}

// parseZoneXML parses a firewalld zone XML and extracts services, ports,
// and rich rules.
func parseZoneXML(text string) (services, ports, richRules []string) {
	var zone firewallZoneXML
	if err := xml.Unmarshal([]byte(text), &zone); err != nil {
		return nil, nil, nil
	}

	for _, svc := range zone.Services {
		if svc.Name != "" {
			services = append(services, svc.Name)
		}
	}
	for _, p := range zone.Ports {
		if p.Port == "" {
			continue
		}
		if p.Protocol != "" {
			ports = append(ports, p.Port+"/"+p.Protocol)
		} else {
			ports = append(ports, p.Port)
		}
	}
	// For rich rules, we re-extract via simple string scanning to match
	// the Python behavior of producing the full <rule>...</rule> text.
	richRules = extractRichRules(text)

	return services, ports, richRules
}

// extractRichRules does a simple extraction of <rule>...</rule> elements
// from raw XML text, matching the Python ET.tostring(rule_el) behavior.
func extractRichRules(text string) []string {
	var rules []string
	remaining := text
	for {
		start := strings.Index(remaining, "<rule")
		if start == -1 {
			break
		}
		end := strings.Index(remaining[start:], "</rule>")
		if end == -1 {
			break
		}
		end += start + len("</rule>")
		rule := strings.TrimSpace(remaining[start:end])
		rules = append(rules, rule)
		remaining = remaining[end:]
	}
	return rules
}

// collectFirewallZones reads firewalld zone XML files.
func collectFirewallZones(exec Executor, section *schema.NetworkSection, warnings *[]Warning) {
	zonesDir := "/etc/firewalld/zones"
	if !exec.FileExists(zonesDir) {
		return
	}

	entries, err := exec.ReadDir(zonesDir)
	if err != nil {
		*warnings = append(*warnings, makeWarning(
			"network",
			"Firewall zone directory unreadable — firewall configuration may be incomplete.",
		))
		return
	}

	names := sortedEntryNames(entries)
	for _, name := range names {
		if !strings.HasSuffix(name, ".xml") {
			continue
		}
		path := filepath.Join(zonesDir, name)
		content, err := exec.ReadFile(path)
		if err != nil {
			continue
		}
		services, ports, richRules := parseZoneXML(content)
		relPath := strings.TrimPrefix(path, "/")
		stem := strings.TrimSuffix(name, ".xml")
		section.FirewallZones = append(section.FirewallZones, schema.FirewallZone{
			Path:      relPath,
			Name:      stem,
			Content:   content,
			Services:  nonNilStrSlice(services),
			Ports:     nonNilStrSlice(ports),
			RichRules: nonNilStrSlice(richRules),
		})
	}
}

// ---------------------------------------------------------------------------
// Firewall direct rules
// ---------------------------------------------------------------------------

// firewallDirectXML is the minimal XML structure for firewalld direct.xml.
type firewallDirectXML struct {
	XMLName xml.Name              `xml:"direct"`
	Rules   []firewallDirectRuleXML `xml:"rule"`
}

type firewallDirectRuleXML struct {
	Ipv      string `xml:"ipv,attr"`
	Table    string `xml:"table,attr"`
	Chain    string `xml:"chain,attr"`
	Priority string `xml:"priority,attr"`
	Content  string `xml:",chardata"`
}

// collectFirewallDirectRules reads firewalld direct.xml.
func collectFirewallDirectRules(exec Executor, section *schema.NetworkSection) {
	directPath := "/etc/firewalld/direct.xml"
	if !exec.FileExists(directPath) {
		return
	}
	content, err := exec.ReadFile(directPath)
	if err != nil {
		return
	}
	var direct firewallDirectXML
	if err := xml.Unmarshal([]byte(content), &direct); err != nil {
		return
	}
	for _, r := range direct.Rules {
		prio := r.Priority
		if prio == "" {
			prio = "0"
		}
		section.FirewallDirectRules = append(section.FirewallDirectRules, schema.FirewallDirectRule{
			Ipv:      r.Ipv,
			Table:    r.Table,
			Chain:    r.Chain,
			Priority: prio,
			Args:     strings.TrimSpace(r.Content),
		})
	}
}

// ---------------------------------------------------------------------------
// resolv.conf provenance detection
// ---------------------------------------------------------------------------

// detectResolvProvenance determines who manages /etc/resolv.conf.
// Since our Executor abstraction doesn't expose symlink detection,
// we check the file content for known generator signatures.
func detectResolvProvenance(exec Executor) string {
	if !exec.FileExists("/etc/resolv.conf") {
		return ""
	}
	content, err := exec.ReadFile("/etc/resolv.conf")
	if err != nil {
		return ""
	}

	// Check for systemd-resolved signature (comment or stub resolver IP).
	for _, line := range strings.Split(content, "\n") {
		lower := strings.ToLower(line)
		if strings.Contains(lower, "systemd-resolve") || strings.Contains(lower, "resolved") {
			return "systemd-resolved"
		}
	}
	// Check for NetworkManager signature.
	for _, line := range strings.Split(content, "\n") {
		if strings.Contains(line, "Generated by NetworkManager") {
			return "networkmanager"
		}
	}
	return "hand-edited"
}

// ---------------------------------------------------------------------------
// /etc/hosts additions
// ---------------------------------------------------------------------------

// collectHostsAdditions reads /etc/hosts and captures non-default entries.
func collectHostsAdditions(exec Executor, section *schema.NetworkSection) {
	content, err := exec.ReadFile("/etc/hosts")
	if err != nil {
		return
	}
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		if strings.Contains(strings.ToLower(line), "localhost") {
			continue
		}
		section.HostsAdditions = append(section.HostsAdditions, line)
	}
}

// ---------------------------------------------------------------------------
// Static route files
// ---------------------------------------------------------------------------

// collectStaticRoutes scans for legacy route-* files and iproute2 configs.
func collectStaticRoutes(exec Executor, section *schema.NetworkSection) {
	// Legacy network-scripts route files.
	nsDir := "/etc/sysconfig/network-scripts"
	if exec.FileExists(nsDir) {
		entries, err := exec.ReadDir(nsDir)
		if err == nil {
			names := sortedEntryNames(entries)
			for _, name := range names {
				if strings.HasPrefix(name, "route-") {
					path := filepath.Join(nsDir, name)
					relPath := strings.TrimPrefix(path, "/")
					section.StaticRoutes = append(section.StaticRoutes, schema.StaticRouteFile{
						Path: relPath,
						Name: name,
					})
				}
			}
		}
	}

	// iproute2 config directory.
	ipDir := "/etc/iproute2"
	if exec.FileExists(ipDir) {
		entries, err := exec.ReadDir(ipDir)
		if err == nil {
			names := sortedEntryNames(entries)
			for _, name := range names {
				path := filepath.Join(ipDir, name)
				relPath := strings.TrimPrefix(path, "/")
				section.StaticRoutes = append(section.StaticRoutes, schema.StaticRouteFile{
					Path: relPath,
					Name: name,
				})
			}
		}
	}
}

// ---------------------------------------------------------------------------
// ip route / ip rule via executor
// ---------------------------------------------------------------------------

// defaultRuleTables are the standard policy routing tables that are always
// present and filtered from ip rule output.
var defaultRuleTables = map[string]bool{
	"local":   true,
	"main":    true,
	"default": true,
}

// parseIPRoutes splits ip route output into non-empty lines.
func parseIPRoutes(text string) []string {
	var routes []string
	for _, ln := range strings.Split(text, "\n") {
		ln = strings.TrimSpace(ln)
		if ln != "" {
			routes = append(routes, ln)
		}
	}
	return routes
}

// parseIPRules parses ip rule output, filtering out default table rules.
func parseIPRules(text string) []string {
	var rules []string
	for _, ln := range strings.Split(text, "\n") {
		ln = strings.TrimSpace(ln)
		if ln == "" {
			continue
		}
		parts := strings.Fields(ln)
		if idx := indexOf(parts, "lookup"); idx >= 0 && idx+1 < len(parts) {
			if defaultRuleTables[parts[idx+1]] {
				continue
			}
		}
		rules = append(rules, ln)
	}
	return rules
}

// collectIPRoutes runs ip route and ip rule, populating the section.
func collectIPRoutes(exec Executor, section *schema.NetworkSection, warnings *[]Warning) {
	// ip route
	result := exec.Run("ip", "route")
	if result.ExitCode == 0 && strings.TrimSpace(result.Stdout) != "" {
		section.IPRoutes = parseIPRoutes(result.Stdout)
	} else if result.ExitCode != 0 {
		*warnings = append(*warnings, makeWarning(
			"network",
			"ip route failed — static route information unavailable.",
		))
	}

	// ip rule
	result = exec.Run("ip", "rule")
	if result.ExitCode == 0 && strings.TrimSpace(result.Stdout) != "" {
		section.IPRules = parseIPRules(result.Stdout)
	} else if result.ExitCode != 0 {
		*warnings = append(*warnings, makeWarning(
			"network",
			"ip rule failed — policy routing rule information unavailable.",
		))
	}
}

// ---------------------------------------------------------------------------
// Proxy settings
// ---------------------------------------------------------------------------

// proxyKeywords are the environment variable names that indicate proxy config.
var proxyKeywords = []string{"http_proxy", "https_proxy", "no_proxy", "ftp_proxy"}

// isProxyLine checks if a line contains any proxy-related variable.
func isProxyLine(line string) bool {
	lower := strings.ToLower(line)
	for _, kw := range proxyKeywords {
		if strings.Contains(lower, kw) {
			return true
		}
	}
	return false
}

// collectProxy scans environment files and profile.d for proxy configuration.
func collectProxy(exec Executor, section *schema.NetworkSection) {
	for _, proxyPath := range []string{"/etc/environment", "/etc/profile.d"} {
		// Check if it's a file first (ReadFile succeeds), then try as directory.
		content, err := exec.ReadFile(proxyPath)
		if err == nil {
			// It's a file — scan lines.
			for _, line := range strings.Split(content, "\n") {
				if isProxyLine(line) {
					relPath := strings.TrimPrefix(proxyPath, "/")
					section.Proxy = append(section.Proxy, schema.ProxyEntry{
						Source: relPath,
						Line:   strings.TrimSpace(line),
					})
				}
			}
			continue
		}
		// Try as directory.
		entries, dirErr := exec.ReadDir(proxyPath)
		if dirErr != nil {
			continue
		}
		names := sortedEntryNames(entries)
		for _, name := range names {
			filePath := filepath.Join(proxyPath, name)
			fileContent, fileErr := exec.ReadFile(filePath)
			if fileErr != nil {
				continue
			}
			for _, line := range strings.Split(fileContent, "\n") {
				if isProxyLine(line) {
					relPath := strings.TrimPrefix(filePath, "/")
					section.Proxy = append(section.Proxy, schema.ProxyEntry{
						Source: relPath,
						Line:   strings.TrimSpace(line),
					})
				}
			}
		}
	}
}

// ---------------------------------------------------------------------------
// DNF/Yum proxy config
// ---------------------------------------------------------------------------

// dnfProxyKeys are the proxy-related keys in dnf.conf/yum.conf.
var dnfProxyKeys = map[string]bool{
	"proxy":             true,
	"proxy_username":    true,
	"proxy_password":    true,
	"proxy_auth_method": true,
}

// collectDNFProxy scans dnf.conf and yum.conf for proxy settings.
func collectDNFProxy(exec Executor, section *schema.NetworkSection) {
	for _, confPath := range []string{"/etc/dnf/dnf.conf", "/etc/yum.conf"} {
		content, err := exec.ReadFile(confPath)
		if err != nil {
			continue
		}
		relPath := strings.TrimPrefix(confPath, "/")
		for _, line := range strings.Split(content, "\n") {
			stripped := strings.TrimSpace(line)
			if strings.HasPrefix(stripped, "#") || !strings.Contains(stripped, "=") {
				continue
			}
			key := strings.TrimSpace(strings.SplitN(stripped, "=", 2)[0])
			if dnfProxyKeys[strings.ToLower(key)] {
				section.Proxy = append(section.Proxy, schema.ProxyEntry{
					Source: relPath,
					Line:   stripped,
				})
			}
		}
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// sortedEntryNames extracts and sorts directory entry names.
func sortedEntryNames(entries []os.DirEntry) []string {
	names := make([]string, 0, len(entries))
	for _, e := range entries {
		if !e.IsDir() {
			names = append(names, e.Name())
		}
	}
	sort.Strings(names)
	return names
}

// nonNilStrSlice ensures a nil slice is returned as an empty slice for
// consistent JSON serialisation.
func nonNilStrSlice(s []string) []string {
	if s == nil {
		return []string{}
	}
	return s
}

// indexOf returns the index of target in parts, or -1 if not found.
func indexOf(parts []string, target string) int {
	for i, p := range parts {
		if p == target {
			return i
		}
	}
	return -1
}
