package inspector

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Helper: load fixture files from testdata/container/
// ---------------------------------------------------------------------------

func loadContainerFixture(t *testing.T, name string) string {
	t.Helper()
	path := filepath.Join("testdata", "container", name)
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("failed to load fixture %s: %v", name, err)
	}
	return string(data)
}

// ---------------------------------------------------------------------------
// extractQuadletImage
// ---------------------------------------------------------------------------

func TestExtractQuadletImage(t *testing.T) {
	tests := []struct {
		name    string
		content string
		want    string
	}{
		{
			name: "standard image line",
			content: `[Container]
Image=registry.example.com/myapp:latest
PublishPort=8080:8080`,
			want: "registry.example.com/myapp:latest",
		},
		{
			name: "image with spaces around equals",
			content: `[Container]
Image = quay.io/fedora/fedora:40
`,
			want: "quay.io/fedora/fedora:40",
		},
		{
			name:    "no image line",
			content: "[Container]\nPublishPort=8080:8080\n",
			want:    "",
		},
		{
			name:    "empty content",
			content: "",
			want:    "",
		},
		{
			name: "case insensitive key",
			content: `[Container]
IMAGE=docker.io/library/nginx:latest`,
			want: "docker.io/library/nginx:latest",
		},
		{
			name: "imageid should not match",
			content: `[Container]
ImageID=sha256:abc123
Image=myimage:v1`,
			want: "myimage:v1",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := extractQuadletImage(tt.content)
			if got != tt.want {
				t.Errorf("extractQuadletImage() = %q, want %q", got, tt.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// extractComposeImages
// ---------------------------------------------------------------------------

func TestExtractComposeImages(t *testing.T) {
	tests := []struct {
		name    string
		content string
		want    []schema.ComposeService
	}{
		{
			name: "2-space indent",
			content: `services:
  web:
    image: nginx:1.25
  db:
    image: postgres:16`,
			want: []schema.ComposeService{
				{Service: "web", Image: "nginx:1.25"},
				{Service: "db", Image: "postgres:16"},
			},
		},
		{
			name: "4-space indent",
			content: `services:
    frontend:
        image: node:20
    backend:
        image: python:3.12`,
			want: []schema.ComposeService{
				{Service: "frontend", Image: "node:20"},
				{Service: "backend", Image: "python:3.12"},
			},
		},
		{
			name: "quoted images",
			content: `services:
  app:
    image: 'registry.io/app:v2'
  cache:
    image: "redis:7"`,
			want: []schema.ComposeService{
				{Service: "app", Image: "registry.io/app:v2"},
				{Service: "cache", Image: "redis:7"},
			},
		},
		{
			name: "service without image",
			content: `services:
  web:
    build: .
  db:
    image: postgres:16`,
			want: []schema.ComposeService{
				{Service: "db", Image: "postgres:16"},
			},
		},
		{
			name:    "empty content",
			content: "",
			want:    nil,
		},
		{
			name: "comments and blank lines",
			content: `# My compose file
services:
  # The web server
  web:
    image: nginx:latest

    ports:
      - "80:80"`,
			want: []schema.ComposeService{
				{Service: "web", Image: "nginx:latest"},
			},
		},
		{
			name: "non-services blocks ignored",
			content: `networks:
  mynet:
    driver: bridge
services:
  web:
    image: nginx:1.25
volumes:
  data:
    driver: local`,
			want: []schema.ComposeService{
				{Service: "web", Image: "nginx:1.25"},
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := extractComposeImages(tt.content)
			if len(got) != len(tt.want) {
				t.Fatalf("got %d images, want %d: %+v", len(got), len(tt.want), got)
			}
			for i := range got {
				if got[i] != tt.want[i] {
					t.Errorf("image[%d] = %+v, want %+v", i, got[i], tt.want[i])
				}
			}
		})
	}
}

// ---------------------------------------------------------------------------
// extractComposeImages with fixture file
// ---------------------------------------------------------------------------

func TestExtractComposeImages_Fixture(t *testing.T) {
	content := loadContainerFixture(t, "compose.yaml")
	got := extractComposeImages(content)

	want := []schema.ComposeService{
		{Service: "web", Image: "nginx:1.25"},
		{Service: "db", Image: "postgres:16"},
		{Service: "redis", Image: "redis:7-alpine"},
	}

	if len(got) != len(want) {
		t.Fatalf("got %d images, want %d: %+v", len(got), len(want), got)
	}
	for i := range got {
		if got[i] != want[i] {
			t.Errorf("image[%d] = %+v, want %+v", i, got[i], want[i])
		}
	}
}

// ---------------------------------------------------------------------------
// matchGlob
// ---------------------------------------------------------------------------

func TestMatchGlob(t *testing.T) {
	tests := []struct {
		pattern string
		name    string
		want    bool
	}{
		{"docker-compose*.yml", "docker-compose.yml", true},
		{"docker-compose*.yml", "docker-compose-prod.yml", true},
		{"docker-compose*.yml", "docker-compose.yaml", false},
		{"compose*.yaml", "compose.yaml", true},
		{"compose*.yaml", "compose-dev.yaml", true},
		{"compose*.yaml", "docker-compose.yaml", false},
		{"*.container", "webapp.container", true},
		{"*.container", "webapp.volume", false},
	}

	for _, tt := range tests {
		t.Run(tt.pattern+"_"+tt.name, func(t *testing.T) {
			got := matchGlob(tt.pattern, tt.name)
			if got != tt.want {
				t.Errorf("matchGlob(%q, %q) = %v, want %v", tt.pattern, tt.name, got, tt.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Quadlet scanning with FakeExecutor
// ---------------------------------------------------------------------------

func TestScanQuadletDir(t *testing.T) {
	containerContent := loadContainerFixture(t, "webapp.container")
	volumeContent := loadContainerFixture(t, "webapp-data.volume")

	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/containers/systemd": {"webapp.container", "webapp-data.volume", "subdir"},
			"/etc/containers/systemd/subdir": {"nested.txt"},
		}).
		WithFiles(map[string]string{
			"/etc/containers/systemd/webapp.container":    containerContent,
			"/etc/containers/systemd/webapp-data.volume":  volumeContent,
		})

	units := scanQuadletDir(exec, "/etc/containers/systemd")

	if len(units) != 2 {
		t.Fatalf("got %d units, want 2", len(units))
	}

	// Units should be in directory order; check the container unit.
	var containerUnit *schema.QuadletUnit
	for i := range units {
		if units[i].Name == "webapp.container" {
			containerUnit = &units[i]
			break
		}
	}
	if containerUnit == nil {
		t.Fatal("webapp.container not found in results")
	}

	if containerUnit.Image != "registry.example.com/myapp:latest" {
		t.Errorf("Image = %q, want %q", containerUnit.Image, "registry.example.com/myapp:latest")
	}
	if containerUnit.Path != "etc/containers/systemd/webapp.container" {
		t.Errorf("Path = %q, want %q", containerUnit.Path, "etc/containers/systemd/webapp.container")
	}

	// Volume unit should have no image.
	var volumeUnit *schema.QuadletUnit
	for i := range units {
		if units[i].Name == "webapp-data.volume" {
			volumeUnit = &units[i]
			break
		}
	}
	if volumeUnit == nil {
		t.Fatal("webapp-data.volume not found in results")
	}
	if volumeUnit.Image != "" {
		t.Errorf("volume Image = %q, want empty", volumeUnit.Image)
	}
}

// ---------------------------------------------------------------------------
// Compose file discovery with dev-artifact filtering
// ---------------------------------------------------------------------------

func TestFindComposeFiles_WithDevArtifactFiltering(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/opt":                        {"myapp", "checkout"},
			"/opt/myapp":                  {"compose.yaml"},
			"/opt/checkout":               {".git", "docker-compose.yml"},
			"/opt/checkout/.git":          {"HEAD"},
		}).
		WithFiles(map[string]string{
			"/opt/myapp/compose.yaml":         "services:\n  web:\n    image: nginx:1.25\n",
			"/opt/checkout/docker-compose.yml": "services:\n  dev:\n    image: devimg:latest\n",
		})

	files := findComposeFiles(exec, "/opt")

	if len(files) != 1 {
		t.Fatalf("got %d compose files, want 1 (dev checkout should be pruned)", len(files))
	}

	if files[0].Path != "opt/myapp/compose.yaml" {
		t.Errorf("Path = %q, want %q", files[0].Path, "opt/myapp/compose.yaml")
	}
	if len(files[0].Images) != 1 || files[0].Images[0].Image != "nginx:1.25" {
		t.Errorf("unexpected images: %+v", files[0].Images)
	}
}

func TestFindComposeFiles_SkipsNodeModules(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/srv":                        {"project"},
			"/srv/project":                {"compose.yml", "node_modules"},
			"/srv/project/node_modules":   {"some-pkg"},
			"/srv/project/node_modules/some-pkg": {"docker-compose.yml"},
		}).
		WithFiles(map[string]string{
			"/srv/project/compose.yml": "services:\n  app:\n    image: myapp:v1\n",
			"/srv/project/node_modules/some-pkg/docker-compose.yml": "services:\n  dep:\n    image: dep:latest\n",
		})

	files := findComposeFiles(exec, "/srv")

	if len(files) != 1 {
		t.Fatalf("got %d compose files, want 1 (node_modules should be skipped)", len(files))
	}
	if files[0].Path != "srv/project/compose.yml" {
		t.Errorf("Path = %q, want %q", files[0].Path, "srv/project/compose.yml")
	}
}

// ---------------------------------------------------------------------------
// Podman JSON parsing
// ---------------------------------------------------------------------------

func TestParsePodmanInspect(t *testing.T) {
	fixture := loadContainerFixture(t, "podman_ps.json")

	var data []map[string]interface{}
	if err := jsonUnmarshal([]byte(fixture), &data); err != nil {
		t.Fatalf("failed to parse fixture: %v", err)
	}

	containers := parsePodmanInspect(data)

	if len(containers) != 2 {
		t.Fatalf("got %d containers, want 2", len(containers))
	}

	c1 := containers[0]
	if c1.ID != "abc123def456" {
		t.Errorf("ID = %q, want %q", c1.ID, "abc123def456")
	}
	if c1.Image != "registry.example.com/myapp:latest" {
		t.Errorf("Image = %q, want %q", c1.Image, "registry.example.com/myapp:latest")
	}
	if c1.Status != "running" {
		t.Errorf("Status = %q, want %q", c1.Status, "running")
	}
	if len(c1.Mounts) != 1 {
		t.Fatalf("got %d mounts, want 1", len(c1.Mounts))
	}
	if c1.Mounts[0].Destination != "/data" {
		t.Errorf("Mount Destination = %q, want %q", c1.Mounts[0].Destination, "/data")
	}
	if !c1.Mounts[0].RW {
		t.Error("Mount RW = false, want true")
	}
	if len(c1.Env) != 2 {
		t.Errorf("got %d env vars, want 2", len(c1.Env))
	}

	c2 := containers[1]
	if c2.ID != "789012345678" {
		t.Errorf("ID = %q, want %q", c2.ID, "789012345678")
	}
	if c2.Image != "redis:7-alpine" {
		t.Errorf("Image = %q, want %q", c2.Image, "redis:7-alpine")
	}
}

func TestParsePodmanPS_Fallback(t *testing.T) {
	data := []map[string]interface{}{
		{
			"ID":    "aaa111",
			"Names": []interface{}{"test-container"},
			"Image": "test:latest",
			"State": map[string]interface{}{"Status": "exited"},
		},
	}

	containers := parsePodmanPS(data)
	if len(containers) != 1 {
		t.Fatalf("got %d containers, want 1", len(containers))
	}
	if containers[0].Name != "test-container" {
		t.Errorf("Name = %q, want %q", containers[0].Name, "test-container")
	}
	if containers[0].Status != "exited" {
		t.Errorf("Status = %q, want %q", containers[0].Status, "exited")
	}
}

// ---------------------------------------------------------------------------
// Flatpak parsing
// ---------------------------------------------------------------------------

func TestDetectFlatpakApps(t *testing.T) {
	flatpakOutput := loadContainerFixture(t, "flatpak_list.txt")

	exec := NewFakeExecutor(map[string]ExecResult{
		"which flatpak": {ExitCode: 0, Stdout: "/usr/bin/flatpak"},
		"flatpak list --app --columns=application,origin,branch": {
			ExitCode: 0,
			Stdout:   flatpakOutput,
		},
	})

	apps := detectFlatpakApps(exec)

	if len(apps) != 3 {
		t.Fatalf("got %d apps, want 3", len(apps))
	}

	want := []schema.FlatpakApp{
		{AppID: "org.mozilla.firefox", Origin: "flathub", Branch: "stable"},
		{AppID: "org.gnome.Calculator", Origin: "fedora", Branch: "stable"},
		{AppID: "com.visualstudio.code", Origin: "flathub", Branch: "stable"},
	}

	for i, app := range apps {
		if app.AppID != want[i].AppID {
			t.Errorf("app[%d].AppID = %q, want %q", i, app.AppID, want[i].AppID)
		}
		if app.Origin != want[i].Origin {
			t.Errorf("app[%d].Origin = %q, want %q", i, app.Origin, want[i].Origin)
		}
		if app.Branch != want[i].Branch {
			t.Errorf("app[%d].Branch = %q, want %q", i, app.Branch, want[i].Branch)
		}
	}
}

func TestDetectFlatpakApps_NotInstalled(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"which flatpak": {ExitCode: 1, Stderr: "flatpak not found"},
	})

	apps := detectFlatpakApps(exec)
	if len(apps) != 0 {
		t.Errorf("got %d apps, want 0 when flatpak not installed", len(apps))
	}
}

// ---------------------------------------------------------------------------
// User quadlet directory discovery
// ---------------------------------------------------------------------------

func TestUserQuadletDirs(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/passwd": `root:x:0:0:root:/root:/bin/bash
daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin
alice:x:1000:1000:Alice:/home/alice:/bin/bash
bob:x:1001:1001:Bob:/home/bob:/bin/zsh
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
`,
		})

	dirs := userQuadletDirs(exec)

	want := []string{
		"/home/alice/.config/containers/systemd",
		"/home/bob/.config/containers/systemd",
	}

	if len(dirs) != len(want) {
		t.Fatalf("got %d dirs, want %d: %v", len(dirs), len(want), dirs)
	}
	for i := range dirs {
		if dirs[i] != want[i] {
			t.Errorf("dir[%d] = %q, want %q", i, dirs[i], want[i])
		}
	}
}

// ---------------------------------------------------------------------------
// Full integration test with FakeExecutor
// ---------------------------------------------------------------------------

func TestRunContainers_Integration(t *testing.T) {
	containerContent := loadContainerFixture(t, "webapp.container")
	volumeContent := loadContainerFixture(t, "webapp-data.volume")
	composeContent := loadContainerFixture(t, "compose.yaml")
	podmanJSON := loadContainerFixture(t, "podman_ps.json")
	flatpakOutput := loadContainerFixture(t, "flatpak_list.txt")

	exec := NewFakeExecutor(map[string]ExecResult{
		"podman ps -a --format json": {ExitCode: 0, Stdout: podmanJSON},
		"podman inspect abc123def456 789012345678": {ExitCode: 0, Stdout: podmanJSON},
		"which flatpak": {ExitCode: 0, Stdout: "/usr/bin/flatpak"},
		"flatpak list --app --columns=application,origin,branch": {
			ExitCode: 0,
			Stdout:   flatpakOutput,
		},
	}).
		WithDirs(map[string][]string{
			"/etc/containers/systemd": {"webapp.container", "webapp-data.volume"},
			"/opt":                    {"deploy"},
			"/opt/deploy":             {"compose.yaml"},
		}).
		WithFiles(map[string]string{
			"/etc/containers/systemd/webapp.container":   containerContent,
			"/etc/containers/systemd/webapp-data.volume": volumeContent,
			"/opt/deploy/compose.yaml":                   composeContent,
		})

	opts := ContainerOptions{
		QueryPodman: true,
		SystemType:  schema.SystemTypePackageMode,
	}

	section, warnings, err := RunContainers(exec, opts)
	if err != nil {
		t.Fatalf("RunContainers failed: %v", err)
	}
	if len(warnings) != 0 {
		t.Errorf("got %d warnings, want 0: %v", len(warnings), warnings)
	}

	// Quadlet units.
	if len(section.QuadletUnits) != 2 {
		t.Errorf("got %d quadlet units, want 2", len(section.QuadletUnits))
	}

	// Compose files.
	if len(section.ComposeFiles) != 1 {
		t.Errorf("got %d compose files, want 1", len(section.ComposeFiles))
	} else if len(section.ComposeFiles[0].Images) != 3 {
		t.Errorf("got %d compose images, want 3", len(section.ComposeFiles[0].Images))
	}

	// Running containers.
	if len(section.RunningContainers) != 2 {
		t.Errorf("got %d running containers, want 2", len(section.RunningContainers))
	}

	// Flatpak apps.
	if len(section.FlatpakApps) != 3 {
		t.Errorf("got %d flatpak apps, want 3", len(section.FlatpakApps))
	}
}

func TestRunContainers_NoPodman(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"which flatpak": {ExitCode: 1},
	})

	opts := ContainerOptions{
		QueryPodman: false,
		SystemType:  schema.SystemTypePackageMode,
	}

	section, warnings, err := RunContainers(exec, opts)
	if err != nil {
		t.Fatalf("RunContainers failed: %v", err)
	}
	if len(warnings) != 0 {
		t.Errorf("got %d warnings, want 0", len(warnings))
	}
	if len(section.QuadletUnits) != 0 {
		t.Errorf("got %d quadlet units, want 0", len(section.QuadletUnits))
	}
	if len(section.RunningContainers) != 0 {
		t.Errorf("got %d containers, want 0 (podman not queried)", len(section.RunningContainers))
	}
}

func TestRunContainers_PodmanFailure(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"podman ps -a --format json": {ExitCode: 1, Stderr: "podman not found"},
		"which flatpak":              {ExitCode: 1},
	})

	opts := ContainerOptions{
		QueryPodman: true,
		SystemType:  schema.SystemTypePackageMode,
	}

	section, warnings, err := RunContainers(exec, opts)
	if err != nil {
		t.Fatalf("RunContainers failed: %v", err)
	}
	if len(warnings) != 1 {
		t.Fatalf("got %d warnings, want 1", len(warnings))
	}
	if section.RunningContainers != nil {
		t.Errorf("expected nil containers on failure, got %d", len(section.RunningContainers))
	}
}

// ---------------------------------------------------------------------------
// filteredWalk integration
// ---------------------------------------------------------------------------

func TestFilteredWalk_PrunesGitDirs(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/test":         {"clean", "vcs-checkout"},
			"/test/clean":   {"compose.yml"},
			"/test/vcs-checkout": {".git", "compose.yml"},
			"/test/vcs-checkout/.git": {"HEAD"},
		}).
		WithFiles(map[string]string{
			"/test/clean/compose.yml":        "ok",
			"/test/vcs-checkout/compose.yml": "should be pruned",
		})

	var visited []string
	filteredWalk(exec, "/test", func(path, name string) {
		visited = append(visited, path)
	})

	if len(visited) != 1 {
		t.Fatalf("visited %d files, want 1: %v", len(visited), visited)
	}
	if visited[0] != "/test/clean/compose.yml" {
		t.Errorf("visited %q, want %q", visited[0], "/test/clean/compose.yml")
	}
}

func TestFilteredWalk_SkipsSkipDirs(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/root":              {"src", "node_modules", "__pycache__"},
			"/root/src":          {"app.py"},
			"/root/node_modules": {"pkg"},
			"/root/node_modules/pkg": {"compose.yml"},
			"/root/__pycache__":  {"cache.pyc"},
		}).
		WithFiles(map[string]string{
			"/root/src/app.py":                      "ok",
			"/root/node_modules/pkg/compose.yml":    "skip",
			"/root/__pycache__/cache.pyc":           "skip",
		})

	var visited []string
	filteredWalk(exec, "/root", func(path, name string) {
		visited = append(visited, path)
	})

	if len(visited) != 1 {
		t.Fatalf("visited %d files, want 1: %v", len(visited), visited)
	}
}

// jsonUnmarshal is a test helper wrapping json.Unmarshal.
func jsonUnmarshal(data []byte, v interface{}) error {
	return json.Unmarshal(data, v)
}
