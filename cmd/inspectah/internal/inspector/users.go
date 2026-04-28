// Users/Groups inspector: non-system users and groups, sudoers, SSH key refs.
// Parses passwd/group/shadow/gshadow/subuid/subgid under host_root.
package inspector

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const (
	// UID constants are in container.go (nonSystemUIDMin, nonSystemUIDMax).
	nonSystemGIDMin = 1000
	nonSystemGIDMax = 60000 // exclusive
)

// nologinShells are shells that indicate a user cannot log in interactively.
var nologinShells = map[string]bool{
	"/sbin/nologin":     true,
	"/bin/false":        true,
	"/usr/sbin/nologin": true,
}

// realShells are interactive login shells.
var realShells = map[string]bool{
	"/bin/bash":     true,
	"/bin/zsh":      true,
	"/bin/sh":       true,
	"/bin/fish":     true,
	"/bin/tcsh":     true,
	"/bin/csh":      true,
	"/usr/bin/bash": true,
	"/usr/bin/zsh":  true,
	"/usr/bin/fish": true,
}

// strategyMap maps user classification to migration strategy.
var strategyMap = map[string]string{
	"service":   "sysusers",
	"human":     "kickstart",
	"ambiguous": "useradd",
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// UserGroupOptions configures the Users/Groups inspector.
type UserGroupOptions struct {
	// UserStrategyOverride forces a specific strategy for all users/groups
	// instead of using classification-based defaults.
	UserStrategyOverride string
}

// RunUsersGroups runs the Users/Groups inspector and returns the populated
// section, accumulated warnings, and any fatal error.
func RunUsersGroups(exec Executor, opts UserGroupOptions) (*schema.UserGroupSection, []Warning, error) {
	var warnings []Warning
	section := &schema.UserGroupSection{
		Users:                 []map[string]interface{}{},
		Groups:                []map[string]interface{}{},
		SudoersRules:          []string{},
		SSHAuthorizedKeysRefs: []map[string]interface{}{},
		PasswdEntries:         []string{},
		ShadowEntries:         []string{},
		GroupEntries:          []string{},
		GshadowEntries:        []string{},
		SubuidEntries:         []string{},
		SubgidEntries:         []string{},
	}

	// -----------------------------------------------------------------------
	// /etc/passwd — non-system users (UID >= 1000 and < 60000)
	// -----------------------------------------------------------------------
	nonSystemUsers := map[string]bool{}
	passwdText, err := exec.ReadFile("/etc/passwd")
	if err == nil {
		parsePasswd(passwdText, section, nonSystemUsers)
	}

	// Classify each user and assign migration strategy.
	for _, u := range section.Users {
		u["classification"] = classifyUser(u)
		if opts.UserStrategyOverride != "" {
			u["strategy"] = opts.UserStrategyOverride
		} else {
			cls, _ := u["classification"].(string)
			u["strategy"] = strategyMap[cls]
		}
	}

	// -----------------------------------------------------------------------
	// /etc/shadow — match by username from passwd
	// -----------------------------------------------------------------------
	shadowText, err := exec.ReadFile("/etc/shadow")
	if err == nil {
		parseShadow(shadowText, section, nonSystemUsers)
	}

	// -----------------------------------------------------------------------
	// /etc/group — non-system groups (GID >= 1000 and < 60000)
	// -----------------------------------------------------------------------
	nonSystemGroups := map[string]bool{}
	groupText, err := exec.ReadFile("/etc/group")
	if err == nil {
		parseGroup(groupText, section, nonSystemGroups)
	}

	// Assign strategy to groups: override, follow primary user, or default.
	assignGroupStrategies(section, opts.UserStrategyOverride)

	// -----------------------------------------------------------------------
	// /etc/gshadow — match by group name
	// -----------------------------------------------------------------------
	gshadowText, err := exec.ReadFile("/etc/gshadow")
	if err == nil {
		parseGshadow(gshadowText, section, nonSystemGroups)
	}

	// -----------------------------------------------------------------------
	// /etc/subuid and /etc/subgid
	// -----------------------------------------------------------------------
	parseSubIDFile(exec, "/etc/subuid", &section.SubuidEntries, nonSystemUsers)
	parseSubIDFile(exec, "/etc/subgid", &section.SubgidEntries, nonSystemUsers)

	// -----------------------------------------------------------------------
	// /etc/sudoers and /etc/sudoers.d/*
	// -----------------------------------------------------------------------
	parseSudoers(exec, section)

	// -----------------------------------------------------------------------
	// SSH authorized_keys per user
	// -----------------------------------------------------------------------
	collectSSHKeys(exec, section)

	return section, warnings, nil
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

// parsePasswd extracts non-system users from /etc/passwd content.
func parsePasswd(text string, section *schema.UserGroupSection, nonSystemUsers map[string]bool) {
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		parts := strings.Split(line, ":")
		if len(parts) < 7 {
			continue
		}
		uid, err := strconv.Atoi(parts[2])
		if err != nil {
			continue
		}
		if uid < nonSystemUIDMin || uid >= nonSystemUIDMax {
			continue
		}
		username := parts[0]
		nonSystemUsers[username] = true

		var gid interface{}
		if g, err := strconv.Atoi(parts[3]); err == nil {
			gid = g
		}

		section.Users = append(section.Users, map[string]interface{}{
			"name":    username,
			"uid":     uid,
			"gid":     gid,
			"shell":   parts[6],
			"home":    parts[5],
			"include": true,
		})
		section.PasswdEntries = append(section.PasswdEntries, line)
	}
}

// parseShadow extracts shadow entries for non-system users.
func parseShadow(text string, section *schema.UserGroupSection, nonSystemUsers map[string]bool) {
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		username := strings.SplitN(line, ":", 2)[0]
		if nonSystemUsers[username] {
			section.ShadowEntries = append(section.ShadowEntries, line)
		}
	}
}

// parseGroup extracts non-system groups from /etc/group content.
func parseGroup(text string, section *schema.UserGroupSection, nonSystemGroups map[string]bool) {
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		parts := strings.Split(line, ":")
		if len(parts) < 3 {
			continue
		}
		gid, err := strconv.Atoi(parts[2])
		if err != nil {
			continue
		}
		if gid < nonSystemGIDMin || gid >= nonSystemGIDMax {
			continue
		}
		groupName := parts[0]
		nonSystemGroups[groupName] = true

		var members []interface{}
		if len(parts) > 3 && parts[3] != "" {
			for _, m := range strings.Split(parts[3], ",") {
				members = append(members, m)
			}
		}
		if members == nil {
			members = []interface{}{}
		}

		section.Groups = append(section.Groups, map[string]interface{}{
			"name":    groupName,
			"gid":     gid,
			"members": members,
			"include": true,
		})
		section.GroupEntries = append(section.GroupEntries, line)
	}
}

// parseGshadow extracts gshadow entries for non-system groups.
func parseGshadow(text string, section *schema.UserGroupSection, nonSystemGroups map[string]bool) {
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		groupName := strings.SplitN(line, ":", 2)[0]
		if nonSystemGroups[groupName] {
			section.GshadowEntries = append(section.GshadowEntries, line)
		}
	}
}

// parseSubIDFile reads a subuid or subgid file and appends matching entries.
func parseSubIDFile(exec Executor, path string, entries *[]string, nonSystemUsers map[string]bool) {
	text, err := exec.ReadFile(path)
	if err != nil {
		return
	}
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		username := strings.SplitN(line, ":", 2)[0]
		if nonSystemUsers[username] {
			*entries = append(*entries, line)
		}
	}
}

// parseSudoers reads /etc/sudoers and /etc/sudoers.d/* for sudo rules.
func parseSudoers(exec Executor, section *schema.UserGroupSection) {
	// Main sudoers file
	if text, err := exec.ReadFile("/etc/sudoers"); err == nil {
		extractSudoersRules(text, section)
	}

	// Drop-in directory
	entries, err := exec.ReadDir("/etc/sudoers.d")
	if err != nil {
		return
	}
	for _, entry := range entries {
		if entry.IsDir() || strings.HasPrefix(entry.Name(), ".") {
			continue
		}
		path := fmt.Sprintf("/etc/sudoers.d/%s", entry.Name())
		if text, err := exec.ReadFile(path); err == nil {
			extractSudoersRules(text, section)
		}
	}
}

// extractSudoersRules parses sudoers content for non-comment, non-Defaults rules.
func extractSudoersRules(text string, section *schema.UserGroupSection) {
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") || strings.HasPrefix(line, "Defaults") {
			continue
		}
		section.SudoersRules = append(section.SudoersRules, line)
	}
}

// collectSSHKeys checks for ~/.ssh/authorized_keys for each user.
func collectSSHKeys(exec Executor, section *schema.UserGroupSection) {
	for _, u := range section.Users {
		home, _ := u["home"].(string)
		if home == "" {
			continue
		}
		authKeysPath := fmt.Sprintf("%s/.ssh/authorized_keys", home)
		if exec.FileExists(authKeysPath) {
			section.SSHAuthorizedKeysRefs = append(section.SSHAuthorizedKeysRefs, map[string]interface{}{
				"user": u["name"],
				"path": authKeysPath,
			})
		}
	}
}

// ---------------------------------------------------------------------------
// User classification
// ---------------------------------------------------------------------------

// classifyUser categorizes a user as "service", "human", or "ambiguous"
// based on shell, home directory, and UID.
func classifyUser(user map[string]interface{}) string {
	shell, _ := user["shell"].(string)
	home, _ := user["home"].(string)
	uid, _ := user["uid"].(int)

	if nologinShells[shell] {
		return "service"
	}
	if home == "/dev/null" || home == "" {
		return "service"
	}
	if strings.HasPrefix(home, "/var/") || strings.HasPrefix(home, "/opt/") || strings.HasPrefix(home, "/srv/") {
		if realShells[shell] {
			return "ambiguous"
		}
		return "service"
	}
	if realShells[shell] && strings.HasPrefix(home, "/home/") && uid >= 1000 {
		return "human"
	}
	return "ambiguous"
}

// assignGroupStrategies sets the migration strategy on each group.
// Groups follow their primary user's strategy when possible.
func assignGroupStrategies(section *schema.UserGroupSection, override string) {
	// Build first-match map: GID -> user entry.
	userByGID := map[int]map[string]interface{}{}
	for _, u := range section.Users {
		gid, ok := u["gid"].(int)
		if !ok {
			continue
		}
		if _, exists := userByGID[gid]; !exists {
			userByGID[gid] = u
		}
	}

	for _, g := range section.Groups {
		if override != "" {
			g["strategy"] = override
			continue
		}
		gid, _ := g["gid"].(int)
		if primaryUser, ok := userByGID[gid]; ok {
			if s, ok := primaryUser["strategy"].(string); ok {
				g["strategy"] = s
			} else {
				g["strategy"] = "sysusers"
			}
		} else {
			g["strategy"] = "sysusers"
		}
	}
}
