// Package inspector — Container workloads inspector.
//
// Scans for Podman Quadlet unit files, docker-compose/compose YAML files,
// running containers (via podman ps + podman inspect), and installed
// Flatpak applications.
package inspector

import (
	"encoding/json"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// quadletExtensions lists file extensions recognised as Quadlet unit files.
var quadletExtensions = map[string]bool{
	".container": true,
	".volume":    true,
	".network":   true,
	".kube":      true,
	".pod":       true,
	".image":     true,
	".build":     true,
}

// composePatterns are glob-style base-name patterns for compose files.
var composePatterns = []string{
	"docker-compose*.yml",
	"docker-compose*.yaml",
	"compose*.yml",
	"compose*.yaml",
}

// composeSearchDirs are top-level directories to scan for compose files.
var composeSearchDirs = []string{"opt", "srv", "etc"}

// pruneMarkers and skipDirNames are defined in config.go and shared
// across inspectors for dev-artifact filtering.

const (
	nonSystemUIDMin = 1000
	nonSystemUIDMax = 60000
)

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

// ContainerOptions controls the Container inspector behaviour.
type ContainerOptions struct {
	QueryPodman bool
	SystemType  schema.SystemType
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

// RunContainers inspects container workloads on the host: Quadlet units,
// compose files, optionally running containers, and Flatpak applications.
func RunContainers(exec Executor, opts ContainerOptions) (*schema.ContainerSection, []Warning, error) {
	section := &schema.ContainerSection{}
	var warnings []Warning

	// --- Quadlet units ---
	quadletDirs := []string{
		"/etc/containers/systemd",
		"/usr/share/containers/systemd",
		"/etc/systemd/system",
	}

	// User-level quadlets: ~/.config/containers/systemd/ for each real user.
	quadletDirs = append(quadletDirs, userQuadletDirs(exec)...)

	for _, dir := range quadletDirs {
		units := scanQuadletDir(exec, dir)
		section.QuadletUnits = append(section.QuadletUnits, units...)
	}

	// --- Compose files ---
	for _, dir := range composeSearchDirs {
		absDir := "/" + dir
		if !exec.FileExists(absDir) {
			continue
		}
		files := findComposeFiles(exec, absDir)
		section.ComposeFiles = append(section.ComposeFiles, files...)
	}

	// --- Running containers (podman) ---
	if opts.QueryPodman {
		containers, w := queryPodmanContainers(exec)
		section.RunningContainers = containers
		warnings = append(warnings, w...)
	}

	// --- Flatpak apps ---
	section.FlatpakApps = detectFlatpakApps(exec)

	return section, warnings, nil
}

// ---------------------------------------------------------------------------
// Quadlet scanning
// ---------------------------------------------------------------------------

// scanQuadletDir reads all quadlet unit files from a single directory.
func scanQuadletDir(exec Executor, dir string) []schema.QuadletUnit {
	entries, err := exec.ReadDir(dir)
	if err != nil {
		return nil
	}

	var units []schema.QuadletUnit
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		ext := filepath.Ext(entry.Name())
		if !quadletExtensions[ext] {
			continue
		}

		path := filepath.Join(dir, entry.Name())
		content, err := exec.ReadFile(path)
		if err != nil {
			continue
		}

		imageRef := ""
		if ext == ".container" {
			imageRef = extractQuadletImage(content)
		}

		// Store path relative to host root.
		relPath := strings.TrimPrefix(path, exec.HostRoot())
		relPath = strings.TrimPrefix(relPath, "/")

		units = append(units, schema.QuadletUnit{
			Path:    relPath,
			Name:    entry.Name(),
			Content: content,
			Image:   imageRef,
		})
	}
	return units
}

// extractQuadletImage parses Image= from a .container quadlet file.
func extractQuadletImage(content string) string {
	for _, line := range strings.Split(content, "\n") {
		trimmed := strings.TrimSpace(line)
		lower := strings.ToLower(trimmed)
		if !strings.HasPrefix(lower, "image") || !strings.Contains(trimmed, "=") {
			continue
		}
		key, val, ok := strings.Cut(trimmed, "=")
		if !ok {
			continue
		}
		if strings.ToLower(strings.TrimSpace(key)) == "image" {
			return strings.TrimSpace(val)
		}
	}
	return ""
}

// userQuadletDirs discovers per-user quadlet directories by parsing
// /etc/passwd for non-system UIDs (1000–59999).
func userQuadletDirs(exec Executor) []string {
	passwd, err := exec.ReadFile("/etc/passwd")
	if err != nil {
		return nil
	}

	var dirs []string
	for _, line := range strings.Split(passwd, "\n") {
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
		home := strings.TrimPrefix(parts[5], "/")
		dirs = append(dirs, "/"+home+"/.config/containers/systemd")
	}
	return dirs
}

// ---------------------------------------------------------------------------
// Compose file discovery
// ---------------------------------------------------------------------------

// findComposeFiles recursively searches a directory for compose files,
// pruning VCS checkouts and dev-artifact directories.
func findComposeFiles(exec Executor, root string) []schema.ComposeFile {
	var matches []string
	filteredWalk(exec, root, func(path string, name string) {
		for _, pattern := range composePatterns {
			if matchGlob(pattern, name) {
				matches = append(matches, path)
			}
		}
	})
	sort.Strings(matches)

	var files []schema.ComposeFile
	for _, path := range matches {
		content, err := exec.ReadFile(path)
		if err != nil {
			continue
		}
		images := extractComposeImages(content)

		relPath := strings.TrimPrefix(path, exec.HostRoot())
		relPath = strings.TrimPrefix(relPath, "/")

		files = append(files, schema.ComposeFile{
			Path:   relPath,
			Images: images,
		})
	}
	return files
}

// filteredWalk performs a recursive directory traversal that prunes VCS
// checkouts and dev-artifact directories. For each regular file visited,
// it calls fn(fullPath, baseName).
func filteredWalk(exec Executor, dir string, fn func(path, name string)) {
	entries, err := exec.ReadDir(dir)
	if err != nil {
		return
	}

	// Check for prune markers among children.
	childNames := make(map[string]bool, len(entries))
	for _, e := range entries {
		childNames[e.Name()] = true
	}
	for marker := range pruneMarkers {
		if childNames[marker] {
			return // VCS root — skip entire subtree
		}
	}

	for _, entry := range entries {
		name := entry.Name()
		fullPath := filepath.Join(dir, name)

		if entry.IsDir() {
			if !skipDirNames[name] {
				filteredWalk(exec, fullPath, fn)
			}
			continue
		}
		fn(fullPath, name)
	}
}

// matchGlob is defined in services.go and shared across inspectors.

// extractComposeImages parses image: fields from a compose YAML without
// requiring a YAML library. Detects service-level indent dynamically so
// 2-space, 4-space, and tab-indented files all work.
func extractComposeImages(content string) []schema.ComposeService {
	var results []schema.ComposeService
	lines := strings.Split(content, "\n")

	inServices := false
	currentService := ""
	serviceIndent := -1 // indent of first service key seen

	imageRe := regexp.MustCompile(`^image:\s*(.+)`)

	for _, line := range lines {
		trimmed := strings.TrimSpace(line)
		if trimmed == "" || strings.HasPrefix(trimmed, "#") {
			continue
		}

		indent := len(line) - len(strings.TrimLeft(line, " \t"))

		// Detect "services:" block.
		if trimmed == "services:" || strings.HasPrefix(trimmed, "services:") {
			inServices = true
			serviceIndent = -1
			currentService = ""
			continue
		}

		if !inServices {
			continue
		}

		// Top-level key other than services — stop.
		if indent == 0 && !strings.HasPrefix(trimmed, "#") {
			inServices = false
			currentService = ""
			serviceIndent = -1
			continue
		}

		// Calibrate service indent from the first indented key.
		if serviceIndent < 0 && indent > 0 {
			serviceIndent = indent
		}

		// Service-level key (e.g. "web:", "db:").
		if serviceIndent > 0 && indent == serviceIndent && strings.HasSuffix(trimmed, ":") {
			currentService = strings.TrimSuffix(trimmed, ":")
			continue
		}

		// Inside a service — look for image:.
		if currentService != "" && indent > serviceIndent {
			m := imageRe.FindStringSubmatch(trimmed)
			if m != nil {
				img := strings.Trim(m[1], "'\"")
				results = append(results, schema.ComposeService{
					Service: currentService,
					Image:   img,
				})
			}
		}
	}
	return results
}

// ---------------------------------------------------------------------------
// Podman container query
// ---------------------------------------------------------------------------

// queryPodmanContainers runs podman ps + podman inspect and returns parsed
// container data, plus any warnings.
func queryPodmanContainers(exec Executor) ([]schema.RunningContainer, []Warning) {
	var warnings []Warning

	result := exec.Run("podman", "ps", "-a", "--format", "json")
	if result.ExitCode != 0 {
		warnings = append(warnings, makeWarning(
			"containers",
			"--query-podman requested but podman ps failed -- live container data unavailable.",
		))
		return nil, warnings
	}

	stdout := strings.TrimSpace(result.Stdout)
	if stdout == "" {
		return nil, warnings
	}

	var psData []map[string]interface{}
	if err := json.Unmarshal([]byte(stdout), &psData); err != nil {
		return nil, warnings
	}

	// Collect container IDs for podman inspect.
	var ids []string
	for _, c := range psData {
		if id, ok := c["ID"].(string); ok && id != "" {
			ids = append(ids, id)
		}
	}

	// Try podman inspect for rich data.
	if len(ids) > 0 {
		args := append([]string{"inspect"}, ids...)
		ir := exec.Run("podman", args...)
		if ir.ExitCode == 0 && strings.TrimSpace(ir.Stdout) != "" {
			var inspectData []map[string]interface{}
			if err := json.Unmarshal([]byte(ir.Stdout), &inspectData); err == nil {
				containers := parsePodmanInspect(inspectData)
				if len(containers) > 0 {
					return containers, warnings
				}
			}
		}
	}

	// Fallback: parse ps data directly.
	return parsePodmanPS(psData), warnings
}

// parsePodmanInspect converts podman inspect JSON into RunningContainer
// structs.
func parsePodmanInspect(data []map[string]interface{}) []schema.RunningContainer {
	var containers []schema.RunningContainer

	for _, c := range data {
		id := stringField(c, "ID", "Id")
		name := stringField(c, "Name")
		name = strings.TrimPrefix(name, "/")

		image := stringField(c, "ImageName", "Image")
		imageID := stringField(c, "ImageID")

		state, _ := c["State"].(map[string]interface{})
		status := stringField(state, "Status")

		mounts := parseMounts(c["Mounts"])
		networks, ports := parseNetworking(c["NetworkSettings"])

		config, _ := c["Config"].(map[string]interface{})
		env := parseStringSlice(config, "Env")

		containers = append(containers, schema.RunningContainer{
			ID:       id,
			Name:     name,
			Image:    image,
			ImageID:  imageID,
			Status:   status,
			Mounts:   mounts,
			Networks: networks,
			Ports:    ports,
			Env:      env,
		})
	}
	return containers
}

// parsePodmanPS is a fallback parser when podman inspect is unavailable.
// It extracts basic fields from podman ps JSON.
func parsePodmanPS(data []map[string]interface{}) []schema.RunningContainer {
	var containers []schema.RunningContainer
	for _, c := range data {
		id := stringField(c, "ID", "Id")

		name := ""
		switch v := c["Names"].(type) {
		case []interface{}:
			if len(v) > 0 {
				name, _ = v[0].(string)
			}
		case string:
			name = v
		}

		state, _ := c["State"].(map[string]interface{})
		status := stringField(state, "Status")
		if status == "" {
			status = stringField(c, "Status", "State")
		}

		containers = append(containers, schema.RunningContainer{
			ID:     id,
			Name:   name,
			Image:  stringField(c, "Image"),
			Status: status,
		})
	}
	return containers
}

// ---------------------------------------------------------------------------
// Flatpak detection
// ---------------------------------------------------------------------------

// detectFlatpakApps lists installed Flatpak applications.
func detectFlatpakApps(exec Executor) []schema.FlatpakApp {
	// Check if flatpak is installed.
	which := exec.Run("which", "flatpak")
	if which.ExitCode != 0 {
		return nil
	}

	result := exec.Run("flatpak", "list", "--app", "--columns=application,origin,branch")
	if result.ExitCode != 0 {
		return nil
	}

	var apps []schema.FlatpakApp
	for _, line := range strings.Split(result.Stdout, "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}

		parts := strings.Split(line, "\t")
		if len(parts) < 2 {
			continue
		}

		branch := ""
		if len(parts) >= 3 {
			branch = strings.TrimSpace(parts[2])
		}

		apps = append(apps, schema.FlatpakApp{
			AppID:  strings.TrimSpace(parts[0]),
			Origin: strings.TrimSpace(parts[1]),
			Branch: branch,
		})
	}
	return apps
}

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

// stringField extracts the first non-empty string value from a map by
// trying each key in order.
func stringField(m map[string]interface{}, keys ...string) string {
	if m == nil {
		return ""
	}
	for _, k := range keys {
		if v, ok := m[k].(string); ok && v != "" {
			return v
		}
	}
	return ""
}

// parseMounts converts a Mounts JSON array into ContainerMount slices.
func parseMounts(v interface{}) []schema.ContainerMount {
	arr, ok := v.([]interface{})
	if !ok {
		return nil
	}
	var mounts []schema.ContainerMount
	for _, item := range arr {
		m, ok := item.(map[string]interface{})
		if !ok {
			continue
		}
		rw := true
		if b, ok := m["RW"].(bool); ok {
			rw = b
		}
		mounts = append(mounts, schema.ContainerMount{
			Type:        stringField(m, "Type"),
			Source:      stringField(m, "Source"),
			Destination: stringField(m, "Destination"),
			Mode:        stringField(m, "Mode"),
			RW:          rw,
		})
	}
	return mounts
}

// parseNetworking extracts networks and ports from NetworkSettings.
func parseNetworking(v interface{}) (map[string]interface{}, map[string]interface{}) {
	ns, ok := v.(map[string]interface{})
	if !ok {
		return nil, nil
	}
	networks, _ := ns["Networks"].(map[string]interface{})
	ports, _ := ns["Ports"].(map[string]interface{})
	return networks, ports
}

// parseStringSlice extracts a []string from a JSON array field.
func parseStringSlice(m map[string]interface{}, key string) []string {
	if m == nil {
		return nil
	}
	arr, ok := m[key].([]interface{})
	if !ok {
		return nil
	}
	var result []string
	for _, v := range arr {
		if s, ok := v.(string); ok {
			result = append(result, s)
		}
	}
	return result
}

