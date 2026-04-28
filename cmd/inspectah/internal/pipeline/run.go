package pipeline

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/renderer"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// RunOptions configures the scan pipeline.
type RunOptions struct {
	HostRoot         string
	FromSnapshotPath string
	InspectOnly      bool
	OutputFile       string
	OutputDir        string
	NoSubscription   bool
	Sensitivity      string
	NoRedaction      bool
	Validate         bool
	PushToGitHub     string
	GitHubToken      string
	Public           bool
	SkipConfirmation bool

	// Inspector options
	ConfigDiffs    bool
	DeepBinaryScan bool
	QueryPodman    bool
	TargetVersion  string
	TargetImage    string
	NoBaseline     bool
	SkipPreflight  bool
	BaselinePackages string
	UserStrategy   string

	// Inspector function (injected by caller)
	RunInspectors func(hostRoot string) (*schema.InspectionSnapshot, error)
}

// Run executes the full scan pipeline: load/inspect -> redact -> render -> package.
func Run(opts RunOptions) (*schema.InspectionSnapshot, error) {
	var snap *schema.InspectionSnapshot

	// Step 1: Load or build snapshot
	if opts.FromSnapshotPath != "" {
		s, err := schema.LoadSnapshot(opts.FromSnapshotPath)
		if err != nil {
			return nil, fmt.Errorf("load snapshot: %w", err)
		}
		snap = s
	} else if opts.RunInspectors != nil {
		s, err := opts.RunInspectors(opts.HostRoot)
		if err != nil {
			return nil, fmt.Errorf("run inspectors: %w", err)
		}
		snap = s
	} else {
		return nil, fmt.Errorf("either --from-snapshot or inspectors required")
	}

	// Step 2: Redaction
	if !opts.NoRedaction {
		snap = RedactSnapshot(snap)
	} else {
		// Run detection without modifying, flag all findings
		// Make a copy for detection, keep original content
		snapJSON, _ := json.Marshal(snap)
		var detectSnap schema.InspectionSnapshot
		json.Unmarshal(snapJSON, &detectSnap)
		RedactSnapshot(&detectSnap)
		// Copy findings as flagged
		for _, raw := range detectSnap.Redactions {
			finding, err := schema.ParseRedaction(raw)
			if err != nil {
				snap.Redactions = append(snap.Redactions, raw)
				continue
			}
			finding.Kind = "flagged"
			data, _ := json.Marshal(finding)
			snap.Redactions = append(snap.Redactions, data)
		}
		if snap.Meta == nil {
			snap.Meta = make(map[string]interface{})
		}
		snap.Meta["_no_redaction"] = true
	}

	// Step 3: Heuristic pass
	if snap.Config != nil {
		for _, f := range snap.Config.Files {
			if !f.Include || f.Content == "" {
				continue
			}
			candidates := FindHeuristicCandidates(f.Content, f.Path)
			for _, c := range candidates {
				finding := schema.RedactionFinding{
					Path:            c.Path,
					Source:          "file",
					Kind:            "flagged",
					Pattern:         c.Reason,
					Line:            &c.Line,
					DetectionMethod: "heuristic",
					Confidence:      &c.Confidence,
				}
				// In strict mode, high-confidence heuristics get redacted
				if opts.Sensitivity == "strict" && c.Confidence == "high" && !opts.NoRedaction {
					finding.Kind = "inline"
				}
				data, _ := json.Marshal(finding)
				snap.Redactions = append(snap.Redactions, data)
			}
		}
	}

	// Step 4: Inspect-only mode — save snapshot and return
	if opts.InspectOnly {
		outPath := filepath.Join(".", "inspection-snapshot.json")
		if err := schema.SaveSnapshot(snap, outPath); err != nil {
			return nil, err
		}
		return snap, nil
	}

	// Step 5: Determine output directory
	tmpDir := opts.OutputDir
	useTmpDir := false
	if tmpDir == "" {
		var err error
		tmpDir, err = os.MkdirTemp("", "inspectah-")
		if err != nil {
			return nil, fmt.Errorf("create temp dir: %w", err)
		}
		useTmpDir = true
		defer func() {
			if useTmpDir {
				os.RemoveAll(tmpDir)
			}
		}()
	}
	os.MkdirAll(tmpDir, 0755)

	// Step 6: Save snapshot JSON
	if err := schema.SaveSnapshot(snap, filepath.Join(tmpDir, "inspection-snapshot.json")); err != nil {
		return nil, err
	}

	// Step 7: Run renderers
	if err := renderer.RunAll(snap, tmpDir, renderer.RunAllOptions{}); err != nil {
		return nil, fmt.Errorf("render: %w", err)
	}

	// Step 8: Bundle subscription certs
	if !opts.NoSubscription && opts.FromSnapshotPath == "" {
		BundleSubscriptionCerts(opts.HostRoot, tmpDir)
	}

	// Step 9: Create tarball if needed
	if opts.OutputDir == "" {
		hostname := ""
		if snap.Meta != nil {
			if h, ok := snap.Meta["hostname"].(string); ok {
				hostname = h
			}
		}
		stamp := GetOutputStamp(hostname)
		tarballName := fmt.Sprintf("inspectah-%s.tar.gz", stamp)

		tarballPath := tarballName
		if opts.OutputFile != "" {
			tarballPath = opts.OutputFile
		}

		if err := CreateTarball(tmpDir, tarballPath, "inspectah-"+stamp); err != nil {
			return nil, fmt.Errorf("create tarball: %w", err)
		}
		fmt.Fprintf(os.Stderr, "Output: %s\n", tarballPath)
		useTmpDir = true // cleanup temp dir
	} else {
		useTmpDir = false // keep the output dir
	}

	return snap, nil
}
