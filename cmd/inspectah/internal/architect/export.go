package architect

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"fmt"
	"sort"
	"strings"
)

// ExportTopology generates a .tar.gz containing a Containerfile per layer
// plus a build.sh script.
func ExportTopology(topo *LayerTopology, baseImage string) ([]byte, error) {
	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)

	for _, layer := range topo.Layers {
		cf := RenderContainerfile(layer.Name, layer.Parent, layer.Packages, baseImage)
		if err := addStringToTar(tw, layer.Name+"/Containerfile", cf); err != nil {
			return nil, fmt.Errorf("write Containerfile for %s: %w", layer.Name, err)
		}
	}

	buildSh := renderBuildSh(topo, baseImage)
	if err := addStringToTar(tw, "build.sh", buildSh); err != nil {
		return nil, fmt.Errorf("write build.sh: %w", err)
	}

	if err := tw.Close(); err != nil {
		return nil, fmt.Errorf("close tar: %w", err)
	}
	if err := gz.Close(); err != nil {
		return nil, fmt.Errorf("close gzip: %w", err)
	}

	return buf.Bytes(), nil
}

// NVRAToName extracts the package name from an NVRA string like
// "httpd-2.4.57-5.el9.x86_64" -> "httpd".
func NVRAToName(nvra string) string {
	// Remove arch suffix
	for _, arch := range []string{".x86_64", ".noarch", ".i686", ".aarch64", ".s390x", ".ppc64le"} {
		if strings.HasSuffix(nvra, arch) {
			nvra = nvra[:len(nvra)-len(arch)]
			break
		}
	}
	// Remove release (after last -)
	if idx := strings.LastIndex(nvra, "-"); idx > 0 {
		nvra = nvra[:idx]
	}
	// Remove version (after last -)
	if idx := strings.LastIndex(nvra, "-"); idx > 0 {
		nvra = nvra[:idx]
	}
	return nvra
}

// RenderContainerfile renders a Containerfile for a single layer.
func RenderContainerfile(layerName string, parent *string, packages []string, baseImage string) string {
	var lines []string

	if parent == nil {
		lines = append(lines, fmt.Sprintf("FROM %s", baseImage))
	} else {
		lines = append(lines, fmt.Sprintf("FROM localhost/%s:latest", *parent))
	}
	lines = append(lines, "")

	if len(packages) > 0 {
		// Extract bare package names from NVRAs and deduplicate
		nameSet := make(map[string]bool, len(packages))
		for _, pkg := range packages {
			nameSet[NVRAToName(pkg)] = true
		}
		pkgNames := make([]string, 0, len(nameSet))
		for n := range nameSet {
			pkgNames = append(pkgNames, n)
		}
		sort.Strings(pkgNames)

		pkgList := strings.Join(pkgNames, " \\\n    ")
		lines = append(lines, fmt.Sprintf("RUN dnf install -y \\\n    %s \\\n    && dnf clean all", pkgList))
		lines = append(lines, "")
	}

	return strings.Join(lines, "\n")
}

func renderBuildSh(topo *LayerTopology, baseImage string) string {
	lines := []string{
		"#!/bin/bash",
		"# Build base first, then derived images",
		"set -euo pipefail",
		"",
	}

	// Base first
	base := topo.GetLayer("base")
	if base != nil {
		lines = append(lines, "podman build -t localhost/base:latest base/")
	}

	// Then derived in order
	for _, layer := range topo.Layers {
		if layer.Parent != nil {
			lines = append(lines, fmt.Sprintf("podman build -t localhost/%s:latest %s/", layer.Name, layer.Name))
		}
	}

	lines = append(lines, "")
	return strings.Join(lines, "\n")
}

func addStringToTar(tw *tar.Writer, name, content string) error {
	data := []byte(content)
	hdr := &tar.Header{
		Name: name,
		Size: int64(len(data)),
		Mode: 0644,
	}
	if err := tw.WriteHeader(hdr); err != nil {
		return err
	}
	_, err := tw.Write(data)
	return err
}
