// Package renderer — Quadlet draft generator for running containers.
//
// GenerateQuadletDraft produces a .container quadlet unit file from a
// RunningContainer snapshot. The output is a best-effort draft — it
// captures the container's image, ports, volumes, environment, networks,
// and restart policy, but leaves TODO comments for aspects that require
// human review (healthcheck, dependency ordering, user namespace).
package renderer

import (
	"fmt"
	"sort"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// skipEnvVars lists environment variable names that are always set by the
// container runtime and should be excluded from Quadlet output.
var skipEnvVars = map[string]bool{
	"PATH":     true,
	"HOME":     true,
	"HOSTNAME": true,
	"TERM":     true,
}

// GenerateQuadletDraft produces a .container Quadlet unit file draft from
// a running container's inspection data. The returned string is ready to
// be written to <name>.container.
func GenerateQuadletDraft(c schema.RunningContainer) string {
	var b strings.Builder

	// --- [Container] section ---
	b.WriteString("[Container]\n")
	b.WriteString(fmt.Sprintf("ContainerName=%s\n", c.Name))
	b.WriteString(fmt.Sprintf("Image=%s\n", c.Image))

	// Ports
	for _, pm := range extractPortMappings(c.Ports) {
		b.WriteString(fmt.Sprintf("PublishPort=%s\n", pm))
	}

	// Volumes from mounts
	for _, m := range c.Mounts {
		if m.Source == "" || m.Destination == "" {
			continue
		}
		vol := m.Source + ":" + m.Destination
		if !m.RW {
			vol += ":ro"
		}
		b.WriteString(fmt.Sprintf("Volume=%s\n", vol))
	}

	// Environment (skip runtime-injected vars)
	for _, envLine := range c.Env {
		key, _, ok := strings.Cut(envLine, "=")
		if !ok {
			continue
		}
		if skipEnvVars[key] {
			continue
		}
		b.WriteString(fmt.Sprintf("Environment=%s\n", envLine))
	}

	// Networks
	for _, name := range sortedNetworkNames(c.Networks) {
		b.WriteString(fmt.Sprintf("Network=%s\n", name))
	}

	// --- [Service] section ---
	b.WriteString("\n[Service]\n")

	switch c.RestartPolicy {
	case "always", "unless-stopped":
		b.WriteString("Restart=always\n")
	case "on-failure":
		b.WriteString("Restart=on-failure\n")
	case "":
		b.WriteString("# TODO: Set Restart= policy (always, on-failure, no)\n")
	default:
		b.WriteString(fmt.Sprintf("Restart=%s\n", c.RestartPolicy))
	}

	// --- Deferred TODO comments ---
	b.WriteString("# TODO: Add HealthCheck if the container defines one\n")
	b.WriteString("# TODO: Review dependency ordering (After=, Requires=)\n")
	b.WriteString("# TODO: Evaluate user namespace mapping (UserNS=)\n")

	// --- [Install] section ---
	b.WriteString("\n[Install]\n")
	b.WriteString("WantedBy=default.target\n")

	return b.String()
}

// extractPortMappings parses the Ports map from podman inspect into
// Quadlet-style port mapping strings. The Ports map has container port
// specs as keys (e.g. "80/tcp") mapping to arrays of host binding
// objects with "HostIp" and "HostPort" fields.
//
// Non-default HostIp values (anything other than "0.0.0.0" and "::")
// are preserved in the output. Default values are omitted so the
// mapping reads as hostPort:containerPort.
func extractPortMappings(ports map[string]interface{}) []string {
	if len(ports) == 0 {
		return nil
	}

	// Sort container port keys for deterministic output.
	keys := make([]string, 0, len(ports))
	for k := range ports {
		keys = append(keys, k)
	}
	sort.Strings(keys)

	var mappings []string
	for _, containerPort := range keys {
		bindings, ok := ports[containerPort].([]interface{})
		if !ok || len(bindings) == 0 {
			continue
		}

		// Strip the protocol suffix for the mapping (e.g. "80/tcp" -> "80").
		cPort := containerPort
		if idx := strings.Index(containerPort, "/"); idx >= 0 {
			cPort = containerPort[:idx]
		}

		for _, binding := range bindings {
			bMap, ok := binding.(map[string]interface{})
			if !ok {
				continue
			}
			hostPort, _ := bMap["HostPort"].(string)
			if hostPort == "" {
				continue
			}
			hostIP, _ := bMap["HostIp"].(string)

			// Omit default bind addresses.
			if hostIP == "0.0.0.0" || hostIP == "::" || hostIP == "" {
				mappings = append(mappings, hostPort+":"+cPort)
			} else {
				mappings = append(mappings, hostIP+":"+hostPort+":"+cPort)
			}
		}
	}
	return mappings
}

// sortedNetworkNames returns the keys of a networks map in sorted order
// for deterministic Quadlet output.
func sortedNetworkNames(networks map[string]interface{}) []string {
	if len(networks) == 0 {
		return nil
	}
	names := make([]string, 0, len(networks))
	for k := range networks {
		names = append(names, k)
	}
	sort.Strings(names)
	return names
}
