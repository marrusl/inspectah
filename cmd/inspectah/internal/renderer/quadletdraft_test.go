package renderer

import (
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// TestGenerateQuadletDraft verifies full container output with ports,
// volumes, env, restart policy, networks, and correct section placement.
func TestGenerateQuadletDraft(t *testing.T) {
	c := schema.RunningContainer{
		Name:          "myapp",
		Image:         "registry.example.com/myapp:latest",
		RestartPolicy: "always",
		Ports: map[string]interface{}{
			"8080/tcp": []interface{}{
				map[string]interface{}{"HostIp": "0.0.0.0", "HostPort": "8080"},
			},
		},
		Mounts: []schema.ContainerMount{
			{Source: "/data/myapp", Destination: "/app/data", RW: true},
			{Source: "/etc/myapp.conf", Destination: "/etc/myapp.conf", RW: false},
		},
		Env: []string{
			"PATH=/usr/bin:/bin",
			"APP_ENV=production",
			"DB_HOST=db.internal",
		},
		Networks: map[string]interface{}{
			"backend": map[string]interface{}{},
		},
	}

	got := GenerateQuadletDraft(c)

	// [Container] section checks
	requireContains(t, got, "[Container]")
	requireContains(t, got, "ContainerName=myapp")
	requireContains(t, got, "Image=registry.example.com/myapp:latest")
	requireContains(t, got, "PublishPort=8080:8080")
	requireContains(t, got, "Volume=/data/myapp:/app/data")
	requireContains(t, got, "Volume=/etc/myapp.conf:/etc/myapp.conf:ro")
	requireContains(t, got, "Environment=APP_ENV=production")
	requireContains(t, got, "Environment=DB_HOST=db.internal")
	requireContains(t, got, "Network=backend")

	// PATH must be skipped
	if strings.Contains(got, "Environment=PATH=") {
		t.Error("PATH env var should be skipped")
	}

	// [Service] section: Restart=always must be here, NOT in [Container]
	requireContains(t, got, "[Service]")
	requireContains(t, got, "Restart=always")

	// Verify Restart is under [Service], not [Container]
	containerIdx := strings.Index(got, "[Container]")
	serviceIdx := strings.Index(got, "[Service]")
	restartIdx := strings.Index(got, "Restart=always")
	if restartIdx < serviceIdx || restartIdx < containerIdx {
		t.Error("Restart=always should appear after [Service] section header")
	}

	// [Install] section
	requireContains(t, got, "[Install]")
	requireContains(t, got, "WantedBy=default.target")
}

// TestGenerateQuadletDraft_NoRestartPolicy verifies that an empty restart
// policy produces a TODO comment and does NOT invent a default.
func TestGenerateQuadletDraft_NoRestartPolicy(t *testing.T) {
	c := schema.RunningContainer{
		Name:  "noretry",
		Image: "alpine:3.19",
	}

	got := GenerateQuadletDraft(c)

	requireContains(t, got, "# TODO: Set Restart= policy")

	// Must NOT have an invented Restart=on-failure or Restart=always
	if strings.Contains(got, "Restart=on-failure") {
		t.Error("should not invent Restart=on-failure when policy is empty")
	}
	if strings.Contains(got, "Restart=always") {
		t.Error("should not invent Restart=always when policy is empty")
	}
}

// TestGenerateQuadletDraft_MinimalContainer verifies the simplest case:
// only name and image produce ContainerName and Image lines.
func TestGenerateQuadletDraft_MinimalContainer(t *testing.T) {
	c := schema.RunningContainer{
		Name:  "minimal",
		Image: "busybox:latest",
	}

	got := GenerateQuadletDraft(c)

	requireContains(t, got, "ContainerName=minimal")
	requireContains(t, got, "Image=busybox:latest")

	// No PublishPort, Volume, Environment, or Network lines
	if strings.Contains(got, "PublishPort=") {
		t.Error("minimal container should have no PublishPort")
	}
	if strings.Contains(got, "Volume=") {
		t.Error("minimal container should have no Volume")
	}
	if strings.Contains(got, "Environment=") {
		t.Error("minimal container should have no Environment")
	}
	if strings.Contains(got, "Network=") {
		t.Error("minimal container should have no Network")
	}
}

// TestGenerateQuadletDraft_NonDefaultHostIp verifies that 127.0.0.1 is
// preserved in port mappings while 0.0.0.0 is omitted.
func TestGenerateQuadletDraft_NonDefaultHostIp(t *testing.T) {
	c := schema.RunningContainer{
		Name:  "localonly",
		Image: "nginx:latest",
		Ports: map[string]interface{}{
			"80/tcp": []interface{}{
				map[string]interface{}{"HostIp": "127.0.0.1", "HostPort": "8080"},
			},
			"443/tcp": []interface{}{
				map[string]interface{}{"HostIp": "0.0.0.0", "HostPort": "8443"},
			},
		},
	}

	got := GenerateQuadletDraft(c)

	// 127.0.0.1 must be preserved
	requireContains(t, got, "PublishPort=127.0.0.1:8080:80")

	// 0.0.0.0 must be omitted (just hostPort:containerPort)
	requireContains(t, got, "PublishPort=8443:443")

	// Verify 0.0.0.0 does not appear in any port mapping
	for _, line := range strings.Split(got, "\n") {
		if strings.HasPrefix(line, "PublishPort=") && strings.Contains(line, "0.0.0.0") {
			t.Errorf("0.0.0.0 should be omitted from port mapping: %s", line)
		}
	}
}

// TestGenerateQuadletDraft_DeferredTodoComments verifies that TODO
// comments are present for healthcheck, dependency ordering, and user
// namespace.
func TestGenerateQuadletDraft_DeferredTodoComments(t *testing.T) {
	c := schema.RunningContainer{
		Name:          "withpolicy",
		Image:         "redis:7",
		RestartPolicy: "on-failure",
	}

	got := GenerateQuadletDraft(c)

	requireContains(t, got, "# TODO: Add HealthCheck if the container defines one")
	requireContains(t, got, "# TODO: Review dependency ordering (After=, Requires=)")
	requireContains(t, got, "# TODO: Evaluate user namespace mapping (UserNS=)")
}

// TestGenerateQuadletDraft_NetworksToString verifies that networks from
// the networks map produce Network= directives.
func TestGenerateQuadletDraft_NetworksToString(t *testing.T) {
	c := schema.RunningContainer{
		Name:  "multinetwork",
		Image: "nginx:latest",
		Networks: map[string]interface{}{
			"frontend": map[string]interface{}{},
			"backend":  map[string]interface{}{},
			"mynet":    map[string]interface{}{},
		},
	}

	got := GenerateQuadletDraft(c)

	requireContains(t, got, "Network=backend")
	requireContains(t, got, "Network=frontend")
	requireContains(t, got, "Network=mynet")

	// Verify sorted order (backend < frontend < mynet)
	backendIdx := strings.Index(got, "Network=backend")
	frontendIdx := strings.Index(got, "Network=frontend")
	mynetIdx := strings.Index(got, "Network=mynet")
	if backendIdx > frontendIdx || frontendIdx > mynetIdx {
		t.Error("networks should be in sorted order")
	}
}

// requireContains fails the test if s does not contain substr.
func requireContains(t *testing.T, s, substr string) {
	t.Helper()
	if !strings.Contains(s, substr) {
		t.Errorf("output missing %q\n\ngot:\n%s", substr, s)
	}
}
