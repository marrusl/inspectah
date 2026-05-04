// Package refine implements the interactive refinement server for inspectah
// output. It extracts a scan output tarball, serves the HTML report over HTTP,
// and handles live re-rendering when the snapshot is modified.
package refine

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/renderer"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

const (
	defaultPort   = 8642
	cacheControl  = "no-cache, no-store, must-revalidate"
	pragma        = "no-cache"
	expires       = "0"
	logPrefix     = "inspectah refine"
)

var requiredFiles = []string{"report.html", "inspection-snapshot.json"}

// ReRenderResult holds the output of a re-render operation.
type ReRenderResult struct {
	HTML           string          `json:"html"`
	Snapshot       json.RawMessage `json:"snapshot"`
	Containerfile  string          `json:"containerfile"`
	TriageManifest json.RawMessage `json:"triage_manifest"`
}

// ReRenderFunc is a function that re-renders the output from snapshot data.
// snapData is the edited snapshot JSON. origData is the optional original
// snapshot for diff display. outputDir is the directory to write results to.
type ReRenderFunc func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error)

// RunRefineOptions configures the refine server.
type RunRefineOptions struct {
	TarballPath string
	Port        int
	NoBrowser   bool

	// ReRenderFn is called when the client requests a re-render.
	// If nil, re-rendering is disabled.
	ReRenderFn ReRenderFunc

	// StopCh, when closed, stops the server. Used for testing.
	// If nil, the server runs until interrupted.
	StopCh chan struct{}
}

// RunRefine extracts a tarball and serves the interactive report.
func RunRefine(opts RunRefineOptions) error {
	// Validate tarball exists
	if _, err := os.Stat(opts.TarballPath); err != nil {
		return fmt.Errorf("tarball not found: %s", opts.TarballPath)
	}

	// Verify it's a valid gzip file by attempting extraction to a temp dir
	tmpDir, err := os.MkdirTemp("", "inspectah-refine-")
	if err != nil {
		return fmt.Errorf("create temp dir: %w", err)
	}
	defer os.RemoveAll(tmpDir)

	logf("extracting %s ...", opts.TarballPath)
	if err := ExtractTarball(opts.TarballPath, tmpDir); err != nil {
		return fmt.Errorf("failed to extract tarball: %w", err)
	}

	if err := validateOutputDir(tmpDir); err != nil {
		return err
	}

	// Count files
	fileCount := 0
	filepath.Walk(tmpDir, func(_ string, info os.FileInfo, _ error) error {
		if info != nil && !info.IsDir() {
			fileCount++
		}
		return nil
	})

	// Read hostname from snapshot
	hostLabel := ""
	snapPath := filepath.Join(tmpDir, "inspection-snapshot.json")
	if data, err := os.ReadFile(snapPath); err == nil {
		var snap map[string]interface{}
		if json.Unmarshal(data, &snap) == nil {
			if meta, ok := snap["meta"].(map[string]interface{}); ok {
				if h, ok := meta["hostname"].(string); ok {
					hostLabel = h
				}
			}
		}
	}

	// Normalize snapshot BEFORE sidecar — leaf defaults, surface defaults,
	// and nil *bool fixes. Both the sidecar and working copy must agree on
	// initial include state.
	if snap, err := schema.LoadSnapshot(snapPath); err == nil {
		renderer.NormalizeLeafDefaults(snap)
		_, isFleet := snap.Meta["fleet"]
		renderer.NormalizeIncludeDefaults(snap, isFleet)
		schema.NormalizeSnapshot(snap)
		schema.SaveSnapshot(snap, snapPath)
	}

	// Create immutable sidecar from the NORMALIZED snapshot
	sidecarPath := filepath.Join(tmpDir, "original-inspection-snapshot.json")
	if _, err := os.Stat(sidecarPath); os.IsNotExist(err) {
		if snapData, err := os.ReadFile(snapPath); err == nil {
			os.WriteFile(sidecarPath, snapData, 0444)
		}
	}

	// Initial re-render if re-render function is available
	reRenderFn := opts.ReRenderFn
	if reRenderFn != nil {
		if snapData, err := os.ReadFile(snapPath); err == nil {
			logf("re-rendering with editor UI enabled...")
			if _, err := reRenderFn(snapData, nil, tmpDir); err != nil {
				logf("initial re-render failed, serving static report: %s", err)
			}
		}
	}

	// Resolve port
	port := opts.Port
	if port == 0 || port == defaultPort {
		var err error
		if port == 0 {
			port, err = findFreePort(0)
		} else {
			port, err = findFreePort(defaultPort)
		}
		if err != nil {
			return err
		}
	}

	handler := newRefineHandler(tmpDir, reRenderFn)

	server := &http.Server{
		Addr:    fmt.Sprintf("127.0.0.1:%d", port),
		Handler: handler,
	}

	cacheBuster := time.Now().Unix()
	url := fmt.Sprintf("http://localhost:%d?t=%d", port, cacheBuster)

	srcName := filepath.Base(opts.TarballPath)
	summary := fmt.Sprintf("%d files from %s", fileCount, srcName)
	if hostLabel != "" {
		summary += fmt.Sprintf("  [%s]", hostLabel)
	}

	rule := strings.Repeat("─", 42)
	logf("extracted %s", summary)
	logf(rule)
	logf("  Report: %s", url)
	logf(rule)
	logf("edit items in the browser, then click Re-render")
	logf("Ctrl+C to stop")

	// Handle shutdown
	shutdownCh := make(chan struct{})

	// Signal handler
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)

	go func() {
		select {
		case <-sigCh:
		case <-func() chan struct{} {
			if opts.StopCh != nil {
				return opts.StopCh
			}
			return make(chan struct{}) // never fires
		}():
		}
		signal.Stop(sigCh)
		server.Close()
		close(shutdownCh)
	}()

	// Open browser
	if !opts.NoBrowser {
		go func() {
			healthURL := fmt.Sprintf("http://127.0.0.1:%d/api/health", port)
			if waitForServer(healthURL, 30*time.Second) {
				browserURL := fmt.Sprintf("http://localhost:%d?t=%d", port, time.Now().Unix())
				openBrowser(browserURL)
			} else {
				logf("warning: server did not become ready; not opening browser")
			}
		}()
	}

	// Start serving
	if err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		return fmt.Errorf("server error: %w", err)
	}

	return nil
}

// refineHandler is the HTTP handler for the refine server.
type refineHandler struct {
	outputDir  string
	reRenderFn ReRenderFunc
	mux        *http.ServeMux

	mu       sync.Mutex
	revision int
	renderID string
}

func newRefineHandler(outputDir string, reRenderFn ReRenderFunc) http.Handler {
	h := &refineHandler{
		outputDir:  outputDir,
		reRenderFn: reRenderFn,
		mux:        http.NewServeMux(),
		revision:   1,
		renderID:   generateRenderID(),
	}

	// Create sidecar if it doesn't exist — immutable copy of the original snapshot
	sidecar := filepath.Join(outputDir, "original-inspection-snapshot.json")
	if _, err := os.Stat(sidecar); os.IsNotExist(err) {
		if snapData, err := os.ReadFile(filepath.Join(outputDir, "inspection-snapshot.json")); err == nil {
			os.WriteFile(sidecar, snapData, 0444)
		}
	}

	h.mux.HandleFunc("/", h.handleRoot)
	h.mux.HandleFunc("/index.html", h.handleRoot)
	h.mux.HandleFunc("/snapshot", h.handleSnapshot)
	h.mux.HandleFunc("/api/health", h.handleHealth)
	h.mux.HandleFunc("/api/snapshot", h.handleAPISnapshot)
	h.mux.HandleFunc("/api/tarball", h.handleTarball)
	h.mux.HandleFunc("/api/render", h.handleRender)

	return h
}

func (h *refineHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if r.Method == "OPTIONS" {
		h.handleOptions(w, r)
		return
	}

	// Check known routes first
	path := r.URL.Path
	switch {
	case path == "/" || path == "/index.html":
		h.handleRoot(w, r)
	case path == "/snapshot":
		h.handleSnapshot(w, r)
	case path == "/api/health":
		h.handleHealth(w, r)
	case path == "/api/snapshot":
		h.handleAPISnapshot(w, r)
	case path == "/api/tarball":
		h.handleTarball(w, r)
	case path == "/api/render":
		h.handleRender(w, r)
	default:
		// Try serving as a static file from the output directory
		h.handleStatic(w, r)
	}
}

func (h *refineHandler) addCacheHeaders(w http.ResponseWriter) {
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Cache-Control", cacheControl)
	w.Header().Set("Pragma", pragma)
	w.Header().Set("Expires", expires)
}

func (h *refineHandler) sendJSON(w http.ResponseWriter, code int, v interface{}) {
	data, err := json.Marshal(v)
	if err != nil {
		h.sendError(w, 500, "json marshal error: "+err.Error())
		return
	}
	h.addCacheHeaders(w)
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	w.Write(data)
}

func (h *refineHandler) sendFile(w http.ResponseWriter, path, contentType string) {
	data, err := os.ReadFile(path)
	if err != nil {
		h.sendError(w, 404, "file not found: "+filepath.Base(path))
		return
	}
	h.addCacheHeaders(w)
	w.Header().Set("Content-Type", contentType)
	w.WriteHeader(200)
	w.Write(data)
}

func (h *refineHandler) sendError(w http.ResponseWriter, code int, msg string) {
	h.addCacheHeaders(w)
	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.WriteHeader(code)
	w.Write([]byte(msg))
}

func (h *refineHandler) handleRoot(w http.ResponseWriter, _ *http.Request) {
	h.sendFile(w, filepath.Join(h.outputDir, "report.html"), "text/html; charset=utf-8")
}

func (h *refineHandler) handleSnapshot(w http.ResponseWriter, _ *http.Request) {
	h.sendFile(w, filepath.Join(h.outputDir, "inspection-snapshot.json"), "application/json")
}

func (h *refineHandler) handleHealth(w http.ResponseWriter, _ *http.Request) {
	reRender := h.reRenderFn != nil
	h.sendJSON(w, 200, map[string]interface{}{
		"status":    "ok",
		"re_render": reRender,
	})
}

func (h *refineHandler) handleAPISnapshot(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case "GET":
		h.mu.Lock()
		rev := h.revision
		h.mu.Unlock()

		snapData, err := os.ReadFile(filepath.Join(h.outputDir, "inspection-snapshot.json"))
		if err != nil {
			h.sendError(w, 500, "failed to read snapshot")
			return
		}
		h.sendJSON(w, 200, map[string]interface{}{
			"snapshot": json.RawMessage(snapData),
			"revision": rev,
		})

	case "PUT":
		body, err := io.ReadAll(r.Body)
		if err != nil {
			h.sendError(w, 400, "failed to read request body")
			return
		}
		defer r.Body.Close()

		var req struct {
			Snapshot json.RawMessage `json:"snapshot"`
			Revision int             `json:"revision"`
		}
		if err := json.Unmarshal(body, &req); err != nil {
			h.sendError(w, 400, "invalid JSON")
			return
		}

		// Validate snapshot is present and well-formed
		if len(req.Snapshot) == 0 || string(req.Snapshot) == "null" {
			h.sendError(w, 400, "missing or empty snapshot field")
			return
		}
		var validSnap schema.InspectionSnapshot
		if err := json.Unmarshal(req.Snapshot, &validSnap); err != nil {
			h.sendError(w, 400, "invalid snapshot: "+err.Error())
			return
		}

		h.mu.Lock()
		if req.Revision != h.revision {
			current := h.revision
			h.mu.Unlock()
			h.sendJSON(w, 409, map[string]interface{}{
				"error":            "stale revision",
				"current_revision": current,
			})
			return
		}

		snapPath := filepath.Join(h.outputDir, "inspection-snapshot.json")
		if err := os.WriteFile(snapPath, req.Snapshot, 0644); err != nil {
			h.mu.Unlock()
			h.sendError(w, 500, "failed to write snapshot")
			return
		}
		h.revision++
		newRev := h.revision
		h.mu.Unlock()

		h.sendJSON(w, 200, map[string]interface{}{
			"revision": newRev,
		})

	default:
		h.sendError(w, 405, "method not allowed")
	}
}

func (h *refineHandler) handleTarball(w http.ResponseWriter, r *http.Request) {
	// Enforce render_id guard — can't download stale tarball
	queryRenderID := r.URL.Query().Get("render_id")
	if queryRenderID != "" {
		h.mu.Lock()
		currentID := h.renderID
		h.mu.Unlock()
		if queryRenderID != currentID {
			h.sendJSON(w, 409, map[string]interface{}{
				"error":             "stale render_id",
				"current_render_id": currentID,
			})
			return
		}
	}

	// Build tarball to temp file, then serve
	tmpFile, err := os.CreateTemp("", "refine-tarball-*.tar.gz")
	if err != nil {
		h.sendError(w, 500, "failed to create temp file")
		return
	}
	tmpPath := tmpFile.Name()
	tmpFile.Close()
	defer os.Remove(tmpPath)

	if err := RepackTarballFiltered(h.outputDir, tmpPath); err != nil {
		h.sendError(w, 500, "failed to create tarball: "+err.Error())
		return
	}

	data, err := os.ReadFile(tmpPath)
	if err != nil {
		h.sendError(w, 500, "failed to read tarball")
		return
	}

	filename := fmt.Sprintf("inspectah-refined-%d.tar.gz", time.Now().Unix())
	h.addCacheHeaders(w)
	w.Header().Set("Content-Type", "application/gzip")
	w.Header().Set("Content-Disposition", fmt.Sprintf("attachment; filename=%q", filename))
	w.WriteHeader(200)
	w.Write(data)
}

func (h *refineHandler) handleRender(w http.ResponseWriter, r *http.Request) {
	if r.Method != "POST" {
		h.sendError(w, 405, "method not allowed")
		return
	}

	if h.reRenderFn == nil {
		h.sendError(w, 503, "re-rendering not available")
		return
	}

	body, err := io.ReadAll(r.Body)
	if err != nil {
		h.sendError(w, 400, "failed to read request body")
		return
	}
	defer r.Body.Close()

	// Parse body — may be a plain snapshot or {snapshot, original} wrapper
	var snapData, origData []byte

	var wrapper struct {
		Snapshot json.RawMessage `json:"snapshot"`
		Original json.RawMessage `json:"original"`
	}

	if err := json.Unmarshal(body, &wrapper); err == nil && len(wrapper.Snapshot) > 0 {
		snapData = wrapper.Snapshot
		if len(wrapper.Original) > 0 {
			origData = wrapper.Original
		}
	} else {
		snapData = body
	}

	result, err := h.reRenderFn(snapData, origData, h.outputDir)
	if err != nil {
		h.sendError(w, 500, err.Error())
		return
	}

	// Update revision and render_id on successful render
	h.mu.Lock()
	h.revision++
	h.renderID = generateRenderID()
	rev := h.revision
	rid := h.renderID
	h.mu.Unlock()

	h.sendJSON(w, 200, map[string]interface{}{
		"html":            result.HTML,
		"snapshot":        result.Snapshot,
		"containerfile":   result.Containerfile,
		"triage_manifest": result.TriageManifest,
		"render_id":       rid,
		"revision":        rev,
	})
}

func (h *refineHandler) handleOptions(w http.ResponseWriter, _ *http.Request) {
	h.addCacheHeaders(w)
	w.Header().Set("Access-Control-Allow-Methods", "GET, POST, PUT, OPTIONS")
	w.Header().Set("Access-Control-Allow-Headers", "Content-Type")
	w.WriteHeader(200)
}

// generateRenderID produces a short random hex string for render_id binding.
func generateRenderID() string {
	b := make([]byte, 8)
	rand.Read(b)
	return hex.EncodeToString(b)
}

func (h *refineHandler) handleStatic(w http.ResponseWriter, r *http.Request) {
	// Sanitize path to prevent directory traversal
	cleanPath := filepath.Clean(r.URL.Path)
	if strings.Contains(cleanPath, "..") {
		h.sendError(w, 403, "forbidden")
		return
	}

	// Strip leading slash
	relPath := strings.TrimPrefix(cleanPath, "/")
	filePath := filepath.Join(h.outputDir, relPath)

	// Only serve if it's within the output dir
	absFile, err := filepath.Abs(filePath)
	if err != nil {
		h.sendError(w, 404, "not found")
		return
	}
	absDir, _ := filepath.Abs(h.outputDir)
	if !strings.HasPrefix(absFile, absDir+string(os.PathSeparator)) {
		h.sendError(w, 403, "forbidden")
		return
	}

	info, err := os.Stat(filePath)
	if err != nil || info.IsDir() {
		h.sendError(w, 404, fmt.Sprintf("not found: %s", r.URL.Path))
		return
	}

	// Detect content type from extension
	contentType := detectContentType(relPath)
	h.sendFile(w, filePath, contentType)
}

// validateOutputDir checks that the required files exist, and if the
// tarball extracted into a single subdirectory, moves contents up.
func validateOutputDir(dir string) error {
	missing := missingRequired(dir)
	if len(missing) == 0 {
		return nil
	}

	// Check for a single subdirectory containing the files
	entries, err := os.ReadDir(dir)
	if err != nil {
		return fmt.Errorf("read output dir: %w", err)
	}

	var subdirs []os.DirEntry
	for _, e := range entries {
		if e.IsDir() {
			subdirs = append(subdirs, e)
		}
	}

	if len(subdirs) == 1 {
		inner := filepath.Join(dir, subdirs[0].Name())
		if len(missingRequired(inner)) == 0 {
			// Move all contents from inner dir to parent
			innerEntries, err := os.ReadDir(inner)
			if err != nil {
				return fmt.Errorf("read inner dir: %w", err)
			}
			for _, e := range innerEntries {
				src := filepath.Join(inner, e.Name())
				dst := filepath.Join(dir, e.Name())
				if err := os.Rename(src, dst); err != nil {
					return fmt.Errorf("move %s: %w", e.Name(), err)
				}
			}
			os.Remove(inner)
			return nil
		}
	}

	return fmt.Errorf("tarball is missing required file(s): %s", strings.Join(missing, ", "))
}

func missingRequired(dir string) []string {
	var missing []string
	for _, f := range requiredFiles {
		if _, err := os.Stat(filepath.Join(dir, f)); err != nil {
			missing = append(missing, f)
		}
	}
	return missing
}

// findFreePort returns a free TCP port at or above start.
// If start is 0, the OS picks a port.
func findFreePort(start int) (int, error) {
	if start == 0 {
		ln, err := net.Listen("tcp", "127.0.0.1:0")
		if err != nil {
			return 0, fmt.Errorf("find free port: %w", err)
		}
		port := ln.Addr().(*net.TCPAddr).Port
		ln.Close()
		return port, nil
	}

	const maxAttempts = 20
	for p := start; p < start+maxAttempts; p++ {
		ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", p))
		if err != nil {
			continue
		}
		ln.Close()
		return p, nil
	}
	return 0, fmt.Errorf("no free port found in range %d-%d", start, start+maxAttempts-1)
}

func waitForServer(url string, timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)
	client := &http.Client{Timeout: 500 * time.Millisecond}
	for time.Now().Before(deadline) {
		resp, err := client.Get(url)
		if err == nil {
			resp.Body.Close()
			if resp.StatusCode == 200 {
				return true
			}
		}
		time.Sleep(100 * time.Millisecond)
	}
	return false
}

func openBrowser(url string) {
	var cmd *exec.Cmd
	switch {
	case commandExists("open"):
		cmd = exec.Command("open", url)
	case commandExists("xdg-open"):
		cmd = exec.Command("xdg-open", url)
	default:
		fmt.Fprintf(os.Stderr, "  Open %s in your browser\n", url)
		return
	}
	cmd.Start()
}

func commandExists(name string) bool {
	_, err := exec.LookPath(name)
	return err == nil
}

func logf(format string, args ...interface{}) {
	fmt.Fprintf(os.Stderr, "[%s] %s\n", logPrefix, fmt.Sprintf(format, args...))
}

func detectContentType(path string) string {
	ext := strings.ToLower(filepath.Ext(path))
	switch ext {
	case ".html", ".htm":
		return "text/html; charset=utf-8"
	case ".css":
		return "text/css; charset=utf-8"
	case ".js":
		return "application/javascript"
	case ".json":
		return "application/json"
	case ".png":
		return "image/png"
	case ".jpg", ".jpeg":
		return "image/jpeg"
	case ".svg":
		return "image/svg+xml"
	case ".gif":
		return "image/gif"
	default:
		return "application/octet-stream"
	}
}
