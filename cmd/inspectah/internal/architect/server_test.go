package architect

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func newTestHandler() *architectHandler {
	topo := makeTestTopology()
	topoJSON, _ := json.Marshal(topo.ToDict())
	rendered := strings.Replace(htmlTemplate, "TOPOLOGY_JSON_PLACEHOLDER", string(topoJSON), 1)
	return &architectHandler{
		topology:     topo,
		baseImage:    "registry.redhat.io/rhel9/rhel-bootc:9.4",
		renderedHTML: rendered,
	}
}

func TestHandleRoot(t *testing.T) {
	h := newTestHandler()
	req := httptest.NewRequest("GET", "/", nil)
	w := httptest.NewRecorder()

	h.handleRoot(w, req)

	resp := w.Result()
	assert.Equal(t, http.StatusOK, resp.StatusCode)
	assert.Contains(t, resp.Header.Get("Content-Type"), "text/html")

	body, _ := io.ReadAll(resp.Body)
	assert.Contains(t, string(body), "inspectah Architect")
	assert.Contains(t, string(body), "base") // topology data injected
}

func TestHandleRoot_IndexHTML(t *testing.T) {
	h := newTestHandler()
	req := httptest.NewRequest("GET", "/index.html", nil)
	w := httptest.NewRecorder()

	h.handleRoot(w, req)
	assert.Equal(t, http.StatusOK, w.Result().StatusCode)
}

func TestHandleRoot_NotFound(t *testing.T) {
	h := newTestHandler()
	req := httptest.NewRequest("GET", "/nonexistent", nil)
	w := httptest.NewRecorder()

	h.handleRoot(w, req)
	assert.Equal(t, http.StatusNotFound, w.Result().StatusCode)
}

func TestHandleHealth(t *testing.T) {
	h := newTestHandler()
	req := httptest.NewRequest("GET", "/api/health", nil)
	w := httptest.NewRecorder()

	h.handleHealth(w, req)

	resp := w.Result()
	assert.Equal(t, http.StatusOK, resp.StatusCode)

	var data map[string]string
	json.NewDecoder(resp.Body).Decode(&data)
	assert.Equal(t, "ok", data["status"])
}

func TestHandleTopology(t *testing.T) {
	h := newTestHandler()
	req := httptest.NewRequest("GET", "/api/topology", nil)
	w := httptest.NewRecorder()

	h.handleTopology(w, req)

	resp := w.Result()
	assert.Equal(t, http.StatusOK, resp.StatusCode)
	assert.Contains(t, resp.Header.Get("Content-Type"), "application/json")

	var data map[string]interface{}
	json.NewDecoder(resp.Body).Decode(&data)
	layers, ok := data["layers"].([]interface{})
	require.True(t, ok)
	assert.Len(t, layers, 3) // base, web, db
}

func TestHandleExport(t *testing.T) {
	h := newTestHandler()
	req := httptest.NewRequest("GET", "/api/export", nil)
	w := httptest.NewRecorder()

	h.handleExport(w, req)

	resp := w.Result()
	assert.Equal(t, http.StatusOK, resp.StatusCode)
	assert.Equal(t, "application/gzip", resp.Header.Get("Content-Type"))
	assert.Contains(t, resp.Header.Get("Content-Disposition"), "architect-export.tar.gz")

	body, _ := io.ReadAll(resp.Body)
	assert.Greater(t, len(body), 0)
}

func TestHandlePreview(t *testing.T) {
	h := newTestHandler()
	req := httptest.NewRequest("GET", "/api/preview/base", nil)
	w := httptest.NewRecorder()

	h.handlePreview(w, req)

	resp := w.Result()
	assert.Equal(t, http.StatusOK, resp.StatusCode)
	assert.Contains(t, resp.Header.Get("Content-Type"), "text/plain")

	body, _ := io.ReadAll(resp.Body)
	assert.Contains(t, string(body), "FROM registry.redhat.io/rhel9/rhel-bootc:9.4")
}

func TestHandlePreview_DerivedLayer(t *testing.T) {
	h := newTestHandler()
	req := httptest.NewRequest("GET", "/api/preview/web", nil)
	w := httptest.NewRecorder()

	h.handlePreview(w, req)

	resp := w.Result()
	assert.Equal(t, http.StatusOK, resp.StatusCode)

	body, _ := io.ReadAll(resp.Body)
	assert.Contains(t, string(body), "FROM localhost/base:latest")
}

func TestHandlePreview_NotFound(t *testing.T) {
	h := newTestHandler()
	req := httptest.NewRequest("GET", "/api/preview/nonexistent", nil)
	w := httptest.NewRecorder()

	h.handlePreview(w, req)

	resp := w.Result()
	assert.Equal(t, http.StatusNotFound, resp.StatusCode)
}

func TestHandleMove(t *testing.T) {
	h := newTestHandler()
	body := `{"package":"httpd","from":"web","to":"db"}`
	req := httptest.NewRequest("POST", "/api/move", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()

	h.handleMove(w, req)

	resp := w.Result()
	assert.Equal(t, http.StatusOK, resp.StatusCode)

	var data map[string]interface{}
	json.NewDecoder(resp.Body).Decode(&data)
	layers := data["layers"].([]interface{})
	assert.Len(t, layers, 3)

	// Verify httpd moved from web to db
	for _, l := range layers {
		layer := l.(map[string]interface{})
		if layer["name"] == "web" {
			pkgs := layer["packages"].([]interface{})
			for _, p := range pkgs {
				assert.NotEqual(t, "httpd", p)
			}
		}
		if layer["name"] == "db" {
			pkgs := layer["packages"].([]interface{})
			found := false
			for _, p := range pkgs {
				if p == "httpd" {
					found = true
				}
			}
			assert.True(t, found)
		}
	}
}

func TestHandleMove_InvalidMethod(t *testing.T) {
	h := newTestHandler()
	req := httptest.NewRequest("GET", "/api/move", nil)
	w := httptest.NewRecorder()

	h.handleMove(w, req)
	assert.Equal(t, http.StatusMethodNotAllowed, w.Result().StatusCode)
}

func TestHandleMove_InvalidJSON(t *testing.T) {
	h := newTestHandler()
	req := httptest.NewRequest("POST", "/api/move", strings.NewReader("not json"))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()

	h.handleMove(w, req)
	assert.Equal(t, http.StatusBadRequest, w.Result().StatusCode)
}

func TestHandleMove_PackageNotFound(t *testing.T) {
	h := newTestHandler()
	body := `{"package":"nonexistent","from":"web","to":"db"}`
	req := httptest.NewRequest("POST", "/api/move", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()

	h.handleMove(w, req)
	assert.Equal(t, http.StatusBadRequest, w.Result().StatusCode)
}

func TestHandleCopy(t *testing.T) {
	h := newTestHandler()
	body := `{"package":"httpd","from":"web","to":"db"}`
	req := httptest.NewRequest("POST", "/api/copy", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()

	h.handleCopy(w, req)

	resp := w.Result()
	assert.Equal(t, http.StatusOK, resp.StatusCode)
}

func TestHandleCopy_InvalidMethod(t *testing.T) {
	h := newTestHandler()
	req := httptest.NewRequest("GET", "/api/copy", nil)
	w := httptest.NewRecorder()

	h.handleCopy(w, req)
	assert.Equal(t, http.StatusMethodNotAllowed, w.Result().StatusCode)
}

func TestHTMLTemplateContainsPlaceholder(t *testing.T) {
	// The raw template should contain our placeholder
	assert.Contains(t, htmlTemplate, "TOPOLOGY_JSON_PLACEHOLDER")
}

func TestFindFreePort(t *testing.T) {
	port, err := findFreePort(18643, 5)
	require.NoError(t, err)
	assert.GreaterOrEqual(t, port, 18643)
	assert.Less(t, port, 18648)
}
