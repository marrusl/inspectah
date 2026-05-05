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

	"github.com/marrusl/inspectah/cmd/inspectah/internal/renderer"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
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

func TestAPISnapshot_PutMalformedSnapshot(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil)

	tests := []struct {
		name string
		body string
	}{
		{"missing snapshot field", `{"revision":1}`},
		{"null snapshot", `{"snapshot":null,"revision":1}`},
		{"snapshot is string not object", `{"snapshot":"not an object","revision":1}`},
		{"snapshot is array not object", `{"snapshot":[1,2,3],"revision":1}`},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req := httptest.NewRequest("PUT", "/api/snapshot", strings.NewReader(tt.body))
			req.Header.Set("Content-Type", "application/json")
			w := httptest.NewRecorder()
			handler.ServeHTTP(w, req)

			assert.Equal(t, 400, w.Code, "malformed snapshot must return 400")
		})
	}

	// Verify snapshot on disk is unchanged after all malformed requests
	diskSnap, err := os.ReadFile(filepath.Join(dir, "inspection-snapshot.json"))
	require.NoError(t, err)
	assert.Contains(t, string(diskSnap), "test-host",
		"snapshot on disk must not change after malformed PUT")
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

// ── Refine-server contract tests ──

func TestRefineServer_AcknowledgeDoesNotChangeArtifacts(t *testing.T) {
	// Acknowledging a display-only item must not change the rendered output.
	dir := setupTestOutputDir(t)

	// Seed the snapshot with a display-only NMConnection.
	snap := map[string]interface{}{
		"meta": map[string]interface{}{"hostname": "test-host"},
		"network": map[string]interface{}{
			"connections": []interface{}{
				map[string]interface{}{
					"name": "eth0", "type": "ethernet",
					"path": "/etc/NetworkManager/system-connections/eth0.nmconnection",
				},
			},
		},
	}
	snapJSON, err := json.Marshal(snap)
	require.NoError(t, err)
	require.NoError(t, os.WriteFile(filepath.Join(dir, "inspection-snapshot.json"), snapJSON, 0644))

	var lastContainerfile string
	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		cf := "FROM ubi9\nRUN echo hello"
		lastContainerfile = cf
		os.WriteFile(filepath.Join(outputDir, "report.html"), []byte("<html>ok</html>"), 0644)
		os.WriteFile(filepath.Join(outputDir, "Containerfile"), []byte(cf), 0644)
		os.WriteFile(filepath.Join(outputDir, "inspection-snapshot.json"), snapData, 0644)
		return ReRenderResult{
			HTML: "<html>ok</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: cf, TriageManifest: json.RawMessage("[]"),
		}, nil
	}
	handler := newRefineHandler(dir, reRenderFn)

	// Initial render.
	req := httptest.NewRequest("POST", "/api/render", strings.NewReader(string(snapJSON)))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)
	require.Equal(t, 200, w.Code)
	var resp1 struct{ Containerfile string `json:"containerfile"` }
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &resp1))
	cfBefore := resp1.Containerfile

	// Acknowledge the connection (set acknowledged=true).
	snap["network"].(map[string]interface{})["connections"] = []interface{}{
		map[string]interface{}{
			"name": "eth0", "type": "ethernet",
			"path":         "/etc/NetworkManager/system-connections/eth0.nmconnection",
			"acknowledged": true,
		},
	}
	ackJSON, err := json.Marshal(snap)
	require.NoError(t, err)

	// PUT the acknowledged snapshot.
	putBody := fmt.Sprintf(`{"snapshot":%s,"revision":2}`, string(ackJSON))
	putReq := httptest.NewRequest("PUT", "/api/snapshot", strings.NewReader(putBody))
	putReq.Header.Set("Content-Type", "application/json")
	putW := httptest.NewRecorder()
	handler.ServeHTTP(putW, putReq)
	require.Equal(t, 200, putW.Code)

	// Rebuild.
	req2 := httptest.NewRequest("POST", "/api/render", strings.NewReader(string(ackJSON)))
	req2.Header.Set("Content-Type", "application/json")
	w2 := httptest.NewRecorder()
	handler.ServeHTTP(w2, req2)
	require.Equal(t, 200, w2.Code)
	var resp2 struct{ Containerfile string `json:"containerfile"` }
	require.NoError(t, json.Unmarshal(w2.Body.Bytes(), &resp2))
	cfAfter := resp2.Containerfile

	assert.Equal(t, cfBefore, cfAfter,
		"acknowledging a display-only item must not change the Containerfile")
	_ = lastContainerfile
}

func TestRefineServer_NotificationAcknowledgePreservesTodo(t *testing.T) {
	// Acknowledging a notification (no-repo package) must not remove the
	// TODO comment from the Containerfile.
	dir := setupTestOutputDir(t)

	snap := map[string]interface{}{
		"meta": map[string]interface{}{"hostname": "test-host"},
		"rpm": map[string]interface{}{
			"packages_added": []interface{}{
				map[string]interface{}{
					"name": "custom-agent", "arch": "x86_64",
					"include": true, "state": "local_install",
				},
			},
		},
	}
	snapJSON, err := json.Marshal(snap)
	require.NoError(t, err)
	require.NoError(t, os.WriteFile(filepath.Join(dir, "inspection-snapshot.json"), snapJSON, 0644))

	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		// Simulate renderer: no-repo packages get a TODO comment.
		cf := "FROM ubi9\n# TODO: custom-agent has no known repo — install manually\n"
		os.WriteFile(filepath.Join(outputDir, "report.html"), []byte("<html>ok</html>"), 0644)
		os.WriteFile(filepath.Join(outputDir, "Containerfile"), []byte(cf), 0644)
		os.WriteFile(filepath.Join(outputDir, "inspection-snapshot.json"), snapData, 0644)
		return ReRenderResult{
			HTML: "<html>ok</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: cf, TriageManifest: json.RawMessage("[]"),
		}, nil
	}
	handler := newRefineHandler(dir, reRenderFn)

	// Initial render.
	req := httptest.NewRequest("POST", "/api/render", strings.NewReader(string(snapJSON)))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)
	require.Equal(t, 200, w.Code)
	var resp1 struct{ Containerfile string `json:"containerfile"` }
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &resp1))
	assert.Contains(t, resp1.Containerfile, "TODO",
		"initial render must contain a TODO for no-repo package")

	// Acknowledge the notification.
	snap["rpm"].(map[string]interface{})["packages_added"] = []interface{}{
		map[string]interface{}{
			"name": "custom-agent", "arch": "x86_64",
			"include": true, "state": "local_install",
			"acknowledged": true,
		},
	}
	ackJSON, err := json.Marshal(snap)
	require.NoError(t, err)

	// PUT acknowledged snapshot.
	putBody := fmt.Sprintf(`{"snapshot":%s,"revision":2}`, string(ackJSON))
	putReq := httptest.NewRequest("PUT", "/api/snapshot", strings.NewReader(putBody))
	putReq.Header.Set("Content-Type", "application/json")
	putW := httptest.NewRecorder()
	handler.ServeHTTP(putW, putReq)
	require.Equal(t, 200, putW.Code)

	// Rebuild.
	req2 := httptest.NewRequest("POST", "/api/render", strings.NewReader(string(ackJSON)))
	req2.Header.Set("Content-Type", "application/json")
	w2 := httptest.NewRecorder()
	handler.ServeHTTP(w2, req2)
	require.Equal(t, 200, w2.Code)
	var resp2 struct{ Containerfile string `json:"containerfile"` }
	require.NoError(t, json.Unmarshal(w2.Body.Bytes(), &resp2))

	assert.Contains(t, resp2.Containerfile, "TODO",
		"acknowledging a notification must NOT remove the TODO comment")
}

func TestRefineServer_GroupedTogglePreservesEqualityAfterRebuild(t *testing.T) {
	// Excluding all packages from a repo group and rebuilding must produce
	// three-way equality: API response == disk snapshot == tarball snapshot.
	dir := setupTestOutputDir(t)

	snap := map[string]interface{}{
		"meta": map[string]interface{}{"hostname": "test-host"},
		"rpm": map[string]interface{}{
			"packages_added": []interface{}{
				map[string]interface{}{
					"name": "vim", "arch": "x86_64",
					"include": true, "source_repo": "appstream",
				},
				map[string]interface{}{
					"name": "nano", "arch": "x86_64",
					"include": true, "source_repo": "appstream",
				},
			},
		},
	}
	snapJSON, err := json.Marshal(snap)
	require.NoError(t, err)
	require.NoError(t, os.WriteFile(filepath.Join(dir, "inspection-snapshot.json"), snapJSON, 0644))

	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		cf := "FROM ubi9\nRUN echo built"
		os.WriteFile(filepath.Join(outputDir, "report.html"), []byte("<html>ok</html>"), 0644)
		os.WriteFile(filepath.Join(outputDir, "Containerfile"), []byte(cf), 0644)
		os.WriteFile(filepath.Join(outputDir, "inspection-snapshot.json"), snapData, 0644)
		return ReRenderResult{
			HTML: "<html>ok</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: cf, TriageManifest: json.RawMessage("[]"),
		}, nil
	}
	handler := newRefineHandler(dir, reRenderFn)

	// Exclude both appstream packages.
	snap["rpm"].(map[string]interface{})["packages_added"] = []interface{}{
		map[string]interface{}{
			"name": "vim", "arch": "x86_64",
			"include": false, "source_repo": "appstream",
		},
		map[string]interface{}{
			"name": "nano", "arch": "x86_64",
			"include": false, "source_repo": "appstream",
		},
	}
	excludedJSON, err := json.Marshal(snap)
	require.NoError(t, err)

	// PUT excluded snapshot.
	putBody := fmt.Sprintf(`{"snapshot":%s,"revision":1}`, string(excludedJSON))
	putReq := httptest.NewRequest("PUT", "/api/snapshot", strings.NewReader(putBody))
	putReq.Header.Set("Content-Type", "application/json")
	putW := httptest.NewRecorder()
	handler.ServeHTTP(putW, putReq)
	require.Equal(t, 200, putW.Code)

	// Rebuild.
	renderReq := httptest.NewRequest("POST", "/api/render", strings.NewReader(string(excludedJSON)))
	renderReq.Header.Set("Content-Type", "application/json")
	renderW := httptest.NewRecorder()
	handler.ServeHTTP(renderW, renderReq)
	require.Equal(t, 200, renderW.Code)

	var renderResp struct {
		RenderID string          `json:"render_id"`
		Snapshot json.RawMessage `json:"snapshot"`
	}
	require.NoError(t, json.Unmarshal(renderW.Body.Bytes(), &renderResp))

	// GET /api/snapshot — must agree with render response.
	getReq := httptest.NewRequest("GET", "/api/snapshot", nil)
	getW := httptest.NewRecorder()
	handler.ServeHTTP(getW, getReq)
	require.Equal(t, 200, getW.Code)
	var getResp struct{ Snapshot json.RawMessage `json:"snapshot"` }
	require.NoError(t, json.Unmarshal(getW.Body.Bytes(), &getResp))

	// Read disk snapshot.
	diskSnap, err := os.ReadFile(filepath.Join(dir, "inspection-snapshot.json"))
	require.NoError(t, err)

	// Download tarball and extract snapshot.
	tarReq := httptest.NewRequest("GET", "/api/tarball?render_id="+renderResp.RenderID, nil)
	tarW := httptest.NewRecorder()
	handler.ServeHTTP(tarW, tarReq)
	require.Equal(t, 200, tarW.Code)
	tmpFile := filepath.Join(t.TempDir(), "grouped.tar.gz")
	os.WriteFile(tmpFile, tarW.Body.Bytes(), 0644)
	extractDir := t.TempDir()
	require.NoError(t, ExtractTarball(tmpFile, extractDir))
	tarSnap, err := os.ReadFile(filepath.Join(extractDir, "inspection-snapshot.json"))
	require.NoError(t, err)

	// Three-way equality proof.
	t.Run("api_response_eq_disk", func(t *testing.T) {
		assert.JSONEq(t, string(getResp.Snapshot), string(diskSnap))
	})
	t.Run("disk_eq_tarball", func(t *testing.T) {
		assert.JSONEq(t, string(diskSnap), string(tarSnap))
	})

	// Verify the exclude decision persisted in all three.
	for _, label := range []string{"api", "disk", "tarball"} {
		var data []byte
		switch label {
		case "api":
			data = getResp.Snapshot
		case "disk":
			data = diskSnap
		case "tarball":
			data = tarSnap
		}
		var s map[string]interface{}
		require.NoError(t, json.Unmarshal(data, &s))
		pkgs := s["rpm"].(map[string]interface{})["packages_added"].([]interface{})
		for _, p := range pkgs {
			pm := p.(map[string]interface{})
			assert.Equal(t, false, pm["include"],
				"%s: package %s should be excluded", label, pm["name"])
		}
	}
}

func TestRefineServer_AcknowledgeResumePersists(t *testing.T) {
	// Acknowledging a no-repo package and saving must persist to disk.
	dir := setupTestOutputDir(t)

	snap := map[string]interface{}{
		"meta": map[string]interface{}{"hostname": "test-host"},
		"rpm": map[string]interface{}{
			"packages_added": []interface{}{
				map[string]interface{}{
					"name": "custom-agent", "arch": "x86_64",
					"include": true, "state": "local_install",
				},
			},
		},
	}
	snapJSON, err := json.Marshal(snap)
	require.NoError(t, err)
	require.NoError(t, os.WriteFile(filepath.Join(dir, "inspection-snapshot.json"), snapJSON, 0644))

	handler := newRefineHandler(dir, nil)

	// Acknowledge the package.
	snap["rpm"].(map[string]interface{})["packages_added"] = []interface{}{
		map[string]interface{}{
			"name": "custom-agent", "arch": "x86_64",
			"include": true, "state": "local_install",
			"acknowledged": true,
		},
	}
	ackJSON, err := json.Marshal(snap)
	require.NoError(t, err)

	// PUT acknowledged snapshot with correct revision.
	putBody := fmt.Sprintf(`{"snapshot":%s,"revision":1}`, string(ackJSON))
	putReq := httptest.NewRequest("PUT", "/api/snapshot", strings.NewReader(putBody))
	putReq.Header.Set("Content-Type", "application/json")
	putW := httptest.NewRecorder()
	handler.ServeHTTP(putW, putReq)
	require.Equal(t, 200, putW.Code)

	// Read snapshot back from disk.
	diskData, err := os.ReadFile(filepath.Join(dir, "inspection-snapshot.json"))
	require.NoError(t, err)

	var diskSnap map[string]interface{}
	require.NoError(t, json.Unmarshal(diskData, &diskSnap))

	pkgs := diskSnap["rpm"].(map[string]interface{})["packages_added"].([]interface{})
	require.Len(t, pkgs, 1)
	pkg := pkgs[0].(map[string]interface{})
	assert.Equal(t, true, pkg["acknowledged"],
		"acknowledged=true must survive the PUT → disk write round-trip")
}

func TestRunRefine_LeafNormalization(t *testing.T) {
	// Build a schema-compliant snapshot with leaf packages and Include: false.
	// Use typed schema to ensure correct serialization (system_type, etc.).
	typedSnap := schema.NewSnapshot()
	typedSnap.Meta = map[string]interface{}{"hostname": "leaf-test"}
	leafNames := []string{"vim", "htop"}
	typedSnap.Rpm = &schema.RpmSection{
		LeafPackages: &leafNames,
		LeafDepTree: map[string]interface{}{
			"vim":  []interface{}{"vim-common", "gpm-libs"},
			"htop": nil,
		},
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "vim-common", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "gpm-libs", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "1.20", Release: "1.el9"},
			{Name: "htop", Arch: "x86_64", Include: false, SourceRepo: "epel", Version: "3.3", Release: "1.el9"},
		},
	}
	snapJSON, err := json.Marshal(typedSnap)
	require.NoError(t, err)

	files := map[string]string{
		"report.html":              "<html><body>leaf test</body></html>",
		"inspection-snapshot.json": string(snapJSON),
		"Containerfile":            "FROM ubi9\n",
	}
	tarball := createTestTarball(t, files)

	port, err := findFreePort(0)
	require.NoError(t, err)

	// Capture the output directory from the ReRenderFn callback.
	var capturedDir string
	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		capturedDir = outputDir
		os.WriteFile(filepath.Join(outputDir, "report.html"), []byte("<html>rendered</html>"), 0644)
		return ReRenderResult{
			HTML:           "<html>rendered</html>",
			Snapshot:       json.RawMessage(snapData),
			Containerfile:  "FROM ubi9\n",
			TriageManifest: json.RawMessage("[]"),
		}, nil
	}

	errCh := make(chan error, 1)
	opts := RunRefineOptions{
		TarballPath: tarball,
		Port:        port,
		NoBrowser:   true,
		ReRenderFn:  reRenderFn,
		StopCh:      make(chan struct{}),
	}

	go func() {
		errCh <- RunRefine(opts)
	}()

	// Wait for server to be ready.
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
		select {
		case <-errCh:
			t.Fatal("server exited early")
		default:
		}
	}
	require.True(t, ready, "server never became ready")

	// The ReRenderFn was called during init — capturedDir is the working dir.
	require.NotEmpty(t, capturedDir, "ReRenderFn must have been called during init")

	// ── Verify working snapshot has Include normalized to true for leaves ──
	workingData, err := os.ReadFile(filepath.Join(capturedDir, "inspection-snapshot.json"))
	require.NoError(t, err)

	var workingSnap map[string]interface{}
	require.NoError(t, json.Unmarshal(workingData, &workingSnap))

	workingPkgs := workingSnap["rpm"].(map[string]interface{})["packages_added"].([]interface{})
	for _, p := range workingPkgs {
		pm := p.(map[string]interface{})
		name := pm["name"].(string)
		if name == "vim" || name == "htop" {
			assert.Equal(t, true, pm["include"],
				"working snapshot: leaf package %s must have include=true after normalization", name)
		}
	}

	// ── Verify sidecar has the same normalized values ──
	sidecarData, err := os.ReadFile(filepath.Join(capturedDir, "original-inspection-snapshot.json"))
	require.NoError(t, err)

	var sidecarSnap map[string]interface{}
	require.NoError(t, json.Unmarshal(sidecarData, &sidecarSnap))

	sidecarPkgs := sidecarSnap["rpm"].(map[string]interface{})["packages_added"].([]interface{})
	for _, p := range sidecarPkgs {
		pm := p.(map[string]interface{})
		name := pm["name"].(string)
		if name == "vim" || name == "htop" {
			assert.Equal(t, true, pm["include"],
				"sidecar: leaf package %s must have include=true after normalization", name)
		}
	}

	// ── Verify ClassifySnapshot with working + sidecar produces DefaultInclude=true ──
	// Load typed snapshots for ClassifySnapshot.
	workingTyped, err := schema.LoadSnapshot(filepath.Join(capturedDir, "inspection-snapshot.json"))
	require.NoError(t, err)
	sidecarTyped, err := schema.LoadSnapshot(filepath.Join(capturedDir, "original-inspection-snapshot.json"))
	require.NoError(t, err)

	items := renderer.ClassifySnapshot(workingTyped, sidecarTyped)
	vimItem := findTriageItem(items, "pkg-vim-x86_64")
	require.NotNil(t, vimItem, "vim must appear in classified manifest")
	assert.True(t, vimItem.DefaultInclude,
		"ClassifySnapshot must produce DefaultInclude=true for leaf package vim")

	htopItem := findTriageItem(items, "pkg-htop-x86_64")
	require.NotNil(t, htopItem, "htop must appear in classified manifest")
	assert.True(t, htopItem.DefaultInclude,
		"ClassifySnapshot must produce DefaultInclude=true for leaf package htop")

	// Stop server.
	close(opts.StopCh)
	select {
	case serverErr := <-errCh:
		assert.NoError(t, serverErr)
	}
}

func findTriageItem(items []renderer.TriageItem, key string) *renderer.TriageItem {
	for i := range items {
		if items[i].Key == key {
			return &items[i]
		}
	}
	return nil
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

func TestRunRefine_IncludeDefaultsNormalization(t *testing.T) {
	// Build a schema-compliant single-machine snapshot with tier-2 surfaces
	// that all start with Include: false. After RunRefine normalizes,
	// working snapshot + sidecar + classifier must all agree on Include=true.
	typedSnap := schema.NewSnapshot()
	typedSnap.Meta = map[string]interface{}{"hostname": "include-defaults-test"}
	typedSnap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Kind: schema.ConfigFileKindRpmOwnedModified, Include: false},
		},
	}
	typedSnap.Services = &schema.ServiceSection{
		StateChanges: []schema.ServiceStateChange{
			{Unit: "httpd.service", CurrentState: "enabled", DefaultState: "disabled", Include: false},
			{Unit: "dnf-makecache.service", CurrentState: "enabled", DefaultState: "enabled", Include: true},
		},
		EnabledUnits: []string{"httpd.service", "dnf-makecache.service"},
	}
	typedSnap.ScheduledTasks = &schema.ScheduledTaskSection{
		CronJobs: []schema.CronJob{
			{Path: "/etc/cron.d/backup", Source: "custom", Include: false},
		},
	}
	typedSnap.Containers = &schema.ContainerSection{
		QuadletUnits: []schema.QuadletUnit{
			{Name: "webapp.container", Image: "webapp:latest", Include: false},
		},
	}
	typedSnap.Network = &schema.NetworkSection{
		FirewallZones: []schema.FirewallZone{
			{Name: "public", Path: "/etc/firewalld/zones/public.xml", Include: false},
		},
	}
	typedSnap.KernelBoot = &schema.KernelBootSection{
		SysctlOverrides: []schema.SysctlOverride{
			{Key: "vm.swappiness", Runtime: "10", Include: false},
		},
	}
	snapJSON, err := json.Marshal(typedSnap)
	require.NoError(t, err)

	files := map[string]string{
		"report.html":              "<html><body>include defaults test</body></html>",
		"inspection-snapshot.json": string(snapJSON),
		"Containerfile":            "FROM ubi9\n",
	}
	tarball := createTestTarball(t, files)

	port, err := findFreePort(0)
	require.NoError(t, err)

	var capturedDir string
	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		capturedDir = outputDir
		os.WriteFile(filepath.Join(outputDir, "report.html"), []byte("<html>rendered</html>"), 0644)
		return ReRenderResult{
			HTML:           "<html>rendered</html>",
			Snapshot:       json.RawMessage(snapData),
			Containerfile:  "FROM ubi9\n",
			TriageManifest: json.RawMessage("[]"),
		}, nil
	}

	errCh := make(chan error, 1)
	opts := RunRefineOptions{
		TarballPath: tarball,
		Port:        port,
		NoBrowser:   true,
		ReRenderFn:  reRenderFn,
		StopCh:      make(chan struct{}),
	}

	go func() {
		errCh <- RunRefine(opts)
	}()

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
		select {
		case <-errCh:
			t.Fatal("server exited early")
		default:
		}
	}
	require.True(t, ready, "server never became ready")
	require.NotEmpty(t, capturedDir, "ReRenderFn must have been called during init")

	// ── Verify working snapshot ──
	workingTyped, err := schema.LoadSnapshot(filepath.Join(capturedDir, "inspection-snapshot.json"))
	require.NoError(t, err)

	assert.True(t, workingTyped.Config.Files[0].Include,
		"working: config file must have Include=true")
	assert.True(t, workingTyped.Services.StateChanges[0].Include,
		"working: httpd.service must have Include=true")
	assert.False(t, workingTyped.Services.StateChanges[1].Include,
		"working: dnf-makecache.service must have Include=false (incompatible)")
	assert.True(t, workingTyped.ScheduledTasks.CronJobs[0].Include,
		"working: cron job must have Include=true")
	assert.True(t, workingTyped.Containers.QuadletUnits[0].Include,
		"working: quadlet must have Include=true")
	assert.True(t, workingTyped.Network.FirewallZones[0].Include,
		"working: firewall zone must have Include=true")
	assert.True(t, workingTyped.KernelBoot.SysctlOverrides[0].Include,
		"working: sysctl must have Include=true")

	// Incompatible service removed from EnabledUnits
	assert.NotContains(t, workingTyped.Services.EnabledUnits, "dnf-makecache.service",
		"working: incompatible service must be removed from EnabledUnits")

	// ── Verify sidecar matches working snapshot ──
	sidecarTyped, err := schema.LoadSnapshot(filepath.Join(capturedDir, "original-inspection-snapshot.json"))
	require.NoError(t, err)

	assert.True(t, sidecarTyped.Config.Files[0].Include,
		"sidecar: config file must have Include=true")
	assert.True(t, sidecarTyped.Services.StateChanges[0].Include,
		"sidecar: httpd.service must have Include=true")
	assert.False(t, sidecarTyped.Services.StateChanges[1].Include,
		"sidecar: dnf-makecache.service must have Include=false (incompatible)")

	// ── Verify ClassifySnapshot produces correct DefaultInclude ──
	items := renderer.ClassifySnapshot(workingTyped, sidecarTyped)

	cfgItem := findTriageItem(items, "cfg-/etc/httpd/conf/httpd.conf")
	require.NotNil(t, cfgItem, "config file must appear in classifier output")
	assert.True(t, cfgItem.DefaultInclude,
		"ClassifySnapshot must produce DefaultInclude=true for config file")

	svcItem := findTriageItem(items, "svc-httpd.service")
	require.NotNil(t, svcItem, "httpd.service must appear in classifier output")
	assert.True(t, svcItem.DefaultInclude,
		"ClassifySnapshot must produce DefaultInclude=true for httpd.service")

	dnfItem := findTriageItem(items, "svc-dnf-makecache.service")
	require.NotNil(t, dnfItem, "dnf-makecache.service must appear in classifier output")
	assert.False(t, dnfItem.DefaultInclude,
		"ClassifySnapshot must produce DefaultInclude=false for incompatible service")

	cronItem := findTriageItem(items, "cron-/etc/cron.d/backup")
	require.NotNil(t, cronItem, "cron job must appear in classifier output")
	assert.True(t, cronItem.DefaultInclude,
		"ClassifySnapshot must produce DefaultInclude=true for cron job")

	quadletItem := findTriageItem(items, "quadlet-webapp.container")
	require.NotNil(t, quadletItem, "quadlet must appear in classifier output")
	assert.True(t, quadletItem.DefaultInclude,
		"ClassifySnapshot must produce DefaultInclude=true for quadlet")

	fwItem := findTriageItem(items, "fw-public")
	require.NotNil(t, fwItem, "firewall zone must appear in classifier output")
	assert.True(t, fwItem.DefaultInclude,
		"ClassifySnapshot must produce DefaultInclude=true for firewall zone")

	sysctlItem := findTriageItem(items, "sysctl-vm.swappiness")
	require.NotNil(t, sysctlItem, "sysctl must appear in classifier output")
	assert.True(t, sysctlItem.DefaultInclude,
		"ClassifySnapshot must produce DefaultInclude=true for sysctl")

	// Stop server
	close(opts.StopCh)
	select {
	case serverErr := <-errCh:
		assert.NoError(t, serverErr)
	}
}

// setupTestOutputDirWithContainer creates a temp directory with a snapshot
// containing a running container named "webapp" for quadlet draft tests.
func setupTestOutputDirWithContainer(t *testing.T, containers []schema.RunningContainer, quadlets []schema.QuadletUnit) string {
	t.Helper()
	dir := t.TempDir()

	require.NoError(t, os.WriteFile(
		filepath.Join(dir, "report.html"),
		[]byte("<html><body>test report</body></html>"),
		0644,
	))

	snap := schema.NewSnapshot()
	snap.Meta = map[string]interface{}{"hostname": "quadlet-test"}
	snap.Containers = &schema.ContainerSection{
		RunningContainers: containers,
		QuadletUnits:      quadlets,
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

func TestHandleQuadletDraft(t *testing.T) {
	dir := setupTestOutputDirWithContainer(t,
		[]schema.RunningContainer{
			{
				Name:        "webapp",
				Image:       "registry.example.com/webapp:latest",
				InspectData: true,
				Ports: map[string]interface{}{
					"8080/tcp": []interface{}{
						map[string]interface{}{"HostIp": "0.0.0.0", "HostPort": "8080"},
					},
				},
			},
		},
		nil,
	)

	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		os.WriteFile(filepath.Join(outputDir, "report.html"), []byte("<html>drafted</html>"), 0644)
		return ReRenderResult{
			HTML:           "<html>drafted</html>",
			Snapshot:       json.RawMessage(snapData),
			Containerfile:  "FROM ubi9",
			TriageManifest: json.RawMessage(`[{"key":"quadlet-webapp.container"}]`),
		}, nil
	}

	handler := newRefineHandler(dir, reRenderFn)
	srv := httptest.NewServer(handler)
	defer srv.Close()

	body := strings.NewReader(`{"container_name":"webapp"}`)
	resp, err := http.Post(srv.URL+"/api/quadlet-draft", "application/json", body)
	require.NoError(t, err)
	defer resp.Body.Close()

	assert.Equal(t, 200, resp.StatusCode)

	respBody, err := io.ReadAll(resp.Body)
	require.NoError(t, err)

	var result map[string]interface{}
	require.NoError(t, json.Unmarshal(respBody, &result))

	// Verify ALL rebuild fields are present
	assert.NotEmpty(t, result["html"], "response must include html")
	assert.NotNil(t, result["snapshot"], "response must include snapshot")
	assert.NotEmpty(t, result["containerfile"], "response must include containerfile")
	assert.NotNil(t, result["triage_manifest"], "response must include triage_manifest")
	assert.NotEmpty(t, result["render_id"], "response must include render_id")
	assert.NotNil(t, result["revision"], "response must include revision")

	// Verify the generated unit was added to the snapshot on disk
	snap, err := schema.LoadSnapshot(filepath.Join(dir, "inspection-snapshot.json"))
	require.NoError(t, err)

	var found *schema.QuadletUnit
	for i := range snap.Containers.QuadletUnits {
		if snap.Containers.QuadletUnits[i].Name == "webapp.container" {
			found = &snap.Containers.QuadletUnits[i]
			break
		}
	}
	require.NotNil(t, found, "generated quadlet unit must exist in snapshot")
	assert.True(t, found.Generated, "generated quadlet must have Generated=true")
	assert.False(t, found.Include, "generated quadlet must have Include=false")
	assert.Equal(t, "registry.example.com/webapp:latest", found.Image)
	assert.NotEmpty(t, found.Content, "generated quadlet must have content")
}

func TestHandleQuadletDraft_DuplicateSuppression(t *testing.T) {
	dir := setupTestOutputDirWithContainer(t,
		[]schema.RunningContainer{
			{Name: "webapp", Image: "webapp:latest", InspectData: true},
		},
		[]schema.QuadletUnit{
			{Name: "webapp.container", Generated: true, Image: "webapp:latest"},
		},
	)

	handler := newRefineHandler(dir, nil)
	srv := httptest.NewServer(handler)
	defer srv.Close()

	body := strings.NewReader(`{"container_name":"webapp"}`)
	resp, err := http.Post(srv.URL+"/api/quadlet-draft", "application/json", body)
	require.NoError(t, err)
	defer resp.Body.Close()

	assert.Equal(t, 409, resp.StatusCode)

	respBody, err := io.ReadAll(resp.Body)
	require.NoError(t, err)
	assert.Contains(t, string(respBody), "already exists")

	var errResp map[string]string
	require.NoError(t, json.Unmarshal(respBody, &errResp), "409 response must be valid JSON")
	assert.NotEmpty(t, errResp["error"], "409 response should contain error message in JSON")
}

func TestHandleQuadletDraft_MissingImage(t *testing.T) {
	dir := setupTestOutputDirWithContainer(t,
		[]schema.RunningContainer{
			{Name: "noimage", Image: ""},
		},
		nil,
	)

	handler := newRefineHandler(dir, nil)
	srv := httptest.NewServer(handler)
	defer srv.Close()

	body := strings.NewReader(`{"container_name":"noimage"}`)
	resp, err := http.Post(srv.URL+"/api/quadlet-draft", "application/json", body)
	require.NoError(t, err)
	defer resp.Body.Close()

	assert.Equal(t, 422, resp.StatusCode)

	respBody, err := io.ReadAll(resp.Body)
	require.NoError(t, err)
	assert.Contains(t, string(respBody), "no image")

	var errResp map[string]string
	require.NoError(t, json.Unmarshal(respBody, &errResp), "422 response must be valid JSON")
	assert.NotEmpty(t, errResp["error"], "422 response should contain error message in JSON")
}

func TestHandleQuadletDraft_RefusesWithoutInspectData(t *testing.T) {
	dir := setupTestOutputDirWithContainer(t,
		[]schema.RunningContainer{
			{Name: "partial", Image: "app:latest", InspectData: false},
		},
		nil,
	)

	handler := newRefineHandler(dir, nil)
	srv := httptest.NewServer(handler)
	defer srv.Close()

	body := strings.NewReader(`{"container_name":"partial"}`)
	resp, err := http.Post(srv.URL+"/api/quadlet-draft", "application/json", body)
	require.NoError(t, err)
	defer resp.Body.Close()

	assert.Equal(t, 422, resp.StatusCode, "must return 422 for ps-only data")

	respBody, err := io.ReadAll(resp.Body)
	require.NoError(t, err)
	assert.Contains(t, string(respBody), "inspect data")

	var errResp map[string]string
	require.NoError(t, json.Unmarshal(respBody, &errResp), "422 response must be valid JSON")
	assert.NotEmpty(t, errResp["error"], "422 response should contain error message in JSON")
}

func TestRenderAPI_SystemTypeUnknown_GenericPath(t *testing.T) {
	dir := setupTestOutputDir(t)

	handler := newRefineHandler(dir, func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		var snap schema.InspectionSnapshot
		if err := json.Unmarshal(snapData, &snap); err != nil {
			return ReRenderResult{}, fmt.Errorf("parse snapshot: %w", err)
		}
		return ReRenderResult{
			HTML: "<html>generic</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: "FROM ubi9\n", TriageManifest: json.RawMessage("[]"),
		}, nil
	})

	req := httptest.NewRequest("POST", "/api/render",
		strings.NewReader(`{"snapshot": {"meta":{"hostname":"test"},"system_type":""}}`))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code,
		"empty system_type must not cause render failure after SystemTypeUnknown fix")
}

func TestHandleQuadletDraft_BlocksRealQuadletCollision(t *testing.T) {
	dir := setupTestOutputDirWithContainer(t,
		[]schema.RunningContainer{
			{Name: "webapp", Image: "webapp:latest", InspectData: true},
		},
		[]schema.QuadletUnit{
			{Name: "webapp.container", Content: "real unit", Generated: false},
		},
	)

	handler := newRefineHandler(dir, nil)
	srv := httptest.NewServer(handler)
	defer srv.Close()

	body := strings.NewReader(`{"container_name":"webapp"}`)
	resp, err := http.Post(srv.URL+"/api/quadlet-draft", "application/json", body)
	require.NoError(t, err)
	defer resp.Body.Close()

	assert.Equal(t, 409, resp.StatusCode)

	respBody, err := io.ReadAll(resp.Body)
	require.NoError(t, err)
	assert.Contains(t, string(respBody), "already exists")
}

func TestRenderAPI_MalformedSnapshot_Rejected(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		t.Fatal("reRenderFn must not be called for malformed snapshots")
		return ReRenderResult{}, nil
	})

	beforeFiles := snapshotDirContents(t, dir)

	req := httptest.NewRequest("POST", "/api/render",
		strings.NewReader(`{"snapshot": {"not_valid": true}}`))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 400, w.Code)
	var body map[string]interface{}
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &body))
	assert.Contains(t, body["error"], "missing meta")

	afterFiles := snapshotDirContents(t, dir)
	assert.Equal(t, beforeFiles, afterFiles,
		"working directory must be unchanged after malformed render rejection")
}

func TestRenderAPI_EmptyMeta_Accepted(t *testing.T) {
	dir := setupTestOutputDir(t)
	renderCalled := false
	handler := newRefineHandler(dir, func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		renderCalled = true
		return ReRenderResult{
			HTML: "<html>ok</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: "FROM ubi9\n", TriageManifest: json.RawMessage("[]"),
		}, nil
	})

	req := httptest.NewRequest("POST", "/api/render",
		strings.NewReader(`{"snapshot": {"meta": {}}}`))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code)
	assert.True(t, renderCalled, "reRenderFn must be called for valid snapshots")
}
