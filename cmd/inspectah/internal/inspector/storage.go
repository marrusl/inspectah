// Storage inspector: fstab, active mounts (findmnt), LVM volumes,
// iSCSI/multipath/automount detection, /var directory scan, and
// credential reference detection in mount options.
package inspector

import (
	"encoding/json"
	"fmt"
	"path/filepath"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// credOptionKeys lists fstab mount-option keys whose values point to
// credential files.
var credOptionKeys = []string{
	"credentials",
	"credential",
	"password_file",
	"secretfile",
}

// varScanDirs lists subdirectories of /var to scan for application data,
// each with a default category for migration recommendations.
var varScanDirs = []struct {
	subdir   string
	category string
}{
	{"var/lib", "application data"},
	{"var/log", "log retention"},
	{"var/data", "application data"},
	{"var/www", "web content"},
	{"var/opt", "add-on packages"},
}

// varLibSkip holds known OS-managed directories under /var/lib that should
// be excluded from the migration plan.
var varLibSkip = map[string]bool{
	"alternatives":   true,
	"authselect":     true,
	"dbus":           true,
	"dnf":            true,
	"logrotate":      true,
	"misc":           true,
	"NetworkManager": true,
	"os-prober":      true,
	"plymouth":       true,
	"polkit-1":       true,
	"portables":      true,
	"private":        true,
	"rpm":            true,
	"rpm-state":      true,
	"selinux":        true,
	"sss":            true,
	"systemd":        true,
	"tuned":          true,
	"unbound":        true,
	"tpm2-tss":       true,
}

// ostreeManagedMounts are mount targets to filter on ostree/bootc systems.
var ostreeManagedMounts = map[string]bool{
	"/sysroot":  true,
	"/ostree":   true,
	"/boot/efi": true,
}

// ostreeMountPrefixes are mount-target prefixes to filter on ostree/bootc.
var ostreeMountPrefixes = []string{"/ostree/", "/sysroot/"}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// StorageOptions configures the storage inspector.
type StorageOptions struct {
	SystemType schema.SystemType
}

// RunStorage inspects storage configuration: fstab entries, active mounts,
// LVM volumes, iSCSI/multipath/dm-crypt presence, automount maps, /var
// directory inventory, and credential references. Returns the populated
// section, accumulated warnings, and any fatal error.
func RunStorage(exec Executor, opts StorageOptions) (*schema.StorageSection, []Warning, error) {
	var warnings []Warning
	section := &schema.StorageSection{
		FstabEntries:   []schema.FstabEntry{},
		MountPoints:    []schema.MountPoint{},
		LvmInfo:        []schema.LvmVolume{},
		VarDirectories: []schema.VarDirectory{},
		CredentialRefs: []schema.CredentialRef{},
	}

	// ---- fstab parsing ----
	parseFstab(exec, section)

	// ---- Active mounts via findmnt ----
	parseFindmnt(exec, section)

	// ---- LVM volumes ----
	parseLVM(exec, section)

	// ---- iSCSI configuration ----
	detectISCSI(exec, section)

	// ---- Multipath ----
	detectMultipath(exec, section)

	// ---- LVM config files ----
	detectLVMConfig(exec, section)

	// ---- dm-crypt devices ----
	detectDMCrypt(exec, section)

	// ---- Automount maps ----
	detectAutomount(exec, section)

	// ---- /var directory scan ----
	section.VarDirectories = scanVarDirectories(exec)

	// ---- ostree mount filtering ----
	if opts.SystemType == schema.SystemTypeRpmOstree || opts.SystemType == schema.SystemTypeBootc {
		section.MountPoints = filterOstreeMounts(section.MountPoints)
	}

	return section, warnings, nil
}

// ---------------------------------------------------------------------------
// fstab parsing
// ---------------------------------------------------------------------------

func parseFstab(exec Executor, section *schema.StorageSection) {
	content, err := exec.ReadFile("/etc/fstab")
	if err != nil {
		return
	}

	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		fields := strings.Fields(line)
		if len(fields) < 3 {
			continue
		}

		opts := ""
		if len(fields) >= 4 {
			opts = fields[3]
		}

		section.FstabEntries = append(section.FstabEntries, schema.FstabEntry{
			Device:     fields[0],
			MountPoint: fields[1],
			Fstype:     fields[2],
			Options:    opts,
		})

		// Scan mount options for credential references.
		detectCredentials(fields[1], opts, section)
	}
}

// detectCredentials scans a comma-separated options string for references
// to credential files.
func detectCredentials(mountPoint, opts string, section *schema.StorageSection) {
	for _, opt := range strings.Split(opts, ",") {
		opt = strings.TrimSpace(opt)
		for _, key := range credOptionKeys {
			if strings.HasPrefix(opt, key+"=") {
				credPath := strings.SplitN(opt, "=", 2)[1]
				section.CredentialRefs = append(section.CredentialRefs, schema.CredentialRef{
					MountPoint:     mountPoint,
					CredentialPath: credPath,
					Source:         "fstab",
				})
			}
		}
	}
}

// ---------------------------------------------------------------------------
// findmnt parsing
// ---------------------------------------------------------------------------

// findmntOutput matches the JSON schema produced by `findmnt --json`.
type findmntOutput struct {
	Filesystems []findmntFS `json:"filesystems"`
}

type findmntFS struct {
	Target  string `json:"target"`
	Source  string `json:"source"`
	Fstype  string `json:"fstype"`
	Options string `json:"options"`
}

func parseFindmnt(exec Executor, section *schema.StorageSection) {
	r := exec.Run("findmnt", "--json", "--real")
	if r.ExitCode != 0 || strings.TrimSpace(r.Stdout) == "" {
		return
	}

	var out findmntOutput
	if err := json.Unmarshal([]byte(r.Stdout), &out); err != nil {
		return
	}

	for _, fs := range out.Filesystems {
		section.MountPoints = append(section.MountPoints, schema.MountPoint{
			Target:  fs.Target,
			Source:  fs.Source,
			Fstype:  fs.Fstype,
			Options: fs.Options,
		})
	}
}

// ---------------------------------------------------------------------------
// LVM volume parsing
// ---------------------------------------------------------------------------

// lvsOutput matches the JSON schema produced by `lvs --reportformat json`.
type lvsOutput struct {
	Report []lvsReport `json:"report"`
}

type lvsReport struct {
	LV []lvsLV `json:"lv"`
}

type lvsLV struct {
	LvName string `json:"lv_name"`
	VgName string `json:"vg_name"`
	LvSize string `json:"lv_size"`
}

func parseLVM(exec Executor, section *schema.StorageSection) {
	r := exec.Run("lvs", "--reportformat", "json", "--units", "g")
	if r.ExitCode != 0 || strings.TrimSpace(r.Stdout) == "" {
		return
	}

	var out lvsOutput
	if err := json.Unmarshal([]byte(r.Stdout), &out); err != nil {
		return
	}

	if len(out.Report) == 0 {
		return
	}

	for _, lv := range out.Report[0].LV {
		section.LvmInfo = append(section.LvmInfo, schema.LvmVolume{
			LvName: lv.LvName,
			VgName: lv.VgName,
			LvSize: lv.LvSize,
		})
	}
}

// ---------------------------------------------------------------------------
// Special device detection (iSCSI, multipath, LVM config, dm-crypt)
// ---------------------------------------------------------------------------

func detectISCSI(exec Executor, section *schema.StorageSection) {
	if exec.FileExists("/etc/iscsi/initiatorname.iscsi") {
		section.MountPoints = append(section.MountPoints, schema.MountPoint{
			Target: "iSCSI",
			Source: "etc/iscsi/initiatorname.iscsi",
			Fstype: "iscsi",
		})
	}
}

func detectMultipath(exec Executor, section *schema.StorageSection) {
	if exec.FileExists("/etc/multipath.conf") {
		section.MountPoints = append(section.MountPoints, schema.MountPoint{
			Target: "multipath",
			Source: "etc/multipath.conf",
			Fstype: "dm-multipath",
		})
	}
}

func detectLVMConfig(exec Executor, section *schema.StorageSection) {
	if exec.FileExists("/etc/lvm/lvm.conf") {
		section.MountPoints = append(section.MountPoints, schema.MountPoint{
			Target: "lvm-config",
			Source: "etc/lvm/lvm.conf",
			Fstype: "lvm",
		})
	}

	// LVM profile files
	entries, err := exec.ReadDir("/etc/lvm/profile")
	if err != nil {
		return
	}
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".profile") {
			continue
		}
		section.MountPoints = append(section.MountPoints, schema.MountPoint{
			Target: fmt.Sprintf("lvm-profile (%s)", e.Name()),
			Source: filepath.Join("etc/lvm/profile", e.Name()),
			Fstype: "lvm",
		})
	}
}

func detectDMCrypt(exec Executor, section *schema.StorageSection) {
	r := exec.Run("dmsetup", "table", "--target", "crypt")
	if r.ExitCode != 0 || strings.TrimSpace(r.Stdout) == "" {
		return
	}
	if strings.Contains(r.Stdout, "No devices found") {
		return
	}

	for _, line := range strings.Split(strings.TrimSpace(r.Stdout), "\n") {
		name := strings.TrimSpace(line)
		if idx := strings.Index(name, ":"); idx >= 0 {
			name = strings.TrimSpace(name[:idx])
		}
		if name != "" {
			section.MountPoints = append(section.MountPoints, schema.MountPoint{
				Target: fmt.Sprintf("dm-crypt (%s)", name),
				Source: "dmsetup",
				Fstype: "dm-crypt",
			})
		}
	}
}

// ---------------------------------------------------------------------------
// Automount detection
// ---------------------------------------------------------------------------

func detectAutomount(exec Executor, section *schema.StorageSection) {
	// /etc/auto.master
	if content, err := exec.ReadFile("/etc/auto.master"); err == nil {
		text := strings.TrimSpace(content)
		if len(text) > 500 {
			text = text[:500]
		}
		section.MountPoints = append(section.MountPoints, schema.MountPoint{
			Target:  "automount",
			Source:  "etc/auto.master",
			Fstype:  "autofs",
			Options: text,
		})
	}

	// Additional auto.* map files
	entries, err := exec.ReadDir("/etc")
	if err != nil {
		return
	}
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		if strings.HasPrefix(e.Name(), "auto.") && e.Name() != "auto.master" {
			section.MountPoints = append(section.MountPoints, schema.MountPoint{
				Target: fmt.Sprintf("automount (%s)", e.Name()),
				Source: fmt.Sprintf("etc/%s", e.Name()),
				Fstype: "autofs",
			})
		}
	}
}

// ---------------------------------------------------------------------------
// /var directory scan
// ---------------------------------------------------------------------------

// scanVarDirectories inventories non-empty directories under key /var
// subdirectories, producing migration recommendations for each.
func scanVarDirectories(exec Executor) []schema.VarDirectory {
	var results []schema.VarDirectory

	for _, sd := range varScanDirs {
		dirPath := "/" + sd.subdir
		if !exec.FileExists(dirPath) {
			continue
		}
		entries, err := exec.ReadDir(dirPath)
		if err != nil {
			continue
		}

		for _, e := range entries {
			if !e.IsDir() || strings.HasPrefix(e.Name(), ".") {
				continue
			}

			// Skip known OS-managed dirs under var/lib.
			if sd.subdir == "var/lib" && varLibSkip[e.Name()] {
				continue
			}

			childPath := filepath.Join(dirPath, e.Name())

			// Check for non-emptiness: see if the directory has at
			// least one child entry. FakeExecutor reports directories
			// that are registered in the dirs map.
			childEntries, err := exec.ReadDir(childPath)
			if err != nil || len(childEntries) == 0 {
				continue
			}

			// In test environments we cannot rglob for sizes, so we
			// report a placeholder. RealExecutor scenarios will have
			// actual du output.
			sizeEstimate := estimateSize(exec, childPath)

			relPath := strings.TrimPrefix(childPath, "/")
			rec := varRecommendation(relPath, sd.category)
			results = append(results, schema.VarDirectory{
				Path:           relPath,
				SizeEstimate:   sizeEstimate,
				Recommendation: rec,
			})
		}
	}

	return results
}

// estimateSize produces a human-readable size estimate for a directory.
// It runs `du -sb` when available, otherwise returns "unknown".
func estimateSize(exec Executor, path string) string {
	r := exec.Run("du", "-sb", path)
	if r.ExitCode != 0 {
		return "unknown"
	}
	fields := strings.Fields(strings.TrimSpace(r.Stdout))
	if len(fields) == 0 {
		return "unknown"
	}
	return formatBytes(fields[0])
}

// formatBytes converts a string of bytes into a human-readable size.
func formatBytes(bytesStr string) string {
	var n int64
	if _, err := fmt.Sscanf(bytesStr, "%d", &n); err != nil {
		return bytesStr
	}

	const (
		kb = 1024
		mb = 1024 * kb
		gb = 1024 * mb
	)

	switch {
	case n >= gb:
		return fmt.Sprintf("~%.1f GB", float64(n)/float64(gb))
	case n > 10*mb:
		return "Over 10 MB"
	case n >= mb:
		return fmt.Sprintf("~%d MB", n/mb)
	case n >= kb:
		return fmt.Sprintf("~%d KB", n/kb)
	default:
		return fmt.Sprintf("%d bytes", n)
	}
}

// varRecommendation maps a /var directory path to a migration
// recommendation string.
func varRecommendation(path, category string) string {
	p := "/" + path

	switch {
	case strings.Contains(p, "mysql") ||
		strings.Contains(p, "pgsql") ||
		strings.Contains(p, "postgres") ||
		strings.Contains(p, "mongodb") ||
		strings.Contains(p, "mariadb"):
		return "PVC / volume mount — database storage, must persist independently"

	case strings.Contains(p, "containers") ||
		strings.Contains(p, "docker"):
		return "PVC / volume mount — container storage"

	case strings.Contains(p, "/var/log"):
		return "PVC / volume mount — log retention (or ship to external logging)"

	case strings.Contains(p, "/var/www"):
		return "Image-embedded or PVC — depends on whether content is static"

	case strings.Contains(strings.ToLower(p), "cache"):
		return "Ephemeral — rebuilds on next run, no migration needed"

	case strings.Contains(p, "spool"):
		return "PVC / volume mount — spool data (mail, print, at jobs)"
	}

	return fmt.Sprintf("PVC / volume mount — %s, review application needs", category)
}

// ---------------------------------------------------------------------------
// ostree mount filtering
// ---------------------------------------------------------------------------

func filterOstreeMounts(mounts []schema.MountPoint) []schema.MountPoint {
	var filtered []schema.MountPoint
	for _, m := range mounts {
		if ostreeManagedMounts[m.Target] {
			continue
		}
		skip := false
		for _, prefix := range ostreeMountPrefixes {
			if strings.HasPrefix(m.Target, prefix) {
				skip = true
				break
			}
		}
		if !skip {
			filtered = append(filtered, m)
		}
	}
	if filtered == nil {
		filtered = []schema.MountPoint{}
	}
	return filtered
}
