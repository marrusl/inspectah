package renderer

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// RenderKickstart produces kickstart-suggestion.ks for deployment-time
// settings that should not be baked into the image.
func RenderKickstart(snap *schema.InspectionSnapshot, outputDir string) error {
	var lines []string

	lines = append(lines, "# Kickstart suggestion -- review and adapt for your environment")
	lines = append(lines, "# These settings belong at deploy time, not baked into the image.")
	lines = append(lines, "")

	// Network
	if snap.Network != nil {
		var dhcpConns, staticConns []schema.NMConnection
		for _, c := range snap.Network.Connections {
			if c.Method == "auto" || c.Method == "dhcp" {
				dhcpConns = append(dhcpConns, c)
			} else if c.Method == "manual" {
				staticConns = append(staticConns, c)
			}
		}

		if len(dhcpConns) > 0 {
			lines = append(lines, "# --- DHCP connections ---")
			for _, c := range dhcpConns {
				name := c.Name
				if name == "" {
					name = "eth0"
				}
				lines = append(lines, fmt.Sprintf("network --bootproto=dhcp --device=%s --activate", name))
			}
			lines = append(lines, "")
		}

		if len(staticConns) > 0 {
			lines = append(lines, "# --- Static connections ---")
			lines = append(lines, "# FIXME: fill in IP, netmask, gateway for each static connection")
			for _, c := range staticConns {
				lines = append(lines, fmt.Sprintf("# network --bootproto=static --device=%s --ip=FIXME --netmask=FIXME --gateway=FIXME", c.Name))
			}
			lines = append(lines, "")
		}

		// Hostname
		hostname := ""
		if snap.Meta != nil {
			if h, ok := snap.Meta["hostname"].(string); ok {
				hostname = h
			}
		}
		if hostname != "" {
			lines = append(lines, fmt.Sprintf("network --hostname=%s", hostname))
			lines = append(lines, "")
		}

		// /etc/hosts additions
		if len(snap.Network.HostsAdditions) > 0 {
			lines = append(lines, "# --- /etc/hosts additions ---")
			lines = append(lines, "%post")
			for _, h := range snap.Network.HostsAdditions {
				lines = append(lines, fmt.Sprintf(`echo "%s" >> /etc/hosts`, h))
			}
			lines = append(lines, "%end")
			lines = append(lines, "")
		}

		// Static routes
		if len(snap.Network.StaticRoutes) > 0 {
			lines = append(lines, "# --- Static route files detected ---")
			lines = append(lines, "# These files were present on the source host. Review each and translate")
			lines = append(lines, "# to NM connection properties (+ipv4.routes) or kickstart route directives.")
			for _, r := range snap.Network.StaticRoutes {
				lines = append(lines, fmt.Sprintf("# FIXME: review %s and add equivalent route to NM connection or kickstart", r.Path))
			}
			lines = append(lines, "")
		}

		// IP policy rules
		var policyRules []string
		for _, r := range snap.Network.IPRules {
			if strings.TrimSpace(r) != "" {
				policyRules = append(policyRules, r)
			}
		}
		if len(policyRules) > 0 {
			lines = append(lines, "# --- Policy routing rules detected ---")
			limit := len(policyRules)
			if limit > 10 {
				limit = 10
			}
			for _, r := range policyRules[:limit] {
				lines = append(lines, fmt.Sprintf("# ip rule: %s", r))
			}
			lines = append(lines, "# FIXME: translate ip rules to NM connection properties or dispatcher scripts")
			lines = append(lines, "")
		}
	}

	// Users deferred to kickstart
	if snap.UsersGroups != nil {
		var ksUsers []map[string]interface{}
		for _, u := range snap.UsersGroups.Users {
			strategy, _ := u["strategy"].(string)
			include := true
			if inc, ok := u["include"].(bool); ok {
				include = inc
			}
			if strategy == "kickstart" && include {
				ksUsers = append(ksUsers, u)
			}
		}

		if len(ksUsers) > 0 {
			lines = append(lines, "# --- Human users (deploy-time provisioning) ---")
			for _, u := range ksUsers {
				uname, _ := u["name"].(string)
				uid, _ := u["uid"].(float64)
				shell, _ := u["shell"].(string)
				home, _ := u["home"].(string)
				gid, _ := u["gid"].(float64)

				opts := fmt.Sprintf("--name=%s", uname)
				if uid > 0 {
					opts += fmt.Sprintf(" --uid=%d", int(uid))
				}
				if gid > 0 {
					opts += fmt.Sprintf(" --gid=%d", int(gid))
				}
				if shell != "" {
					opts += fmt.Sprintf(" --shell=%s", shell)
				}
				if home != "" {
					opts += fmt.Sprintf(" --homedir=%s", home)
				}
				lines = append(lines, fmt.Sprintf("user %s", opts))
			}
			lines = append(lines, "# Set passwords interactively or via --password/--iscrypted")
			lines = append(lines, "")
		}
	}

	lines = append(lines, "# --- Examples ---")
	lines = append(lines, "# network --bootproto=dhcp --device=eth0")
	lines = append(lines, "# network --hostname=myhost.example.com")
	lines = append(lines, "# network --bootproto=static --ip=192.168.1.10 --netmask=255.255.255.0 --gateway=192.168.1.1")
	lines = append(lines, "")

	// Storage: remote mounts
	if snap.Storage != nil {
		var nfsMounts, cifsMounts []schema.FstabEntry
		for _, e := range snap.Storage.FstabEntries {
			lower := strings.ToLower(e.Fstype)
			if strings.Contains(lower, "nfs") {
				nfsMounts = append(nfsMounts, e)
			} else if strings.Contains(lower, "cifs") {
				cifsMounts = append(cifsMounts, e)
			}
		}
		if len(nfsMounts) > 0 || len(cifsMounts) > 0 {
			lines = append(lines, "# --- Remote filesystem mounts detected ---")
			for _, m := range nfsMounts {
				lines = append(lines, fmt.Sprintf("# NFS: %s -> %s", m.Device, m.MountPoint))
				lines = append(lines, "#   FIXME: provide NFS credentials at deploy time")
			}
			for _, m := range cifsMounts {
				lines = append(lines, fmt.Sprintf("# CIFS: %s -> %s", m.Device, m.MountPoint))
				lines = append(lines, "#   FIXME: provide CIFS credentials (username/password) at deploy time")
			}
			lines = append(lines, "")
		}
	}

	content := strings.Join(lines, "\n")
	return os.WriteFile(filepath.Join(outputDir, "kickstart-suggestion.ks"), []byte(content), 0644)
}
