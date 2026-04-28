package architect

import (
	"archive/tar"
	"compress/gzip"
	"io"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestNVRAToName(t *testing.T) {
	tests := []struct {
		input    string
		expected string
	}{
		{"httpd-2.4.57-5.el9.x86_64", "httpd"},
		{"mod_ssl-2.4.57-5.el9.x86_64", "mod_ssl"},
		{"postgresql-15.4-1.el9.aarch64", "postgresql"},
		{"bash-5.2.26-3.el9.noarch", "bash"},
		{"vim-enhanced-9.0.2153-1.el9.i686", "vim-enhanced"},
		{"kernel-core-5.14.0-427.el9.s390x", "kernel-core"},
		{"glibc-2.34-83.el9.ppc64le", "glibc"},
		{"simple", "simple"},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			assert.Equal(t, tt.expected, NVRAToName(tt.input))
		})
	}
}

func TestRenderContainerfile_BaseLayer(t *testing.T) {
	cf := RenderContainerfile("base", nil, []string{
		"httpd-2.4.57-5.el9.x86_64",
		"mod_ssl-2.4.57-5.el9.x86_64",
		"bash-5.2.26-3.el9.x86_64",
	}, "registry.redhat.io/rhel9/rhel-bootc:9.4")

	assert.Contains(t, cf, "FROM registry.redhat.io/rhel9/rhel-bootc:9.4")
	assert.Contains(t, cf, "RUN dnf install -y")
	assert.Contains(t, cf, "bash")
	assert.Contains(t, cf, "httpd")
	assert.Contains(t, cf, "mod_ssl")
	assert.Contains(t, cf, "&& dnf clean all")
}

func TestRenderContainerfile_DerivedLayer(t *testing.T) {
	parent := "base"
	cf := RenderContainerfile("web", &parent, []string{
		"httpd-2.4.57-5.el9.x86_64",
	}, "registry.redhat.io/rhel9/rhel-bootc:9.4")

	assert.Contains(t, cf, "FROM localhost/base:latest")
	assert.Contains(t, cf, "httpd")
	assert.NotContains(t, cf, "registry.redhat.io") // not in derived
}

func TestRenderContainerfile_NoPackages(t *testing.T) {
	cf := RenderContainerfile("empty", nil, nil, "registry.redhat.io/rhel9/rhel-bootc:9.4")

	assert.Contains(t, cf, "FROM registry.redhat.io/rhel9/rhel-bootc:9.4")
	assert.NotContains(t, cf, "RUN dnf install")
}

func TestRenderContainerfile_DeduplicatesPackageNames(t *testing.T) {
	// Same package name with different versions should only appear once
	cf := RenderContainerfile("test", nil, []string{
		"httpd-2.4.57-5.el9.x86_64",
		"httpd-2.4.57-5.el9.aarch64",
	}, "registry.redhat.io/rhel9/rhel-bootc:9.4")

	// Count occurrences of "httpd" in the dnf install line
	lines := strings.Split(cf, "\n")
	var installLine string
	for _, l := range lines {
		if strings.Contains(l, "dnf install") {
			installLine = l
		}
	}
	assert.Equal(t, 1, strings.Count(installLine+"\n"+strings.Join(lines, "\n"), "httpd"))
}

func TestRenderContainerfile_PackagesAreSorted(t *testing.T) {
	cf := RenderContainerfile("test", nil, []string{
		"zsh-5.9-el9.x86_64",
		"bash-5.2-el9.x86_64",
		"httpd-2.4-el9.x86_64",
	}, "base:latest")

	idx_bash := strings.Index(cf, "bash")
	idx_httpd := strings.Index(cf, "httpd")
	idx_zsh := strings.Index(cf, "zsh")

	assert.Greater(t, idx_bash, 0)
	assert.Greater(t, idx_httpd, idx_bash)
	assert.Greater(t, idx_zsh, idx_httpd)
}

func TestExportTopology(t *testing.T) {
	topo := makeTestTopology()
	baseImage := "registry.redhat.io/rhel9/rhel-bootc:9.4"

	data, err := ExportTopology(topo, baseImage)
	require.NoError(t, err)
	require.NotEmpty(t, data)

	// Verify tarball contents
	files := extractTarFiles(t, data)

	// Should have: base/Containerfile, web/Containerfile, db/Containerfile, build.sh
	assert.Contains(t, files, "base/Containerfile")
	assert.Contains(t, files, "web/Containerfile")
	assert.Contains(t, files, "db/Containerfile")
	assert.Contains(t, files, "build.sh")

	// Check base Containerfile
	assert.Contains(t, files["base/Containerfile"], "FROM registry.redhat.io/rhel9/rhel-bootc:9.4")

	// Check derived Containerfile
	assert.Contains(t, files["web/Containerfile"], "FROM localhost/base:latest")

	// Check build.sh
	assert.Contains(t, files["build.sh"], "#!/bin/bash")
	assert.Contains(t, files["build.sh"], "podman build -t localhost/base:latest base/")
	assert.Contains(t, files["build.sh"], "podman build -t localhost/web:latest web/")
	assert.Contains(t, files["build.sh"], "podman build -t localhost/db:latest db/")
}

func TestExportTopology_SingleFleet(t *testing.T) {
	fleets := []FleetInput{
		{
			Name:      "web",
			Packages:  []string{"httpd-2.4.57-5.el9.x86_64"},
			HostCount: 1,
		},
	}
	topo := AnalyzeFleets(fleets)

	data, err := ExportTopology(topo, "base:latest")
	require.NoError(t, err)

	files := extractTarFiles(t, data)
	assert.Contains(t, files, "web/Containerfile")
	assert.Contains(t, files, "build.sh")
	assert.NotContains(t, files, "base/Containerfile") // no base for single fleet
}

// --- helpers ---

func extractTarFiles(t *testing.T, data []byte) map[string]string {
	t.Helper()

	gz, err := gzip.NewReader(strings.NewReader(string(data)))
	require.NoError(t, err)
	defer gz.Close()

	tr := tar.NewReader(gz)
	files := make(map[string]string)

	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		require.NoError(t, err)

		content, err := io.ReadAll(tr)
		require.NoError(t, err)
		files[hdr.Name] = string(content)
	}

	return files
}
