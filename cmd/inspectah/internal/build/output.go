package build

import "fmt"

func FormatSuccess(tag string) string {
	return fmt.Sprintf(`Built: %s

Next steps:
  Test:   bcvk ephemeral run-ssh %s
  Switch: bootc switch %s
  Push:   podman push %s <registry>/%s`,
		tag, tag, tag, tag, stripLocalhost(tag))
}

func FormatMissingPodman() string {
	return `Error: podman not found
  Linux:  sudo dnf install podman
  macOS:  brew install podman && podman machine init && podman machine start`
}

func stripLocalhost(tag string) string {
	if len(tag) > 10 && tag[:10] == "localhost/" {
		return tag[10:]
	}
	return tag
}
