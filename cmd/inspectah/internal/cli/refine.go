package cli

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/refine"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/renderer"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/spf13/cobra"
)

func newRefineCmd(_ *GlobalOpts) *cobra.Command {
	var (
		port      int
		noBrowser bool
	)

	cmd := &cobra.Command{
		Use:   "refine <tarball>",
		Short: "Serve the interactive report for operator refinement",
		Long: `Serve an inspectah tarball as an interactive web UI where operators
can toggle packages, configs, and services, then re-render the
Containerfile.`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			tarball, err := filepath.Abs(args[0])
			if err != nil {
				return fmt.Errorf("cannot resolve tarball path: %w", err)
			}

			return refine.RunRefine(refine.RunRefineOptions{
				TarballPath: tarball,
				Port:        port,
				NoBrowser:   noBrowser,
				ReRenderFn:  nativeReRender,
			})
		},
	}

	cmd.Flags().IntVar(&port, "port", 8642, "port for the refine server")
	cmd.Flags().BoolVar(&noBrowser, "no-browser", false, "do not open browser automatically")

	return cmd
}

// nativeReRender re-renders the output by loading the snapshot and running
// the renderer pipeline directly — no subprocess or container needed.
//
// Renders into a temp copy of the working directory. If rendering fails,
// the temp copy is discarded and the working directory is completely
// untouched (Kit R3: full working-dir protection). On success, renderer
// outputs are swapped into the working directory via syncRenderedOutput.
func nativeReRender(snapData []byte, origData []byte, outputDir string) (refine.ReRenderResult, error) {
	var snap schema.InspectionSnapshot
	if err := json.Unmarshal(snapData, &snap); err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("parse snapshot: %w", err)
	}

	// Sync user triage decisions into renderer-consumed structures.
	// Service toggles rebuild EnabledUnits/DisabledUnits from StateChanges;
	// cron toggles propagate CronJob.Include to GeneratedTimerUnits.
	renderer.SyncServiceDecisions(&snap)
	renderer.SyncCronDecisions(&snap)

	// Re-serialize so the exported snapshot agrees with rendered output.
	snapData, err := json.Marshal(snap)
	if err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("marshal sync'd snapshot: %w", err)
	}

	// Reconcile secret overrides: derive a redaction view that reflects
	// config file Include state. The canonical Redactions are preserved
	// for SPA binding; the reconciled view is used by renderers.
	//
	// "overridden" findings (user included a file the scanner excluded)
	// are dropped from the render-time slice — they should not appear
	// in counts, placeholders, or review listings. The canonical
	// Redactions (with original Kind values) are restored after rendering.
	reconciled := renderer.ReconcileSecretOverrides(&snap)
	var reconciledJSON []json.RawMessage
	for _, f := range reconciled {
		if f.Kind == "overridden" {
			continue
		}
		raw, _ := json.Marshal(f)
		reconciledJSON = append(reconciledJSON, raw)
	}
	canonicalRedactions := snap.Redactions
	snap.Redactions = reconciledJSON // renderers see reconciled view

	// Render into a temp copy — if rendering fails, the working directory
	// is completely untouched.
	renderDir, err := os.MkdirTemp("", "inspectah-render-")
	if err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("create render dir: %w", err)
	}
	defer os.RemoveAll(renderDir)

	// Copy working dir contents to temp render dir so
	// cleanRendererOutputs can selectively preserve only snapshot + sidecar.
	if err := copyDir(outputDir, renderDir); err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("copy working dir: %w", err)
	}

	// Clean all renderer-owned outputs from the temp dir BEFORE rendering.
	// This ensures the render starts from a clean state — no stale files
	// from a prior render survive into the new output. Only input data
	// (sidecar, snapshot) is preserved.
	cleanRendererOutputs(renderDir)

	// Write the sync'd (canonical) snapshot to render dir — renderers
	// read Redactions from the snap struct (currently set to reconciled),
	// not from the file, so the file gets canonical data.
	snapPath := filepath.Join(renderDir, "inspection-snapshot.json")
	if err := os.WriteFile(snapPath, snapData, 0644); err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("write snapshot: %w", err)
	}

	// Use server-owned sidecar from render dir
	origSnapPath := filepath.Join(renderDir, "original-inspection-snapshot.json")
	if _, err := os.Stat(origSnapPath); err != nil {
		origSnapPath = ""
	}

	// Run all renderers in the temp dir — fresh output, no stale artifacts.
	// snap.Redactions is set to the reconciled view so renderers see
	// overridden/excluded state without signature changes.
	if err := renderer.RunAll(&snap, renderDir, renderer.RunAllOptions{
		RefineMode:           true,
		OriginalSnapshotPath: origSnapPath,
	}); err != nil {
		// renderDir is cleaned up by defer — outputDir untouched
		snap.Redactions = canonicalRedactions // restore before returning
		return refine.ReRenderResult{}, fmt.Errorf("render: %w", err)
	}

	// Restore canonical redactions for the exported snapshot.
	snap.Redactions = canonicalRedactions

	// Rendering succeeded — replace ALL renderer-owned outputs in the
	// working directory with the rendered versions. This covers the full
	// output set from renderer.RunAll(): Containerfile, report.html,
	// inspection-snapshot.json, README.md, audit-report.md,
	// kickstart-suggestion.ks, secrets-review.md, merge-notes.md,
	// config/, redacted/, drop-ins/, quadlet/.
	//
	// Strategy: walk renderDir, skip the immutable sidecar, copy
	// everything else into outputDir via temp+rename per file.
	if err := syncRenderedOutput(renderDir, outputDir); err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("swap rendered output: %w", err)
	}

	htmlData, _ := os.ReadFile(filepath.Join(outputDir, "report.html"))
	containerfileData, _ := os.ReadFile(filepath.Join(outputDir, "Containerfile"))

	// Load the original (sidecar) snapshot for DefaultInclude computation.
	// Use the working dir's sidecar (not renderDir, which is cleaned up).
	var origSnap *schema.InspectionSnapshot
	sidecarPath := filepath.Join(outputDir, "original-inspection-snapshot.json")
	if s, err := schema.LoadSnapshot(sidecarPath); err == nil {
		origSnap = s
	}

	manifest := renderer.ClassifySnapshot(&snap, origSnap)
	manifestJSON, _ := json.Marshal(manifest)

	return refine.ReRenderResult{
		HTML:           string(htmlData),
		Snapshot:       json.RawMessage(snapData),
		Containerfile:  string(containerfileData),
		TriageManifest: json.RawMessage(manifestJSON),
	}, nil
}

// cleanRendererOutputs removes all renderer-owned files and
// directories from dir, preserving only the snapshot and sidecar.
// config/ IS renderer-owned — writeConfigTree() regenerates it
// from snapshot data during renderer.RunAll().
func cleanRendererOutputs(dir string) {
	preserved := map[string]bool{
		"inspection-snapshot.json":          true,
		"original-inspection-snapshot.json": true,
	}

	entries, _ := os.ReadDir(dir)
	for _, e := range entries {
		name := e.Name()
		if preserved[name] {
			continue
		}
		os.RemoveAll(filepath.Join(dir, name))
	}
}

// syncRenderedOutput replaces ALL content in dst with the rendered
// output from src. Skips the immutable sidecar. Removes stale files
// AND empty directories from dst that are absent in src.
func syncRenderedOutput(src, dst string) error {
	// Phase 1: Build inventory of files AND directories the new render produced.
	newPaths := make(map[string]bool)
	filepath.Walk(src, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil
		}
		rel, _ := filepath.Rel(src, path)
		if rel == "." {
			return nil
		}
		if filepath.Base(rel) == "original-inspection-snapshot.json" {
			return nil
		}
		newPaths[rel] = true
		return nil
	})

	// Phase 2: Copy all new render output into dst.
	if err := filepath.Walk(src, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		rel, _ := filepath.Rel(src, path)
		if rel == "." {
			return nil
		}
		if filepath.Base(rel) == "original-inspection-snapshot.json" {
			return nil
		}
		target := filepath.Join(dst, rel)
		if info.IsDir() {
			return os.MkdirAll(target, 0755)
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		if err := os.MkdirAll(filepath.Dir(target), 0755); err != nil {
			return err
		}
		tmp := target + ".tmp"
		if err := os.WriteFile(tmp, data, info.Mode()); err != nil {
			return err
		}
		return os.Rename(tmp, target)
	}); err != nil {
		return fmt.Errorf("copy rendered output: %w", err)
	}

	// Phase 3: Remove stale FILES from dst not in the new render.
	filepath.Walk(dst, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil
		}
		rel, _ := filepath.Rel(dst, path)
		if rel == "." || info.IsDir() {
			return nil
		}
		if filepath.Base(rel) == "original-inspection-snapshot.json" {
			return nil
		}
		if !newPaths[rel] {
			os.Remove(path)
		}
		return nil
	})

	// Phase 4: Remove stale empty DIRECTORIES from dst (bottom-up).
	// Walk in reverse depth order so parent dirs are removed after children.
	var dirs []string
	filepath.Walk(dst, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil
		}
		rel, _ := filepath.Rel(dst, path)
		if rel == "." || !info.IsDir() {
			return nil
		}
		if filepath.Base(rel) == "original-inspection-snapshot.json" {
			return nil
		}
		dirs = append(dirs, path)
		return nil
	})
	// Reverse so deepest directories are processed first
	for i := len(dirs) - 1; i >= 0; i-- {
		entries, _ := os.ReadDir(dirs[i])
		if len(entries) == 0 {
			os.Remove(dirs[i])
		}
	}

	return nil
}

// copyDir recursively copies src to dst, preserving file modes.
func copyDir(src, dst string) error {
	return filepath.Walk(src, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		rel, err := filepath.Rel(src, path)
		if err != nil {
			return err
		}
		if rel == "." {
			return nil
		}

		target := filepath.Join(dst, rel)
		if info.IsDir() {
			return os.MkdirAll(target, info.Mode())
		}

		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		return os.WriteFile(target, data, info.Mode())
	})
}
