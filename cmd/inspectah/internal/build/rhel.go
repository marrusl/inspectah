package build

import (
	"regexp"
	"strings"
)

type Detection int

const (
	DetectionNonEntitled Detection = iota
	DetectionEntitled
	DetectionAmbiguous
)

var ubiPattern = regexp.MustCompile(`^registry\.redhat\.io/ubi[789]($|/.*)`)

func ClassifyBuild(containerfile string) Detection {
	argNames := parseARGNames(containerfile)
	stages := parseFROMs(containerfile, argNames)

	hasEntitled := false
	hasAmbiguous := false

	for _, img := range stages {
		switch classifyImage(img, argNames) {
		case DetectionEntitled:
			hasEntitled = true
		case DetectionAmbiguous:
			hasAmbiguous = true
		}
	}

	if hasEntitled {
		return DetectionEntitled
	}
	if hasAmbiguous {
		return DetectionAmbiguous
	}
	return DetectionNonEntitled
}

func parseARGNames(content string) map[string]bool {
	names := make(map[string]bool)
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if !strings.HasPrefix(strings.ToUpper(line), "ARG ") {
			continue
		}
		rest := strings.TrimSpace(line[4:])
		name := rest
		if idx := strings.Index(rest, "="); idx >= 0 {
			name = rest[:idx]
		}
		names[name] = true
	}
	return names
}

func parseFROMs(content string, argNames map[string]bool) []string {
	var images []string
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		upper := strings.ToUpper(line)
		if !strings.HasPrefix(upper, "FROM ") {
			continue
		}

		rest := strings.TrimSpace(line[5:])
		if strings.HasPrefix(rest, "--platform=") {
			parts := strings.Fields(rest)
			if len(parts) < 2 {
				continue
			}
			rest = strings.Join(parts[1:], " ")
		}

		img := strings.Fields(rest)[0]
		images = append(images, img)
	}
	return images
}

func classifyImage(img string, argNames map[string]bool) Detection {
	if strings.Contains(img, "${") || strings.Contains(img, "$") {
		return DetectionAmbiguous
	}

	if !strings.HasPrefix(img, "registry.redhat.io/") {
		return DetectionNonEntitled
	}

	repoPath := strings.TrimPrefix(img, "registry.redhat.io/")
	repoPath = strings.Split(repoPath, ":")[0]
	repoPath = strings.Split(repoPath, "@")[0]

	if ubiPattern.MatchString("registry.redhat.io/" + repoPath) {
		return DetectionNonEntitled
	}

	return DetectionEntitled
}
