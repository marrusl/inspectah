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
			HTML:          "<html>re-rendered</html>",
			Snapshot:      json.RawMessage(snapData),
			Containerfile: "FROM ubi9\nRUN echo re-rendered",
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
}

func TestNewHandler_ReRenderWithOriginal(t *testing.T) {
	dir := setupTestOutputDir(t)

	var capturedOrigData []byte
	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		capturedOrigData = origData
		return ReRenderResult{
			HTML:          "<html>ok</html>",
			Snapshot:      json.RawMessage(snapData),
			Containerfile: "FROM ubi9",
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
