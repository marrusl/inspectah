package refine

import (
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// setupTestOutputDir creates a temp directory with the required refine files.
func setupTestOutputDir(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()

	require.NoError(t, os.WriteFile(
		filepath.Join(dir, "report.html"),
		[]byte("<html><body>test report</body></html>"),
		0644,
	))

	snap := map[string]interface{}{
		"meta": map[string]interface{}{
			"hostname": "test-host",
		},
	}
	snapJSON, err := json.Marshal(snap)
	require.NoError(t, err)
	require.NoError(t, os.WriteFile(
		filepath.Join(dir, "inspection-snapshot.json"),
		snapJSON,
		0644,
	))

	require.NoError(t, os.WriteFile(
		filepath.Join(dir, "Containerfile"),
		[]byte("FROM ubi9\n"),
		0644,
	))

	return dir
}

func TestNewHandler_ServesReport(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil)

	tests := []struct {
		path        string
		wantStatus  int
		wantContain string
		wantType    string
	}{
		{"/", 200, "test report", "text/html"},
		{"/index.html", 200, "test report", "text/html"},
		{"/snapshot", 200, "test-host", "application/json"},
		{"/api/health", 200, "ok", "application/json"},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			req := httptest.NewRequest("GET", tt.path, nil)
			w := httptest.NewRecorder()
			handler.ServeHTTP(w, req)

			assert.Equal(t, tt.wantStatus, w.Code)
			assert.Contains(t, w.Body.String(), tt.wantContain)
			assert.Contains(t, w.Header().Get("Content-Type"), tt.wantType)
		})
	}
}

func TestNewHandler_CacheHeaders(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil)

	req := httptest.NewRequest("GET", "/", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, "no-cache, no-store, must-revalidate", w.Header().Get("Cache-Control"))
	assert.Equal(t, "no-cache", w.Header().Get("Pragma"))
	assert.Equal(t, "0", w.Header().Get("Expires"))
	assert.Equal(t, "*", w.Header().Get("Access-Control-Allow-Origin"))
}

func TestNewHandler_HealthEndpoint(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil)

	req := httptest.NewRequest("GET", "/api/health", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code)

	var body map[string]interface{}
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &body))
	assert.Equal(t, "ok", body["status"])
}

func TestNewHandler_TarballDownload(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil)

	req := httptest.NewRequest("GET", "/api/tarball", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code)
	assert.Equal(t, "application/gzip", w.Header().Get("Content-Type"))
	assert.Contains(t, w.Header().Get("Content-Disposition"), "inspectah-refined")

	// Verify it's a valid tarball by extracting
	tmpFile := filepath.Join(t.TempDir(), "download.tar.gz")
	require.NoError(t, os.WriteFile(tmpFile, w.Body.Bytes(), 0644))

	verifyDir := t.TempDir()
	err := ExtractTarball(tmpFile, verifyDir)
	require.NoError(t, err)
	assert.FileExists(t, filepath.Join(verifyDir, "report.html"))
}

func TestNewHandler_NotFound(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil)

	req := httptest.NewRequest("GET", "/nonexistent", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 404, w.Code)
}

func TestNewHandler_CORS(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil)

	req := httptest.NewRequest("OPTIONS", "/api/health", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code)
	assert.Equal(t, "*", w.Header().Get("Access-Control-Allow-Origin"))
	assert.Contains(t, w.Header().Get("Access-Control-Allow-Methods"), "GET")
	assert.Contains(t, w.Header().Get("Access-Control-Allow-Methods"), "POST")
}

func TestNewHandler_ReRender(t *testing.T) {
	dir := setupTestOutputDir(t)

	// Create a mock re-render function that just updates report.html
	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		// Write updated report
		os.WriteFile(filepath.Join(outputDir, "report.html"), []byte("<html>re-rendered</html>"), 0644)
		return ReRenderResult{
			HTML:           "<html>re-rendered</html>",
			Snapshot:       json.RawMessage(snapData),
			Containerfile:  "FROM ubi9\nRUN echo re-rendered",
			TriageManifest: json.RawMessage("[]"),
		}, nil
	}

	handler := newRefineHandler(dir, reRenderFn)

	// POST snapshot data to /api/render
	snapJSON := `{"meta":{"hostname":"test"}}`
	req := httptest.NewRequest("POST", "/api/render", strings.NewReader(snapJSON))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code)

	var result map[string]interface{}
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &result))
	assert.Contains(t, result["html"], "re-rendered")
	assert.Equal(t, "FROM ubi9\nRUN echo re-rendered", result["containerfile"])

	// Phase 2: verify render_id, revision, triage_manifest in response
	assert.NotEmpty(t, result["render_id"], "render response must include render_id")
	assert.NotNil(t, result["revision"], "render response must include revision")
	assert.NotNil(t, result["triage_manifest"], "render response must include triage_manifest")
}

func TestNewHandler_ReRenderWithOriginal(t *testing.T) {
	dir := setupTestOutputDir(t)

	var capturedOrigData []byte
	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		capturedOrigData = origData
		return ReRenderResult{
			HTML:           "<html>ok</html>",
			Snapshot:       json.RawMessage(snapData),
			Containerfile:  "FROM ubi9",
			TriageManifest: json.RawMessage("[]"),
		}, nil
	}

	handler := newRefineHandler(dir, reRenderFn)

	// POST with both snapshot and original
	payload := `{"snapshot":{"meta":{}},"original":{"meta":{"hostname":"orig"}}}`
	req := httptest.NewRequest("POST", "/api/render", strings.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code)
	assert.NotNil(t, capturedOrigData)
	assert.Contains(t, string(capturedOrigData), "orig")
}

func TestNewHandler_ReRenderNoFunction(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil) // no re-render function

	req := httptest.NewRequest("POST", "/api/render", strings.NewReader(`{}`))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 503, w.Code)
}

func TestNewHandler_StaticFile(t *testing.T) {
	dir := setupTestOutputDir(t)

	// Create a CSS file in the output dir
	require.NoError(t, os.WriteFile(filepath.Join(dir, "styles.css"), []byte("body { color: red; }"), 0644))

	handler := newRefineHandler(dir, nil)

	req := httptest.NewRequest("GET", "/styles.css", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code)
	assert.Contains(t, w.Body.String(), "color: red")
}

func TestValidateOutputDir(t *testing.T) {
	t.Run("valid dir", func(t *testing.T) {
		dir := setupTestOutputDir(t)
		err := validateOutputDir(dir)
		assert.NoError(t, err)
	})

	t.Run("missing files", func(t *testing.T) {
		dir := t.TempDir()
		err := validateOutputDir(dir)
		assert.Error(t, err)
		assert.Contains(t, err.Error(), "report.html")
	})

	t.Run("nested single subdir", func(t *testing.T) {
		dir := t.TempDir()
		inner := filepath.Join(dir, "inspectah-output")
		require.NoError(t, os.MkdirAll(inner, 0755))
		require.NoError(t, os.WriteFile(filepath.Join(inner, "report.html"), []byte("<html>"), 0644))
		require.NoError(t, os.WriteFile(filepath.Join(inner, "inspection-snapshot.json"), []byte(`{}`), 0644))

		err := validateOutputDir(dir)
		require.NoError(t, err)

		// Files should have been moved up
		assert.FileExists(t, filepath.Join(dir, "report.html"))
		assert.FileExists(t, filepath.Join(dir, "inspection-snapshot.json"))
	})
}

func TestFindFreePort(t *testing.T) {
	port, err := findFreePort(0)
	require.NoError(t, err)
	assert.Greater(t, port, 0)

	// The returned port should be usable
	ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", port))
	require.NoError(t, err)
	ln.Close()
}

func TestRunRefine_InvalidTarball(t *testing.T) {
	err := RunRefine(RunRefineOptions{
		TarballPath: "/nonexistent/file.tar.gz",
		Port:        0,
		NoBrowser:   true,
	})
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "not found")
}

func TestRunRefine_NotATarball(t *testing.T) {
	notTar := filepath.Join(t.TempDir(), "bad.tar.gz")
	require.NoError(t, os.WriteFile(notTar, []byte("not a tarball"), 0644))

	err := RunRefine(RunRefineOptions{
		TarballPath: notTar,
		Port:        0,
		NoBrowser:   true,
	})
	assert.Error(t, err)
}

func TestRunRefine_ServesOverHTTP(t *testing.T) {
	// Create a valid tarball
	files := map[string]string{
		"report.html":              "<html><body>integration test</body></html>",
		"inspection-snapshot.json": `{"meta":{"hostname":"test-host"}}`,
		"Containerfile":            "FROM ubi9\n",
	}
	tarball := createTestTarball(t, files)

	// Find a free port
	port, err := findFreePort(0)
	require.NoError(t, err)

	// Run the server in a goroutine
	errCh := make(chan error, 1)
	opts := RunRefineOptions{
		TarballPath: tarball,
		Port:        port,
		NoBrowser:   true,
		StopCh:      make(chan struct{}),
	}

	go func() {
		errCh <- RunRefine(opts)
	}()

	// Wait for server to be ready
	healthURL := fmt.Sprintf("http://127.0.0.1:%d/api/health", port)
	client := &http.Client{}
	ready := false
	for i := 0; i < 50; i++ {
		resp, err := client.Get(healthURL)
		if err == nil {
			resp.Body.Close()
			if resp.StatusCode == 200 {
				ready = true
				break
			}
		}
		// brief sleep via channel
		select {
		case <-errCh:
			t.Fatal("server exited early")
		default:
		}
	}
	require.True(t, ready, "server never became ready")

	// Test GET /
	resp, err := client.Get(fmt.Sprintf("http://127.0.0.1:%d/", port))
	require.NoError(t, err)
	body, _ := io.ReadAll(resp.Body)
	resp.Body.Close()
	assert.Equal(t, 200, resp.StatusCode)
	assert.Contains(t, string(body), "integration test")

	// Test GET /snapshot
	resp, err = client.Get(fmt.Sprintf("http://127.0.0.1:%d/snapshot", port))
	require.NoError(t, err)
	body, _ = io.ReadAll(resp.Body)
	resp.Body.Close()
	assert.Equal(t, 200, resp.StatusCode)
	assert.Contains(t, string(body), "test-host")

	// Stop server
	close(opts.StopCh)

	// Wait for server to finish
	select {
	case serverErr := <-errCh:
		assert.NoError(t, serverErr)
	}
}

// ── Phase 2 Tests ──

func TestAPISnapshot_GetRevision(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil)

	req := httptest.NewRequest("GET", "/api/snapshot", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code)

	var resp struct {
		Snapshot json.RawMessage `json:"snapshot"`
		Revision int             `json:"revision"`
	}
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &resp))
	assert.Equal(t, 1, resp.Revision)
	assert.Contains(t, string(resp.Snapshot), "test-host")
}

func TestAPISnapshot_PutAutosave(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil)

	// Save with correct revision
	body := `{"snapshot":{"meta":{"hostname":"updated"}},"revision":1}`
	req := httptest.NewRequest("PUT", "/api/snapshot", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code)

	var resp struct {
		Revision int `json:"revision"`
	}
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &resp))
	assert.Equal(t, 2, resp.Revision)

	// Verify snapshot was written to disk
	diskSnap, err := os.ReadFile(filepath.Join(dir, "inspection-snapshot.json"))
	require.NoError(t, err)
	assert.Contains(t, string(diskSnap), "updated")
}

func TestAPISnapshot_PutStaleRevision(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil)

	// Save with wrong revision — should get 409
	body := `{"snapshot":{"meta":{}},"revision":99}`
	req := httptest.NewRequest("PUT", "/api/snapshot", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 409, w.Code)
	assert.Contains(t, w.Body.String(), "stale revision")
}

func TestRenderAPI_RevisionAndRenderID(t *testing.T) {
	dir := setupTestOutputDir(t)

	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		os.WriteFile(filepath.Join(outputDir, "report.html"), []byte("<html>ok</html>"), 0644)
		return ReRenderResult{
			HTML: "<html>ok</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: "FROM ubi9", TriageManifest: json.RawMessage("[]"),
		}, nil
	}
	handler := newRefineHandler(dir, reRenderFn)

	// First render
	req := httptest.NewRequest("POST", "/api/render", strings.NewReader(`{"meta":{}}`))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)
	require.Equal(t, 200, w.Code)

	var resp1 struct {
		RenderID string  `json:"render_id"`
		Revision float64 `json:"revision"`
	}
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &resp1))
	assert.NotEmpty(t, resp1.RenderID)
	assert.Equal(t, float64(2), resp1.Revision) // starts at 1, increments to 2

	// Second render — different render_id, higher revision
	req2 := httptest.NewRequest("POST", "/api/render", strings.NewReader(`{"meta":{}}`))
	req2.Header.Set("Content-Type", "application/json")
	w2 := httptest.NewRecorder()
	handler.ServeHTTP(w2, req2)
	require.Equal(t, 200, w2.Code)

	var resp2 struct {
		RenderID string  `json:"render_id"`
		Revision float64 `json:"revision"`
	}
	require.NoError(t, json.Unmarshal(w2.Body.Bytes(), &resp2))
	assert.NotEqual(t, resp1.RenderID, resp2.RenderID)
	assert.Equal(t, float64(3), resp2.Revision)
}

func TestTarball_RenderIDGuard(t *testing.T) {
	dir := setupTestOutputDir(t)

	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		os.WriteFile(filepath.Join(outputDir, "report.html"), []byte("<html>ok</html>"), 0644)
		return ReRenderResult{
			HTML: "<html>ok</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: "FROM ubi9", TriageManifest: json.RawMessage("[]"),
		}, nil
	}
	handler := newRefineHandler(dir, reRenderFn)

	// Render to get a render_id
	renderReq := httptest.NewRequest("POST", "/api/render", strings.NewReader(`{"meta":{}}`))
	renderReq.Header.Set("Content-Type", "application/json")
	renderW := httptest.NewRecorder()
	handler.ServeHTTP(renderW, renderReq)
	require.Equal(t, 200, renderW.Code)

	var renderResp struct {
		RenderID string `json:"render_id"`
	}
	require.NoError(t, json.Unmarshal(renderW.Body.Bytes(), &renderResp))

	// Download with matching render_id — should succeed
	tarReq := httptest.NewRequest("GET", "/api/tarball?render_id="+renderResp.RenderID, nil)
	tarW := httptest.NewRecorder()
	handler.ServeHTTP(tarW, tarReq)
	assert.Equal(t, 200, tarW.Code)

	// Download with stale render_id — should get 409
	staleReq := httptest.NewRequest("GET", "/api/tarball?render_id=stale-id", nil)
	staleW := httptest.NewRecorder()
	handler.ServeHTTP(staleW, staleReq)
	assert.Equal(t, 409, staleW.Code)
	assert.Contains(t, staleW.Body.String(), "stale render_id")

	// Download without render_id — should still succeed (backward compat)
	noIDReq := httptest.NewRequest("GET", "/api/tarball", nil)
	noIDW := httptest.NewRecorder()
	handler.ServeHTTP(noIDW, noIDReq)
	assert.Equal(t, 200, noIDW.Code)
}

func TestTarball_ExcludesSidecar(t *testing.T) {
	dir := setupTestOutputDir(t)
	// Create sidecar file
	require.NoError(t, os.WriteFile(
		filepath.Join(dir, "original-inspection-snapshot.json"),
		[]byte(`{"sidecar":true}`), 0444))

	handler := newRefineHandler(dir, nil)

	req := httptest.NewRequest("GET", "/api/tarball", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)
	require.Equal(t, 200, w.Code)

	// Extract and verify sidecar is excluded
	tmpFile := filepath.Join(t.TempDir(), "download.tar.gz")
	require.NoError(t, os.WriteFile(tmpFile, w.Body.Bytes(), 0644))
	verifyDir := t.TempDir()
	require.NoError(t, ExtractTarball(tmpFile, verifyDir))

	assert.NoFileExists(t, filepath.Join(verifyDir, "original-inspection-snapshot.json"),
		"sidecar must be excluded from tarball")
	assert.FileExists(t, filepath.Join(verifyDir, "report.html"),
		"normal files must still be in tarball")
}

func TestSidecar_CreatedOnInit(t *testing.T) {
	dir := setupTestOutputDir(t)
	// Verify sidecar doesn't exist before handler creation
	sidecarPath := filepath.Join(dir, "original-inspection-snapshot.json")
	os.Remove(sidecarPath) // ensure clean state

	_ = newRefineHandler(dir, nil)

	// Sidecar should now exist
	assert.FileExists(t, sidecarPath, "sidecar must be created on handler init")
	sidecarData, err := os.ReadFile(sidecarPath)
	require.NoError(t, err)
	snapData, err := os.ReadFile(filepath.Join(dir, "inspection-snapshot.json"))
	require.NoError(t, err)
	assert.JSONEq(t, string(snapData), string(sidecarData),
		"sidecar must match original snapshot")
}

func TestRenderAPI_FailedRender_EntireWorkingDirUnchanged(t *testing.T) {
	dir := setupTestOutputDir(t)

	failingRender := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		return ReRenderResult{}, fmt.Errorf("renderer exploded")
	}
	handler := newRefineHandler(dir, failingRender)

	// Record full working-dir state AFTER handler init (which creates sidecar)
	beforeFiles := snapshotDirContents(t, dir)

	req := httptest.NewRequest("POST", "/api/render", strings.NewReader(`{"meta":{}}`))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 500, w.Code)

	// Entire working directory must be byte-identical
	afterFiles := snapshotDirContents(t, dir)
	assert.Equal(t, beforeFiles, afterFiles,
		"working directory must be completely unchanged after failed render")
}

func TestE2E_RenderEquality_ThreeWayProof(t *testing.T) {
	dir := setupTestOutputDir(t)

	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		os.WriteFile(filepath.Join(outputDir, "inspection-snapshot.json"), snapData, 0644)
		cf := "FROM ubi9\nRUN echo rendered"
		os.WriteFile(filepath.Join(outputDir, "Containerfile"), []byte(cf), 0644)
		os.WriteFile(filepath.Join(outputDir, "report.html"), []byte("<html>rendered</html>"), 0644)
		return ReRenderResult{
			HTML: "<html>rendered</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: cf, TriageManifest: json.RawMessage("[]"),
		}, nil
	}
	handler := newRefineHandler(dir, reRenderFn)

	// ── Step 1: POST /api/render ──
	renderReq := httptest.NewRequest("POST", "/api/render",
		strings.NewReader(`{"meta":{"hostname":"e2e"}}`))
	renderReq.Header.Set("Content-Type", "application/json")
	renderW := httptest.NewRecorder()
	handler.ServeHTTP(renderW, renderReq)
	require.Equal(t, 200, renderW.Code)

	var resp struct {
		RenderID       string          `json:"render_id"`
		Snapshot       json.RawMessage `json:"snapshot"`
		Containerfile  string          `json:"containerfile"`
		HTML           string          `json:"html"`
		TriageManifest json.RawMessage `json:"triage_manifest"`
	}
	require.NoError(t, json.Unmarshal(renderW.Body.Bytes(), &resp))

	// ── Step 2: Read working directory ──
	diskSnap, err := os.ReadFile(filepath.Join(dir, "inspection-snapshot.json"))
	require.NoError(t, err)
	diskCf, err := os.ReadFile(filepath.Join(dir, "Containerfile"))
	require.NoError(t, err)
	diskHTML, err := os.ReadFile(filepath.Join(dir, "report.html"))
	require.NoError(t, err)

	// ── Step 3: Extract tarball ──
	tarReq := httptest.NewRequest("GET", "/api/tarball?render_id="+resp.RenderID, nil)
	tarW := httptest.NewRecorder()
	handler.ServeHTTP(tarW, tarReq)
	require.Equal(t, 200, tarW.Code)

	tmpFile := filepath.Join(t.TempDir(), "e2e.tar.gz")
	os.WriteFile(tmpFile, tarW.Body.Bytes(), 0644)
	extractDir := t.TempDir()
	require.NoError(t, ExtractTarball(tmpFile, extractDir))

	tarSnap, err := os.ReadFile(filepath.Join(extractDir, "inspection-snapshot.json"))
	require.NoError(t, err)
	tarCf, err := os.ReadFile(filepath.Join(extractDir, "Containerfile"))
	require.NoError(t, err)
	tarHTML, err := os.ReadFile(filepath.Join(extractDir, "report.html"))
	require.NoError(t, err)

	// ══ PROOF: three-way equality for each artifact ══

	// Snapshot: response == disk == tarball
	t.Run("snapshot_response_eq_disk", func(t *testing.T) {
		assert.JSONEq(t, string(resp.Snapshot), string(diskSnap))
	})
	t.Run("snapshot_disk_eq_tarball", func(t *testing.T) {
		assert.JSONEq(t, string(diskSnap), string(tarSnap))
	})

	// Containerfile: response == disk == tarball
	t.Run("containerfile_response_eq_disk", func(t *testing.T) {
		assert.Equal(t, resp.Containerfile, string(diskCf))
	})
	t.Run("containerfile_disk_eq_tarball", func(t *testing.T) {
		assert.Equal(t, string(diskCf), string(tarCf))
	})

	// report.html: response == disk == tarball
	t.Run("html_response_eq_disk", func(t *testing.T) {
		assert.Equal(t, resp.HTML, string(diskHTML))
	})
	t.Run("html_disk_eq_tarball", func(t *testing.T) {
		assert.Equal(t, string(diskHTML), string(tarHTML))
	})

	// Exclusion: tarball must NOT contain sidecar or excluded files
	t.Run("tarball_excludes_sidecar", func(t *testing.T) {
		assert.NoFileExists(t, filepath.Join(extractDir, "original-inspection-snapshot.json"))
	})
}

// snapshotDirContents reads all files in dir recursively and returns
// a map of relative-path -> content for comparison.
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
