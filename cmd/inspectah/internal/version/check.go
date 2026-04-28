package version

import (
	"fmt"
	"strings"
)

func NormalizeTag(input string) string {
	return strings.TrimPrefix(input, "v")
}

func ToImageRef(registry, repo, version string) string {
	tag := NormalizeTag(version)
	return fmt.Sprintf("%s/%s:%s", registry, repo, tag)
}

const (
	DefaultRegistry = "ghcr.io"
	DefaultRepo     = "marrusl/inspectah"
	DefaultTag      = "0.6.0"
)

func DefaultImageRef() string {
	return fmt.Sprintf("%s/%s:%s", DefaultRegistry, DefaultRepo, DefaultTag)
}
