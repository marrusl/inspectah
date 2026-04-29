package cli

import (
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/refine"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/renderer"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestNativeReRender_ProducesCanonicalOutput(t *testing.T) {
	// Set up a working directory with a valid snapshot and sidecar
	workDir := t.TempDir()
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", Version: "2.4", Release: "1", Arch: "x86_64",
				State: "installed", SourceRepo: "appstream", Include: true},
		},
	}
	snapData, err := json.Marshal(snap)
	require.NoError(t, err)
	require.NoError(t, os.WriteFile(filepath.Join(workDir, "inspection-snapshot.json"), snapData, 0644))
	require.NoError(t, os.WriteFile(filepath.Join(workDir, "original-inspection-snapshot.json"), snapData, 0444))

	// Call the real nativeReRender
	result, err := nativeReRender(snapData, nil, workDir)
	require.NoError(t, err)

	// Verify response contains all expected fields
	assert.NotEmpty(t, result.HTML)
	assert.NotEmpty(t, result.Containerfile)
	assert.NotEmpty(t, result.Snapshot)

	// ── Full output set verification ──
	// Render into a FRESH directory with the same snapshot to get the
	// ground-truth output set from renderer.RunAll(). Then compare
	// the working directory against it to prove the success-path swap
	// replaced ALL renderer-owned outputs, not just a subset.
	groundTruthDir := t.TempDir()
	require.NoError(t, os.WriteFile(filepath.Join(groundTruthDir, "inspection-snapshot.json"), snapData, 0644))
	// Copy sidecar to ground-truth dir so the render is refine-mode-equivalent
	sidecarSrc := filepath.Join(workDir, "original-inspection-snapshot.json")
	sidecarGT := filepath.Join(groundTruthDir, "original-inspection-snapshot.json")
	if data, err := os.ReadFile(sidecarSrc); err == nil {
		os.WriteFile(sidecarGT, data, 0444)
	}
	var groundSnap schema.InspectionSnapshot
	require.NoError(t, json.Unmarshal(snapData, &groundSnap))
	require.NoError(t, renderer.RunAll(&groundSnap, groundTruthDir, renderer.RunAllOptions{
		RefineMode:           true,
		OriginalSnapshotPath: sidecarGT,
	}))

	// Ground truth now contains every file renderer.RunAll() produces.
	groundFiles := snapshotDirContents(t, groundTruthDir)

	// The working directory must contain every file the ground truth has.
	// (It may also contain the sidecar, which ground truth lacks — that's fine.)
	workFiles := snapshotDirContents(t, workDir)
	for path, groundContent := range groundFiles {
		workContent, exists := workFiles[path]
		assert.True(t, exists, "working directory missing renderer output: %s", path)
		if exists {
			assert.Equal(t, groundContent, workContent,
				"working directory file %s differs from ground-truth render", path)
		}
	}

	// ── Response == disk for the three API-returned artifacts ──
	diskSnap, _ := os.ReadFile(filepath.Join(workDir, "inspection-snapshot.json"))
	assert.JSONEq(t, string(result.Snapshot), string(diskSnap))

	diskCf, _ := os.ReadFile(filepath.Join(workDir, "Containerfile"))
	assert.Equal(t, result.Containerfile, string(diskCf))

	diskHTML, _ := os.ReadFile(filepath.Join(workDir, "report.html"))
	assert.Equal(t, result.HTML, string(diskHTML))

	// ── Sidecar immutability ──
	sidecar, _ := os.ReadFile(filepath.Join(workDir, "original-inspection-snapshot.json"))
	assert.Equal(t, string(snapData), string(sidecar), "sidecar must be immutable")
}

func TestNativeReRender_FailurePreservesWorkingDir(t *testing.T) {
	workDir := t.TempDir()
	// Write initial state
	require.NoError(t, os.WriteFile(filepath.Join(workDir, "inspection-snapshot.json"), []byte("{}"), 0644))
	require.NoError(t, os.WriteFile(filepath.Join(workDir, "Containerfile"), []byte("FROM ubi9\n"), 0644))
	require.NoError(t, os.WriteFile(filepath.Join(workDir, "report.html"), []byte("<html>original</html>"), 0644))

	before := snapshotDirContents(t, workDir)

	// Call with invalid snapshot JSON — should fail
	_, err := nativeReRender([]byte("not valid json"), nil, workDir)
	assert.Error(t, err)

	// Working directory must be unchanged
	after := snapshotDirContents(t, workDir)
	assert.Equal(t, before, after, "working directory must be unchanged after failed render")
}

func TestNativeReRender_StaleArtifactRemoval(t *testing.T) {
	workDir := t.TempDir()

	// ── Render N: snapshot with config files + redactions ──
	// This produces TWO guaranteed-disappearing renderer-owned paths:
	// 1. redacted/etc/secret.conf.REDACTED (from WriteRedactedDir)
	// 2. config/etc/httpd/conf/httpd.conf (from writeConfigTree)
	snapN := schema.NewSnapshot()
	snapN.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", Version: "2.4", Release: "1", Arch: "x86_64",
				State: "installed", SourceRepo: "appstream", Include: true},
		},
	}
	snapN.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Kind: schema.ConfigFileKindRpmOwnedModified,
				Category: schema.ConfigCategoryOther, Content: "ServerRoot /etc/httpd", Include: true},
		},
	}
	snapN.Redactions = []json.RawMessage{
		json.RawMessage(`{"path":"/etc/secret.conf","source":"file","kind":"excluded","finding_type":"api_key","original":"REDACTED"}`),
	}
	snapDataN, err := json.Marshal(snapN)
	require.NoError(t, err)
	require.NoError(t, os.WriteFile(
		filepath.Join(workDir, "inspection-snapshot.json"), snapDataN, 0644))
	require.NoError(t, os.WriteFile(
		filepath.Join(workDir, "original-inspection-snapshot.json"), snapDataN, 0444))

	// Call the real nativeReRender for render N
	_, err = nativeReRender(snapDataN, nil, workDir)
	require.NoError(t, err)

	// ── NAMED ASSERTION: renderer-owned outputs exist after render N ──
	redactedFile := filepath.Join(workDir, "redacted", "etc", "secret.conf.REDACTED")
	assert.FileExists(t, redactedFile,
		"render N must produce redacted/etc/secret.conf.REDACTED")
	redactedDir := filepath.Join(workDir, "redacted")
	assert.DirExists(t, redactedDir,
		"render N must produce redacted/ directory")
	configFile := filepath.Join(workDir, "config", "etc", "httpd", "conf", "httpd.conf")
	assert.FileExists(t, configFile,
		"render N must produce config/etc/httpd/conf/httpd.conf")

	afterRenderN := snapshotDirContents(t, workDir)
	t.Logf("Files after render N: %v", mapKeys(afterRenderN))

	// ── Render N+1: snapshot WITHOUT redactions ──
	// WriteRedactedDir produces nothing → redacted/ must disappear entirely.
	snapN1 := schema.NewSnapshot()
	snapN1.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "nginx", Version: "1.24", Release: "1", Arch: "x86_64",
				State: "installed", SourceRepo: "appstream", Include: true},
		},
	}
	// No redactions
	snapDataN1, err := json.Marshal(snapN1)
	require.NoError(t, err)

	// Call the real nativeReRender for render N+1
	_, err = nativeReRender(snapDataN1, nil, workDir)
	require.NoError(t, err)

	afterRenderN1 := snapshotDirContents(t, workDir)
	t.Logf("Files after render N+1: %v", mapKeys(afterRenderN1))

	// ── NAMED ASSERTION: stale outputs are gone after render N+1 ──
	// Redacted artifacts (render N had redactions, render N+1 does not)
	assert.NoFileExists(t, redactedFile,
		"redacted/etc/secret.conf.REDACTED must disappear after render N+1")
	_, err = os.Stat(redactedDir)
	assert.True(t, os.IsNotExist(err),
		"redacted/ directory must be removed after render N+1 (empty dir cleanup)")

	// Config tree artifacts (render N had httpd config, render N+1 does not)
	assert.NoFileExists(t, configFile,
		"config/etc/httpd/conf/httpd.conf must disappear after render N+1")
	_, err = os.Stat(filepath.Join(workDir, "config", "etc", "httpd"))
	assert.True(t, os.IsNotExist(err),
		"config/etc/httpd/ directory must be removed after render N+1")

	// ── FULL SET PROOF: working dir == ground truth for render N+1 ──
	groundDir := t.TempDir()
	require.NoError(t, os.WriteFile(
		filepath.Join(groundDir, "inspection-snapshot.json"), snapDataN1, 0644))
	// Copy sidecar to ground-truth dir for refine-mode-equivalent render
	sidecarGT := filepath.Join(groundDir, "original-inspection-snapshot.json")
	sidecarData, _ := os.ReadFile(filepath.Join(workDir, "original-inspection-snapshot.json"))
	os.WriteFile(sidecarGT, sidecarData, 0444)

	var groundSnap schema.InspectionSnapshot
	require.NoError(t, json.Unmarshal(snapDataN1, &groundSnap))
	require.NoError(t, renderer.RunAll(&groundSnap, groundDir, renderer.RunAllOptions{
		RefineMode:           true,
		OriginalSnapshotPath: sidecarGT,
	}))
	groundFiles := snapshotDirContents(t, groundDir)

	// Working dir must contain ONLY ground-truth files + sidecar
	for path := range afterRenderN1 {
		if path == "original-inspection-snapshot.json" {
			continue
		}
		_, inGround := groundFiles[path]
		assert.True(t, inGround,
			"stale artifact %q survived render N+1 — must be removed", path)
	}

	for path, content := range groundFiles {
		workContent, exists := afterRenderN1[path]
		assert.True(t, exists, "missing expected file: %s", path)
		if exists {
			assert.Equal(t, content, workContent,
				"file %s differs from ground-truth render", path)
		}
	}

	// ── Sidecar must survive both renders ──
	sidecar, err := os.ReadFile(
		filepath.Join(workDir, "original-inspection-snapshot.json"))
	require.NoError(t, err)
	assert.Equal(t, string(snapDataN), string(sidecar),
		"sidecar must be immutable across multiple renders")

	// ── TARBALL PROOF: stale artifacts absent from exported tarball ──
	// Package the post-render-N+1 working directory as a tarball and
	// verify stale artifacts don't leak into the export.
	// Uses RepackTarballFiltered (same as server's /api/tarball endpoint).
	tarPath := filepath.Join(t.TempDir(), "test-refined.tar.gz")
	require.NoError(t, refine.RepackTarballFiltered(workDir, tarPath))

	tarExtractDir := t.TempDir()
	require.NoError(t, refine.ExtractTarball(tarPath, tarExtractDir))

	// Named assertions: stale artifacts must not be in the tarball
	_, tarRedactedErr := os.Stat(filepath.Join(tarExtractDir, "redacted"))
	assert.True(t, os.IsNotExist(tarRedactedErr),
		"stale redacted/ directory must not appear in exported tarball")
	assert.NoFileExists(t,
		filepath.Join(tarExtractDir, "redacted", "etc", "secret.conf.REDACTED"),
		"stale redacted file must not appear in exported tarball")

	// Stale config/ artifacts must not be in the tarball
	assert.NoFileExists(t,
		filepath.Join(tarExtractDir, "config", "etc", "httpd", "conf", "httpd.conf"),
		"stale config file must not appear in exported tarball")

	// Sidecar must also be excluded from tarball (per tarball allowlist)
	assert.NoFileExists(t,
		filepath.Join(tarExtractDir, "original-inspection-snapshot.json"),
		"sidecar must be excluded from exported tarball")
}

func snapshotDirContents(t *testing.T, dir string) map[string]string {
	t.Helper()
	contents := make(map[string]string)
	filepath.Walk(dir, func(path string, info os.FileInfo, err error) error {
		if err != nil || info.IsDir() {
			return nil
		}
		rel, _ := filepath.Rel(dir, path)
		data, _ := os.ReadFile(path)
		contents[rel] = string(data)
		return nil
	})
	return contents
}

func mapKeys(m map[string]string) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}
