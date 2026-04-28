// Non-RPM Software inspector: scans /opt, /srv, /usr/local for software
// not installed via RPM. Detects ELF binaries (readelf classification),
// Python venvs and pip packages, npm/yarn/gem lockfiles, git repositories,
// and .env files for secrets review.
//
// User home directories (/home) are intentionally excluded — artifacts
// found there are overwhelmingly development checkouts, not deployed
// services.
package inspector

import (
	"path/filepath"
	"regexp"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// NonRpmOptions configures the Non-RPM Software inspector.
type NonRpmOptions struct {
	// DeepBinaryScan enables full strings output analysis (vs 4KB head).
	DeepBinaryScan bool

	// SystemType is the detected system type.
	SystemType schema.SystemType
}

// RunNonRpmSoftware runs the Non-RPM Software inspector and returns the
// populated section, accumulated warnings, and any fatal error.
func RunNonRpmSoftware(exec Executor, opts NonRpmOptions) (*schema.NonRpmSoftwareSection, []Warning, error) {
	var warnings []Warning
	section := &schema.NonRpmSoftwareSection{
		Items:    []schema.NonRpmItem{},
		EnvFiles: []schema.ConfigFileEntry{},
	}

	isOstree := opts.SystemType == schema.SystemTypeRpmOstree ||
		opts.SystemType == schema.SystemTypeBootc

	// Probe for binary analysis tool availability (warn once).
	hasReadelf := probeCommand(exec, "readelf")
	hasFile := true
	if !hasReadelf {
		warnings = append(warnings, makeWarning(
			"non_rpm_software",
			"readelf not available (rc=127) — ELF binary classification skipped. Install binutils in the inspectah container image.",
		))
	} else {
		hasFile = probeCommand(exec, "file")
		if !hasFile {
			warnings = append(warnings, makeWarning(
				"non_rpm_software",
				"file not available (rc=127) — binary type detection skipped. Install file in the inspectah container image.",
			))
		}
	}

	tools := binaryTools{hasReadelf: hasReadelf, hasFile: hasFile}

	scanDirs(exec, section, tools, opts.DeepBinaryScan, isOstree)
	scanVenvPackages(exec, section, &warnings)
	scanPip(exec, section, isOstree)
	scanNpm(exec, section, isOstree)
	scanGem(exec, section, isOstree)
	scanEnvFiles(exec, section)

	// Filter ostree-internal /var paths.
	if isOstree {
		filterOstreeVarPaths(section)
	}

	// Deduplicate: keep highest-confidence item per path.
	deduplicateItems(section)

	return section, warnings, nil
}

// ---------------------------------------------------------------------------
// Tool availability
// ---------------------------------------------------------------------------

type binaryTools struct {
	hasReadelf bool
	hasFile    bool
}

// probeCommand checks if a command is available (exit code != 127).
func probeCommand(exec Executor, name string) bool {
	r := exec.Run(name, "--version")
	return r.ExitCode != 127
}

// isDevArtifactRel returns true if any component of the relative path
// is a dev/build directory. Delegates to the shared pruneMarkers and
// skipDirNames maps defined in config.go via IsDevArtifact.
func isDevArtifactRel(relPath string) bool {
	return IsDevArtifact(relPath, "")
}

// ---------------------------------------------------------------------------
// FHS constants
// ---------------------------------------------------------------------------

var fhsDirs = map[string]bool{
	"bin": true, "etc": true, "games": true, "include": true,
	"lib": true, "lib64": true, "libexec": true,
	"sbin": true, "share": true, "src": true, "man": true,
}

var fhsBinDirs = map[string]bool{
	"bin": true, "sbin": true, "libexec": true,
}

var fhsLibDirs = map[string]bool{
	"lib": true, "lib64": true,
}

var fhsEnumerateDirs = mergeMaps(fhsBinDirs, fhsLibDirs)

func mergeMaps(a, b map[string]bool) map[string]bool {
	m := make(map[string]bool, len(a)+len(b))
	for k := range a {
		m[k] = true
	}
	for k := range b {
		m[k] = true
	}
	return m
}

// ---------------------------------------------------------------------------
// Version patterns
// ---------------------------------------------------------------------------

var versionPatterns = []*regexp.Regexp{
	regexp.MustCompile(`(?i)version\s*[=:]\s*["']?([0-9]+\.[0-9]+(?:\.[0-9]+)?)`),
	regexp.MustCompile(`v([0-9]+\.[0-9]+(?:\.[0-9]+)?)[\s\-]`),
	regexp.MustCompile(`([0-9]+\.[0-9]+\.[0-9]+)(?:\s|$|\))`),
}

var deepVersionPatterns = append(versionPatterns,
	regexp.MustCompile(`go([0-9]+\.[0-9]+(?:\.[0-9]+)?)\b`),
	regexp.MustCompile(`rustc\s+([0-9]+\.[0-9]+\.[0-9]+)`),
	regexp.MustCompile(`(?i)(?:built|compiled|linked)\s+(?:with|against)\s+\S+\s+([0-9]+\.[0-9]+\.[0-9]+)`),
	regexp.MustCompile(`(?:release|tag)[/\-]v?([0-9]+\.[0-9]+\.[0-9]+)`),
	regexp.MustCompile(`([0-9]+\.[0-9]+\.[0-9]+[\-][a-zA-Z0-9.]+)`),
	regexp.MustCompile(`v([0-9]+\.[0-9]+\.[0-9]+)-[0-9]+-g[0-9a-f]+`),
	regexp.MustCompile(`(?i)(?:OpenSSL|LibreSSL|BoringSSL)\s+([0-9]+\.[0-9]+\.[0-9]+[a-z]?)`),
	regexp.MustCompile(`(?i)java\s+version\s+["']([0-9]+\.[0-9]+\.[0-9]+)`),
	regexp.MustCompile(`(?i)node\s+v([0-9]+\.[0-9]+\.[0-9]+)`),
	regexp.MustCompile(`Python\s+([0-9]+\.[0-9]+\.[0-9]+)`),
)

// ---------------------------------------------------------------------------
// readelf-based binary classification
// ---------------------------------------------------------------------------

// classifyBinary uses readelf to classify an ELF binary. Returns nil if
// readelf is unavailable or the file is not an ELF binary.
func classifyBinary(exec Executor, tools binaryTools, path string) *binaryClassification {
	if !tools.hasReadelf {
		return nil
	}

	r := exec.Run("readelf", "-S", path)
	if r.ExitCode != 0 {
		return nil
	}

	isGo := strings.Contains(r.Stdout, ".note.go.buildid") ||
		strings.Contains(r.Stdout, ".gopclntab")
	isRust := strings.Contains(r.Stdout, ".rustc")

	rd := exec.Run("readelf", "-d", path)
	dynamicOutput := ""
	if rd.ExitCode == 0 {
		dynamicOutput = rd.Stdout
	}
	isStatic := strings.Contains(strings.ToLower(dynamicOutput), "no dynamic section") ||
		strings.TrimSpace(dynamicOutput) == ""

	var sharedLibs []string
	for _, line := range strings.Split(dynamicOutput, "\n") {
		if strings.Contains(line, "(NEEDED)") {
			if start := strings.Index(line, "["); start >= 0 {
				if end := strings.Index(line[start:], "]"); end >= 0 {
					sharedLibs = append(sharedLibs, line[start+1:start+end])
				}
			}
		}
	}

	lang := "c/c++"
	if isGo {
		lang = "go"
	} else if isRust {
		lang = "rust"
	}

	return &binaryClassification{
		lang:       lang,
		static:     isStatic,
		sharedLibs: sharedLibs,
	}
}

type binaryClassification struct {
	lang       string
	static     bool
	sharedLibs []string
}

// isBinary uses the `file` command to detect executables.
func isBinary(exec Executor, tools binaryTools, path string) bool {
	if !tools.hasFile {
		return false
	}
	r := exec.Run("file", "-b", path)
	if r.ExitCode != 0 {
		return false
	}
	out := strings.ToLower(r.Stdout)
	return strings.Contains(out, "elf") ||
		strings.Contains(out, "executable") ||
		strings.Contains(out, "script")
}

// stringsVersion extracts a version string from a binary using the
// `strings` command. When limitKB > 0, only the first limitKB*1024
// bytes are scanned.
func stringsVersion(exec Executor, path string, limitKB int, deep bool) string {
	var r ExecResult
	if limitKB > 0 {
		r = exec.Run("sh", "-c", "head -c "+itoa(limitKB*1024)+" "+path+" | strings")
	} else {
		r = exec.Run("strings", path)
	}
	if r.ExitCode != 0 {
		return ""
	}

	patterns := versionPatterns
	if deep {
		patterns = deepVersionPatterns
	}
	for _, pat := range patterns {
		if m := pat.FindStringSubmatch(r.Stdout); len(m) > 1 {
			return strings.TrimSpace(m[1])
		}
	}
	return ""
}

// itoa converts an int to a string without importing strconv.
func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	s := ""
	neg := false
	if n < 0 {
		neg = true
		n = -n
	}
	for n > 0 {
		s = string(rune('0'+n%10)) + s
		n /= 10
	}
	if neg {
		s = "-" + s
	}
	return s
}

// ---------------------------------------------------------------------------
// Directory scanning
// ---------------------------------------------------------------------------

// dirHasContent returns true if the directory tree contains at least one
// file (checked via ReadDir recursion through the executor).
func dirHasContent(exec Executor, path string) bool {
	entries, err := exec.ReadDir(path)
	if err != nil {
		return false
	}
	for _, e := range entries {
		child := filepath.Join(path, e.Name())
		if e.IsDir() {
			if dirHasContent(exec, child) {
				return true
			}
		} else {
			return true
		}
	}
	return false
}

// classifyFile classifies a single file and returns a NonRpmItem.
func classifyFile(exec Executor, tools binaryTools, path, relPath string, deep bool) schema.NonRpmItem {
	item := schema.NonRpmItem{
		Path:       relPath,
		Name:       filepath.Base(path),
		Confidence: "low",
		Method:     "file scan",
	}

	bc := classifyBinary(exec, tools, path)
	if bc != nil {
		item.Lang = bc.lang
		item.Static = bc.static
		item.SharedLibs = bc.sharedLibs
		item.Confidence = "high"
		item.Method = "readelf (" + bc.lang + ")"
		return item
	}

	if isBinary(exec, tools, path) {
		limit := 4
		if deep {
			limit = 0
		}
		ver := stringsVersion(exec, path, limit, deep)
		if ver != "" {
			item.Version = ver
			if deep {
				item.Method = "strings"
			} else {
				item.Method = "strings (first 4KB)"
			}
			item.Confidence = "medium"
		}
	}

	return item
}

// scanFhsDirFiles enumerates individual files inside an FHS directory
// (bin, lib, etc.) and classifies each one.
func scanFhsDirFiles(
	exec Executor, section *schema.NonRpmSoftwareSection,
	tools binaryTools, fhsDir, relBase string, deep bool,
) {
	entries, err := exec.ReadDir(fhsDir)
	if err != nil {
		return
	}

	dirName := filepath.Base(fhsDir)
	for _, e := range entries {
		if strings.HasPrefix(e.Name(), ".") {
			continue
		}
		childPath := filepath.Join(fhsDir, e.Name())
		childRel := filepath.Join(relBase, e.Name())

		if e.IsDir() {
			// Recurse one level for lib subdirs.
			if fhsLibDirs[dirName] {
				scanFhsDirFiles(exec, section, tools, childPath, childRel, deep)
			}
			continue
		}

		// File — classify it.
		item := classifyFile(exec, tools, childPath, childRel, deep)
		section.Items = append(section.Items, item)
	}
}

// scanDirs scans /opt and /usr/local for non-RPM software directories.
func scanDirs(
	exec Executor, section *schema.NonRpmSoftwareSection,
	tools binaryTools, deep bool, isOstree bool,
) {
	scanBases := []string{"/opt"}
	if !isOstree {
		scanBases = append(scanBases, "/usr/local")
	}

	for _, base := range scanBases {
		entries, err := exec.ReadDir(base)
		if err != nil {
			continue
		}

		relBase := strings.TrimPrefix(base, "/")

		for _, entry := range entries {
			if !entry.IsDir() || strings.HasPrefix(entry.Name(), ".") {
				continue
			}
			childPath := filepath.Join(base, entry.Name())
			childRel := filepath.Join(relBase, entry.Name())

			if isDevArtifactRel(childRel) {
				continue
			}

			// FHS dirs under /usr/local: skip empty, enumerate bin/lib.
			if base == "/usr/local" {
				if fhsDirs[entry.Name()] && !dirHasContent(exec, childPath) {
					continue
				}
				if fhsEnumerateDirs[entry.Name()] {
					scanFhsDirFiles(exec, section, tools, childPath, childRel, deep)
					continue
				}
			}

			// Check for git repo first.
			gitItem := scanGitRepo(exec, childPath, childRel)
			if gitItem != nil {
				section.Items = append(section.Items, *gitItem)
				continue
			}

			// Skip venvs — handled separately.
			if exec.FileExists(filepath.Join(childPath, "pyvenv.cfg")) {
				continue
			}

			// Generic directory scan.
			item := schema.NonRpmItem{
				Path:       childRel,
				Name:       entry.Name(),
				Confidence: "low",
				Method:     "directory scan",
			}

			// Try to classify first binary found in the subtree.
			classifyFirstBinary(exec, tools, childPath, &item, deep)

			section.Items = append(section.Items, item)
		}
	}
}

// classifyFirstBinary walks a directory tree looking for the first
// classifiable binary and updates the item in place.
func classifyFirstBinary(
	exec Executor, tools binaryTools, dirPath string,
	item *schema.NonRpmItem, deep bool,
) {
	walkDir(exec, dirPath, func(path, relPath string, isDir bool) bool {
		if isDir {
			name := filepath.Base(path)
			return !pruneMarkers[name] && !skipDirNames[name]
		}
		// File — try to classify.
		bc := classifyBinary(exec, tools, path)
		if bc != nil {
			item.Lang = bc.lang
			item.Static = bc.static
			item.SharedLibs = bc.sharedLibs
			item.Confidence = "high"
			item.Method = "readelf (" + bc.lang + ")"
			return false // stop walking
		}
		if isBinary(exec, tools, path) {
			limit := 4
			if deep {
				limit = 0
			}
			ver := stringsVersion(exec, path, limit, deep)
			if ver != "" {
				item.Version = ver
				if deep {
					item.Method = "strings"
				} else {
					item.Method = "strings (first 4KB)"
				}
				item.Confidence = "medium"
				return false // stop walking
			}
		}
		return true // keep looking
	})
}

// walkDir recursively walks a directory tree using the executor. The
// visitor returns true to continue, false to stop. For directories,
// returning false skips that subtree.
func walkDir(exec Executor, root string, visitor func(path, relPath string, isDir bool) bool) {
	var walk func(dir string)
	walk = func(dir string) {
		entries, err := exec.ReadDir(dir)
		if err != nil {
			return
		}
		for _, e := range entries {
			child := filepath.Join(dir, e.Name())
			if e.IsDir() {
				if visitor(child, "", true) {
					walk(child)
				}
			} else {
				if !visitor(child, "", false) {
					return
				}
			}
		}
	}
	walk(root)
}

// ---------------------------------------------------------------------------
// Git repository detection
// ---------------------------------------------------------------------------

// scanGitRepo checks if a directory has .git and extracts remote URL,
// commit hash, and branch name.
func scanGitRepo(exec Executor, dirPath, relPath string) *schema.NonRpmItem {
	gitDir := filepath.Join(dirPath, ".git")
	if !exec.FileExists(gitDir) {
		return nil
	}

	// Extract remote URL from .git/config.
	remoteURL := ""
	configContent, err := exec.ReadFile(filepath.Join(gitDir, "config"))
	if err == nil {
		for _, line := range strings.Split(configContent, "\n") {
			stripped := strings.TrimSpace(line)
			if strings.HasPrefix(stripped, "url =") {
				remoteURL = strings.TrimSpace(strings.SplitN(stripped, "=", 2)[1])
				break
			}
		}
	}

	// Extract commit hash and branch from HEAD.
	commitHash := ""
	branch := ""
	headContent, err := exec.ReadFile(filepath.Join(gitDir, "HEAD"))
	if err == nil {
		headContent = strings.TrimSpace(headContent)
		if strings.HasPrefix(headContent, "ref:") {
			ref := strings.TrimSpace(strings.SplitN(headContent, ":", 2)[1])
			if strings.HasPrefix(ref, "refs/heads/") {
				branch = ref[len("refs/heads/"):]
			}
			refContent, err := exec.ReadFile(filepath.Join(gitDir, ref))
			if err == nil {
				commitHash = strings.TrimSpace(refContent)
			}
		} else {
			commitHash = headContent
		}
	}

	return &schema.NonRpmItem{
		Path:       relPath,
		Name:       filepath.Base(dirPath),
		Method:     "git repository",
		Confidence: "high",
		GitRemote:  remoteURL,
		GitCommit:  commitHash,
		GitBranch:  branch,
	}
}

// ---------------------------------------------------------------------------
// Python venv detection
// ---------------------------------------------------------------------------

// scanVenvPackages discovers Python venvs under /opt, /srv and scans
// their dist-info directories for installed packages.
func scanVenvPackages(
	exec Executor, section *schema.NonRpmSoftwareSection,
	warnings *[]Warning,
) {
	pipFailCount := 0

	for _, searchRoot := range []string{"/opt", "/srv"} {
		venvs := findVenvs(exec, searchRoot)
		for _, v := range venvs {
			packages := scanDistInfo(exec, v.path)

			// Try pip list --path for richer package list.
			pipPackages := tryPipList(exec, v.path)
			if pipPackages != nil {
				packages = pipPackages
			} else if pipPackages == nil && exec.FileExists(v.path) {
				// pip list attempted and failed
				pipFailCount++
			}

			relPath := strings.TrimPrefix(v.path, "/")
			section.Items = append(section.Items, schema.NonRpmItem{
				Path:               relPath,
				Name:               filepath.Base(v.path),
				Method:             "python venv",
				Confidence:         "high",
				SystemSitePackages: v.systemSitePackages,
				Packages:           packages,
			})
		}
	}

	if pipFailCount > 0 {
		*warnings = append(*warnings, makeWarning(
			"non_rpm_software",
			"pip list --path failed for venv(s) — package inventory may be incomplete (dist-info scan used as fallback).",
		))
	}
}

type venvInfo struct {
	path               string
	systemSitePackages bool
}

// findVenvs looks for pyvenv.cfg files under a root directory.
func findVenvs(exec Executor, root string) []venvInfo {
	var results []venvInfo

	var walk func(dir string)
	walk = func(dir string) {
		entries, err := exec.ReadDir(dir)
		if err != nil {
			return
		}
		for _, e := range entries {
			child := filepath.Join(dir, e.Name())
			if e.IsDir() {
				name := e.Name()
				if pruneMarkers[name] || skipDirNames[name] {
					continue
				}
				walk(child)
			} else if e.Name() == "pyvenv.cfg" {
				venvDir := dir
				systemSP := false
				content, err := exec.ReadFile(child)
				if err == nil {
					for _, line := range strings.Split(content, "\n") {
						lower := strings.ToLower(strings.TrimSpace(line))
						if strings.HasPrefix(lower, "include-system-site-packages") {
							systemSP = strings.Contains(lower, "true")
							break
						}
					}
				}
				results = append(results, venvInfo{
					path:               venvDir,
					systemSitePackages: systemSP,
				})
			}
		}
	}
	walk(root)
	return results
}

// scanDistInfo scans dist-info directories inside a venv's site-packages.
func scanDistInfo(exec Executor, venvPath string) []schema.PipPackage {
	var packages []schema.PipPackage

	// Look for site-packages directories.
	var findSitePackages func(dir string)
	findSitePackages = func(dir string) {
		entries, err := exec.ReadDir(dir)
		if err != nil {
			return
		}
		for _, e := range entries {
			child := filepath.Join(dir, e.Name())
			if e.IsDir() {
				if e.Name() == "site-packages" {
					// Scan for .dist-info dirs inside site-packages.
					spEntries, err := exec.ReadDir(child)
					if err != nil {
						continue
					}
					for _, sp := range spEntries {
						if sp.IsDir() && strings.HasSuffix(sp.Name(), ".dist-info") {
							name, version := parseDistInfoName(
								strings.TrimSuffix(sp.Name(), ".dist-info"),
							)
							packages = append(packages, schema.PipPackage{
								Name:    name,
								Version: version,
							})
						}
					}
				} else {
					findSitePackages(child)
				}
			}
		}
	}
	findSitePackages(venvPath)
	return packages
}

// parseDistInfoName splits "name-version" into (name, version).
func parseDistInfoName(s string) (string, string) {
	idx := strings.LastIndex(s, "-")
	if idx < 0 {
		return s, ""
	}
	return s[:idx], s[idx+1:]
}

// tryPipList attempts to run pip list --path and parse the output.
func tryPipList(exec Executor, venvPath string) []schema.PipPackage {
	// Find a site-packages directory under the venv.
	spPath := findSitePackagesPath(exec, venvPath)
	if spPath == "" {
		return nil
	}

	r := exec.Run("pip", "list", "--path", spPath, "--format", "columns")
	if r.ExitCode != 0 || strings.TrimSpace(r.Stdout) == "" {
		return nil
	}
	return parsePipList(r.Stdout)
}

// findSitePackagesPath finds the first site-packages directory under a root.
func findSitePackagesPath(exec Executor, root string) string {
	var result string
	var walk func(dir string) bool
	walk = func(dir string) bool {
		entries, err := exec.ReadDir(dir)
		if err != nil {
			return true
		}
		for _, e := range entries {
			if e.IsDir() {
				child := filepath.Join(dir, e.Name())
				if e.Name() == "site-packages" {
					result = child
					return false
				}
				if !walk(child) {
					return false
				}
			}
		}
		return true
	}
	walk(root)
	return result
}

// parsePipList parses `pip list` columnar output.
func parsePipList(output string) []schema.PipPackage {
	var packages []schema.PipPackage
	for _, line := range strings.Split(strings.TrimSpace(output), "\n") {
		if strings.HasPrefix(line, "---") || strings.HasPrefix(line, "Package") {
			continue
		}
		parts := strings.Fields(line)
		if len(parts) >= 2 {
			packages = append(packages, schema.PipPackage{
				Name:    parts[0],
				Version: parts[1],
			})
		}
	}
	return packages
}

// ---------------------------------------------------------------------------
// pip dist-info scanning (system-level)
// ---------------------------------------------------------------------------

// scanPip detects pip-installed packages by scanning system dist-info dirs.
func scanPip(exec Executor, section *schema.NonRpmSoftwareSection, isOstree bool) {
	var searchRoots []string
	if isOstree {
		searchRoots = []string{"/usr/local/lib/python3"}
	} else {
		searchRoots = []string{
			"/usr/lib/python3",
			"/usr/lib64/python3",
			"/usr/local/lib/python3",
		}
	}

	for _, searchRoot := range searchRoots {
		// The search root is a prefix — look for python3.X dirs.
		parentDir := filepath.Dir(searchRoot)
		prefix := filepath.Base(searchRoot)

		entries, err := exec.ReadDir(parentDir)
		if err != nil {
			continue
		}

		for _, e := range entries {
			if !e.IsDir() || !strings.HasPrefix(e.Name(), prefix) {
				continue
			}

			pyDir := filepath.Join(parentDir, e.Name())
			spDir := filepath.Join(pyDir, "site-packages")
			if !exec.FileExists(spDir) {
				// Fall back to the directory itself.
				spDir = pyDir
			}

			spEntries, err := exec.ReadDir(spDir)
			if err != nil {
				continue
			}

			for _, sp := range spEntries {
				if !sp.IsDir() || !strings.HasSuffix(sp.Name(), ".dist-info") {
					continue
				}

				name, version := parseDistInfoName(
					strings.TrimSuffix(sp.Name(), ".dist-info"),
				)

				// Check for C extensions via RECORD file.
				hasCExt := false
				recordPath := filepath.Join(spDir, sp.Name(), "RECORD")
				recordContent, err := exec.ReadFile(recordPath)
				if err == nil {
					for _, line := range strings.Split(recordContent, "\n") {
						trimmed := strings.TrimSpace(line)
						if strings.HasSuffix(trimmed, ".so") ||
							strings.Contains(trimmed, ".so,") {
							hasCExt = true
							break
						}
					}
				}

				relPath := strings.TrimPrefix(
					filepath.Join(spDir, sp.Name()), "/",
				)
				section.Items = append(section.Items, schema.NonRpmItem{
					Path:           relPath,
					Name:           name,
					Version:        version,
					Confidence:     "high",
					Method:         "pip dist-info",
					HasCExtensions: hasCExt,
				})
			}
		}
	}

	// Scan for requirements.txt files under /opt, /srv.
	for _, root := range []string{"/opt", "/srv"} {
		findFiles(exec, root, "requirements.txt", func(path string) {
			content, _ := exec.ReadFile(path)
			relPath := strings.TrimPrefix(path, "/")
			section.Items = append(section.Items, schema.NonRpmItem{
				Path:       relPath,
				Name:       "requirements.txt",
				Confidence: "high",
				Method:     "pip requirements.txt",
				Content:    content,
			})
		})
	}
}

// findFiles recursively searches for files with the given name, pruning
// dev artifact directories.
func findFiles(exec Executor, root, targetName string, handler func(path string)) {
	var walk func(dir string)
	walk = func(dir string) {
		entries, err := exec.ReadDir(dir)
		if err != nil {
			return
		}
		for _, e := range entries {
			child := filepath.Join(dir, e.Name())
			if e.IsDir() {
				name := e.Name()
				if pruneMarkers[name] || skipDirNames[name] {
					continue
				}
				walk(child)
			} else if e.Name() == targetName {
				handler(child)
			}
		}
	}
	walk(root)
}

// ---------------------------------------------------------------------------
// npm/yarn scanning
// ---------------------------------------------------------------------------

var lockfileNames = map[string]bool{
	"package.json":      true,
	"package-lock.json": true,
	"yarn.lock":         true,
	"Gemfile":           true,
	"Gemfile.lock":      true,
}

// readLockfileDir reads lockfile-related files from a directory.
func readLockfileDir(exec Executor, dir string) *map[string]interface{} {
	files := make(map[string]interface{})
	for name := range lockfileNames {
		path := filepath.Join(dir, name)
		content, err := exec.ReadFile(path)
		if err == nil {
			files[name] = content
		}
	}
	if len(files) == 0 {
		return nil
	}
	return &files
}

// scanNpm scans for npm/yarn lockfiles under /opt, /srv, /usr/local.
func scanNpm(exec Executor, section *schema.NonRpmSoftwareSection, isOstree bool) {
	roots := []string{"/opt", "/srv"}
	if !isOstree {
		roots = append(roots, "/usr/local")
	}

	for _, root := range roots {
		// Search for package-lock.json.
		findFiles(exec, root, "package-lock.json", func(path string) {
			dir := filepath.Dir(path)
			files := readLockfileDir(exec, dir)
			relPath := strings.TrimPrefix(dir, "/")
			section.Items = append(section.Items, schema.NonRpmItem{
				Path:       relPath,
				Name:       filepath.Base(dir),
				Confidence: "high",
				Method:     "npm package-lock.json",
				Files:      files,
			})
		})

		// Search for yarn.lock.
		findFiles(exec, root, "yarn.lock", func(path string) {
			dir := filepath.Dir(path)
			files := readLockfileDir(exec, dir)
			relPath := strings.TrimPrefix(dir, "/")
			section.Items = append(section.Items, schema.NonRpmItem{
				Path:       relPath,
				Name:       filepath.Base(dir),
				Confidence: "high",
				Method:     "yarn.lock",
				Files:      files,
			})
		})
	}
}

// ---------------------------------------------------------------------------
// Gem scanning
// ---------------------------------------------------------------------------

// scanGem scans for Gemfile.lock under /opt, /srv, /usr/local.
func scanGem(exec Executor, section *schema.NonRpmSoftwareSection, isOstree bool) {
	roots := []string{"/opt", "/srv"}
	if !isOstree {
		roots = append(roots, "/usr/local")
	}

	for _, root := range roots {
		findFiles(exec, root, "Gemfile.lock", func(path string) {
			dir := filepath.Dir(path)
			files := readLockfileDir(exec, dir)
			relPath := strings.TrimPrefix(dir, "/")
			section.Items = append(section.Items, schema.NonRpmItem{
				Path:       relPath,
				Name:       filepath.Base(dir),
				Confidence: "high",
				Method:     "gem Gemfile.lock",
				Files:      files,
			})
		})
	}
}

// ---------------------------------------------------------------------------
// .env file scanning
// ---------------------------------------------------------------------------

var envFileNames = map[string]bool{
	".env":              true,
	".env.local":        true,
	".env.production":   true,
	".env.staging":      true,
	".env.development":  true,
}

// scanEnvFiles scans /opt for dotenv files and adds them to section.EnvFiles.
func scanEnvFiles(exec Executor, section *schema.NonRpmSoftwareSection) {
	findFilesMatching(exec, "/opt", func(name string) bool {
		return envFileNames[name]
	}, func(path string) {
		content, _ := exec.ReadFile(path)
		relPath := strings.TrimPrefix(path, "/")
		section.EnvFiles = append(section.EnvFiles, schema.ConfigFileEntry{
			Path:    relPath,
			Kind:    schema.ConfigFileKindUnowned,
			Content: content,
		})
	})
}

// findFilesMatching recursively searches for files whose names match the
// predicate, pruning dev artifact directories.
func findFilesMatching(exec Executor, root string, match func(string) bool, handler func(string)) {
	var walk func(dir string)
	walk = func(dir string) {
		entries, err := exec.ReadDir(dir)
		if err != nil {
			return
		}
		for _, e := range entries {
			child := filepath.Join(dir, e.Name())
			if e.IsDir() {
				name := e.Name()
				if pruneMarkers[name] || skipDirNames[name] {
					continue
				}
				walk(child)
			} else if match(e.Name()) {
				handler(child)
			}
		}
	}
	walk(root)
}

// ---------------------------------------------------------------------------
// Post-processing
// ---------------------------------------------------------------------------

// filterOstreeVarPaths removes ostree-internal /var paths from items.
func filterOstreeVarPaths(section *schema.NonRpmSoftwareSection) {
	ostreeVarInternals := []string{
		"var/lib/ostree",
		"var/lib/rpm-ostree",
		"var/lib/flatpak",
	}

	filtered := section.Items[:0]
	for _, item := range section.Items {
		skip := false
		for _, internal := range ostreeVarInternals {
			if strings.HasPrefix(item.Path, internal) {
				skip = true
				break
			}
		}
		if !skip {
			filtered = append(filtered, item)
		}
	}
	section.Items = filtered
}

// deduplicateItems keeps the highest-confidence item per path.
func deduplicateItems(section *schema.NonRpmSoftwareSection) {
	confidenceRank := map[string]int{
		"high":   2,
		"medium": 1,
		"low":    0,
	}

	type entry struct {
		item schema.NonRpmItem
		rank int
	}

	seen := make(map[string]entry)
	// Preserve order using a separate slice of keys.
	var order []string

	for _, item := range section.Items {
		rank := confidenceRank[item.Confidence]
		if existing, ok := seen[item.Path]; !ok {
			seen[item.Path] = entry{item: item, rank: rank}
			order = append(order, item.Path)
		} else if rank > existing.rank {
			seen[item.Path] = entry{item: item, rank: rank}
		}
	}

	deduped := make([]schema.NonRpmItem, 0, len(order))
	for _, path := range order {
		deduped = append(deduped, seen[path].item)
	}
	section.Items = deduped
}
