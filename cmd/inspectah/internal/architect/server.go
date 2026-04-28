package architect

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os/exec"
	"runtime"
	"strings"
)

//go:embed static/architect.html
var htmlTemplate string

const (
	defaultPort = 8643
	maxPostBody = 1 * 1024 * 1024 // 1 MB
)

// ServerConfig holds the server configuration.
type ServerConfig struct {
	Topology    *LayerTopology
	BaseImage   string
	Bind        string
	Port        int
	OpenBrowser bool
}

// StartServer creates and starts the architect HTTP server.
// Returns the actual port and the server (caller calls Serve).
func StartServer(cfg ServerConfig) (int, *http.Server, error) {
	if cfg.Bind == "" {
		cfg.Bind = "127.0.0.1"
	}

	actualPort := cfg.Port
	if actualPort == 0 {
		actualPort = defaultPort
	}

	var err error
	actualPort, err = findFreePort(actualPort, 20)
	if err != nil {
		return 0, nil, err
	}

	// Render HTML with topology injected
	topoJSON, err := json.Marshal(cfg.Topology.ToDict())
	if err != nil {
		return 0, nil, fmt.Errorf("marshal topology: %w", err)
	}
	renderedHTML := strings.Replace(htmlTemplate, "TOPOLOGY_JSON_PLACEHOLDER", string(topoJSON), 1)

	mux := http.NewServeMux()
	handler := &architectHandler{
		topology:     cfg.Topology,
		baseImage:    cfg.BaseImage,
		renderedHTML: renderedHTML,
	}

	mux.HandleFunc("/", handler.handleRoot)
	mux.HandleFunc("/api/health", handler.handleHealth)
	mux.HandleFunc("/api/topology", handler.handleTopology)
	mux.HandleFunc("/api/export", handler.handleExport)
	mux.HandleFunc("/api/preview/", handler.handlePreview)
	mux.HandleFunc("/api/move", handler.handleMove)
	mux.HandleFunc("/api/copy", handler.handleCopy)

	addr := fmt.Sprintf("%s:%d", cfg.Bind, actualPort)
	srv := &http.Server{
		Addr:    addr,
		Handler: mux,
	}

	return actualPort, srv, nil
}

// OpenBrowser opens the default browser to the given URL.
func OpenBrowser(url string) error {
	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		cmd = exec.Command("open", url)
	case "linux":
		cmd = exec.Command("xdg-open", url)
	case "windows":
		cmd = exec.Command("cmd", "/c", "start", url)
	default:
		return fmt.Errorf("unsupported platform: %s", runtime.GOOS)
	}
	return cmd.Start()
}

type architectHandler struct {
	topology     *LayerTopology
	baseImage    string
	renderedHTML string
}

func (h *architectHandler) handleRoot(w http.ResponseWriter, r *http.Request) {
	path := r.URL.Path
	if path != "/" && path != "/index.html" {
		http.NotFound(w, r)
		return
	}
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Write([]byte(h.renderedHTML))
}

func (h *architectHandler) handleHealth(w http.ResponseWriter, r *http.Request) {
	sendJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

func (h *architectHandler) handleTopology(w http.ResponseWriter, r *http.Request) {
	sendJSON(w, http.StatusOK, h.topology.ToDict())
}

func (h *architectHandler) handleExport(w http.ResponseWriter, r *http.Request) {
	data, err := ExportTopology(h.topology, h.baseImage)
	if err != nil {
		sendJSON(w, http.StatusInternalServerError, map[string]string{"error": err.Error()})
		return
	}
	w.Header().Set("Content-Type", "application/gzip")
	w.Header().Set("Content-Disposition", `attachment; filename="architect-export.tar.gz"`)
	w.Write(data)
}

func (h *architectHandler) handlePreview(w http.ResponseWriter, r *http.Request) {
	layerName := strings.TrimPrefix(r.URL.Path, "/api/preview/")
	if layerName == "" {
		sendJSON(w, http.StatusBadRequest, map[string]string{"error": "layer name required"})
		return
	}

	layer := h.topology.GetLayer(layerName)
	if layer == nil {
		sendJSON(w, http.StatusNotFound, map[string]string{"error": fmt.Sprintf("layer %q not found", layerName)})
		return
	}

	content := RenderContainerfile(layer.Name, layer.Parent, layer.Packages, h.baseImage)
	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.Write([]byte(content))
}

func (h *architectHandler) handleMove(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		sendJSON(w, http.StatusMethodNotAllowed, map[string]string{"error": "POST required"})
		return
	}
	h.handlePkgOperation(w, r, h.topology.MovePackage)
}

func (h *architectHandler) handleCopy(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		sendJSON(w, http.StatusMethodNotAllowed, map[string]string{"error": "POST required"})
		return
	}
	h.handlePkgOperation(w, r, h.topology.CopyPackage)
}

type pkgRequest struct {
	Package string `json:"package"`
	From    string `json:"from"`
	To      string `json:"to"`
}

func (h *architectHandler) handlePkgOperation(w http.ResponseWriter, r *http.Request, op func(string, string, string) error) {
	if r.ContentLength > maxPostBody {
		sendJSON(w, http.StatusRequestEntityTooLarge, map[string]string{"error": "request body too large"})
		return
	}

	body, err := io.ReadAll(io.LimitReader(r.Body, maxPostBody))
	if err != nil {
		sendJSON(w, http.StatusBadRequest, map[string]string{"error": "read body: " + err.Error()})
		return
	}

	var req pkgRequest
	if err := json.Unmarshal(body, &req); err != nil {
		sendJSON(w, http.StatusBadRequest, map[string]string{"error": "invalid JSON: " + err.Error()})
		return
	}

	if err := op(req.Package, req.From, req.To); err != nil {
		sendJSON(w, http.StatusBadRequest, map[string]string{"error": err.Error()})
		return
	}

	sendJSON(w, http.StatusOK, h.topology.ToDict())
}

func sendJSON(w http.ResponseWriter, code int, data interface{}) {
	body, err := json.Marshal(data)
	if err != nil {
		log.Printf("marshal error: %v", err)
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	w.Write(body)
}

func findFreePort(start, maxAttempts int) (int, error) {
	for port := start; port < start+maxAttempts; port++ {
		ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", port))
		if err == nil {
			ln.Close()
			return port, nil
		}
	}
	return 0, fmt.Errorf("no free port found in range %d-%d", start, start+maxAttempts)
}
