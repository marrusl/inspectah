package pipeline

import (
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/inspector"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// DetectUnreachableRepos tests
// ---------------------------------------------------------------------------

func TestDetectUnreachableRepos(t *testing.T) {
	tests := []struct {
		name     string
		stderr   string
		wantIDs  []string
		wantErrs []string
	}{
		{
			name:   "no failures",
			stderr: "Metadata cache created.\nLast metadata check: ok\n",
		},
		{
			name:    "single quoted repo",
			stderr:  "Failed to synchronize cache for repo 'epel'\n",
			wantIDs: []string{"epel"},
		},
		{
			name:    "double quoted repo",
			stderr:  `Failed to download metadata for repo "rpmfusion-free"` + "\n",
			wantIDs: []string{"rpmfusion-free"},
		},
		{
			name: "multiple repos",
			stderr: "Failed to synchronize cache for repo 'epel'\n" +
				"some other info\n" +
				"Cannot download repomd.xml for repo 'extras'\n",
			wantIDs: []string{"epel", "extras"},
		},
		{
			name: "deduplication",
			stderr: "Failed to synchronize cache for repo 'epel'\n" +
				"Failed to download metadata for repo 'epel'\n",
			wantIDs: []string{"epel"},
		},
		{
			name:   "empty stderr",
			stderr: "",
		},
		{
			name:   "no quotes in pattern match",
			stderr: "Failed to synchronize cache for repo without_quotes\n",
			// No quoted repo ID — should not extract
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := DetectUnreachableRepos(tt.stderr)
			if len(tt.wantIDs) == 0 {
				assert.Empty(t, got)
				return
			}
			require.Len(t, got, len(tt.wantIDs))
			for i, wantID := range tt.wantIDs {
				assert.Equal(t, wantID, got[i].RepoID)
				assert.Equal(t, wantID, got[i].RepoName)
				assert.NotEmpty(t, got[i].Error)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// classifyDirectInstalls tests
// ---------------------------------------------------------------------------

func TestClassifyDirectInstalls(t *testing.T) {
	tests := []struct {
		name           string
		packages       []schema.PackageEntry
		wantRepo       []string
		wantDirect     []string
	}{
		{
			name: "mixed sources",
			packages: []schema.PackageEntry{
				{Name: "vim", SourceRepo: "baseos", Include: true},
				{Name: "local-tool", SourceRepo: "(none)", Include: true},
				{Name: "httpd", SourceRepo: "appstream", Include: true},
				{Name: "manual-pkg", SourceRepo: "commandline", Include: true},
			},
			wantRepo:   []string{"httpd", "vim"},
			wantDirect: []string{"local-tool", "manual-pkg"},
		},
		{
			name: "all repo packages",
			packages: []schema.PackageEntry{
				{Name: "nginx", SourceRepo: "appstream", Include: true},
				{Name: "curl", SourceRepo: "baseos", Include: true},
			},
			wantRepo: []string{"curl", "nginx"},
		},
		{
			name: "all direct installs",
			packages: []schema.PackageEntry{
				{Name: "local-a", SourceRepo: "", Include: true},
				{Name: "local-b", SourceRepo: "installed", Include: true},
			},
			wantDirect: []string{"local-a", "local-b"},
		},
		{
			name: "empty source repo treated as direct",
			packages: []schema.PackageEntry{
				{Name: "mystery", SourceRepo: "  ", Include: true},
			},
			wantDirect: []string{"mystery"},
		},
		{
			name:     "nil rpm section",
			packages: nil,
		},
		{
			name: "excluded packages not classified",
			packages: []schema.PackageEntry{
				{Name: "included", SourceRepo: "baseos", Include: true},
				{Name: "excluded", SourceRepo: "baseos", Include: false},
			},
			wantRepo: []string{"included"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			snap := &schema.InspectionSnapshot{}
			if tt.packages != nil {
				snap.Rpm = &schema.RpmSection{PackagesAdded: tt.packages}
			}
			repo, direct := classifyDirectInstalls(snap)
			assert.Equal(t, tt.wantRepo, repo, "repo packages")
			assert.Equal(t, tt.wantDirect, direct, "direct installs")
		})
	}
}

// ---------------------------------------------------------------------------
// RunPreflight tests
// ---------------------------------------------------------------------------

func TestRunPreflight_NoBaseImage(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{}

	fake := inspector.NewFakeExecutor(nil)
	result, err := RunPreflight(fake, PreflightOptions{Snapshot: snap})
	require.NoError(t, err)
	assert.Equal(t, "failed", result.Status)
	assert.Contains(t, *result.StatusReason, "No base image")
}

func TestRunPreflight_NoPackages(t *testing.T) {
	bi := "registry.redhat.io/rhel9/rhel-bootc:9.4"
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaseImage:     &bi,
		PackagesAdded: []schema.PackageEntry{},
	}

	fake := inspector.NewFakeExecutor(nil)
	result, err := RunPreflight(fake, PreflightOptions{Snapshot: snap})
	require.NoError(t, err)
	assert.Equal(t, "completed", result.Status)
	assert.Equal(t, bi, result.BaseImage)
}

func TestRunPreflight_PullFails(t *testing.T) {
	bi := "registry.redhat.io/rhel9/rhel-bootc:9.4"
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaseImage: &bi,
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", SourceRepo: "appstream", Include: true},
		},
	}

	// podman pull fails
	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		"nsenter -t 1 -m -u -i -n -- podman pull -q " + bi: {
			Stderr:   "connection refused",
			ExitCode: 1,
		},
	})

	result, err := RunPreflight(fake, PreflightOptions{Snapshot: snap})
	require.NoError(t, err)
	assert.Equal(t, "failed", result.Status)
	assert.Contains(t, *result.StatusReason, "could not be pulled")
}

func TestRunPreflight_AllAvailable(t *testing.T) {
	bi := "registry.redhat.io/rhel9/rhel-bootc:9.4"
	containerName := "test-preflight"
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaseImage: &bi,
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", SourceRepo: "appstream", Include: true},
			{Name: "vim-enhanced", SourceRepo: "baseos", Include: true},
		},
	}

	// Build the full command keys
	pullKey := "nsenter -t 1 -m -u -i -n -- podman pull -q " + bi
	runKey := "nsenter -t 1 -m -u -i -n -- podman run -d --name " + containerName + " " + bi + " sleep infinity"
	queryKey := "nsenter -t 1 -m -u -i -n -- podman exec " + containerName + " dnf repoquery --available --queryformat %{name} httpd vim-enhanced"
	repolistKey := "nsenter -t 1 -m -u -i -n -- podman exec " + containerName + " dnf repolist --quiet"
	rmKey := "nsenter -t 1 -m -u -i -n -- podman rm -f " + containerName

	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		pullKey: {ExitCode: 0},
		runKey:  {ExitCode: 0},
		queryKey: {
			Stdout:   "httpd\nvim-enhanced\n",
			ExitCode: 0,
		},
		repolistKey: {
			Stdout:   "appstream\nbaseos\n",
			ExitCode: 0,
		},
		rmKey: {ExitCode: 0},
	})

	result, err := RunPreflight(fake, PreflightOptions{
		Snapshot:      snap,
		ContainerName: containerName,
	})
	require.NoError(t, err)
	assert.Equal(t, "completed", result.Status)
	assert.Nil(t, result.StatusReason)
	assert.Equal(t, []string{"httpd", "vim-enhanced"}, result.Available)
	assert.Empty(t, result.Unavailable)
	assert.Equal(t, bi, result.BaseImage)
	assert.Equal(t, []string{"appstream", "baseos"}, result.ReposQueried)
}

func TestRunPreflight_SomeUnavailable(t *testing.T) {
	bi := "registry.redhat.io/rhel9/rhel-bootc:9.4"
	containerName := "test-preflight-partial"
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaseImage: &bi,
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", SourceRepo: "appstream", Include: true},
			{Name: "custom-pkg", SourceRepo: "baseos", Include: true},
		},
	}

	pullKey := "nsenter -t 1 -m -u -i -n -- podman pull -q " + bi
	runKey := "nsenter -t 1 -m -u -i -n -- podman run -d --name " + containerName + " " + bi + " sleep infinity"
	queryKey := "nsenter -t 1 -m -u -i -n -- podman exec " + containerName + " dnf repoquery --available --queryformat %{name} custom-pkg httpd"
	repolistKey := "nsenter -t 1 -m -u -i -n -- podman exec " + containerName + " dnf repolist --quiet"
	rmKey := "nsenter -t 1 -m -u -i -n -- podman rm -f " + containerName

	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		pullKey: {ExitCode: 0},
		runKey:  {ExitCode: 0},
		queryKey: {
			Stdout:   "httpd\n",
			ExitCode: 0,
		},
		repolistKey: {
			Stdout:   "appstream\nbaseos\n",
			ExitCode: 0,
		},
		rmKey: {ExitCode: 0},
	})

	result, err := RunPreflight(fake, PreflightOptions{
		Snapshot:      snap,
		ContainerName: containerName,
	})
	require.NoError(t, err)
	assert.Equal(t, "completed", result.Status)
	assert.Equal(t, []string{"httpd"}, result.Available)
	assert.Equal(t, []string{"custom-pkg"}, result.Unavailable)
}

func TestRunPreflight_RepoUnreachable(t *testing.T) {
	bi := "registry.redhat.io/rhel9/rhel-bootc:9.4"
	containerName := "test-preflight-unreachable"
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaseImage: &bi,
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", SourceRepo: "appstream", Include: true},
			{Name: "epel-pkg", SourceRepo: "epel", Include: true},
		},
	}

	pullKey := "nsenter -t 1 -m -u -i -n -- podman pull -q " + bi
	runKey := "nsenter -t 1 -m -u -i -n -- podman run -d --name " + containerName + " " + bi + " sleep infinity"
	queryKey := "nsenter -t 1 -m -u -i -n -- podman exec " + containerName + " dnf repoquery --available --queryformat %{name} epel-pkg httpd"
	repolistKey := "nsenter -t 1 -m -u -i -n -- podman exec " + containerName + " dnf repolist --quiet"
	rmKey := "nsenter -t 1 -m -u -i -n -- podman rm -f " + containerName

	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		pullKey: {ExitCode: 0},
		runKey:  {ExitCode: 0},
		queryKey: {
			Stdout:   "httpd\n",
			Stderr:   "Failed to synchronize cache for repo 'epel'\n",
			ExitCode: 0,
		},
		repolistKey: {
			Stdout:   "appstream\nbaseos\n",
			ExitCode: 0,
		},
		rmKey: {ExitCode: 0},
	})

	result, err := RunPreflight(fake, PreflightOptions{
		Snapshot:      snap,
		ContainerName: containerName,
	})
	require.NoError(t, err)
	assert.Equal(t, "partial", result.Status)
	assert.NotNil(t, result.StatusReason)
	assert.Contains(t, *result.StatusReason, "unreachable")
	assert.Equal(t, []string{"httpd"}, result.Available)
	// epel-pkg removed from unavailable because its repo is unreachable
	assert.Empty(t, result.Unavailable)
	require.Len(t, result.RepoUnreachable, 1)
	assert.Equal(t, "epel", result.RepoUnreachable[0].RepoID)
}

func TestRunPreflight_DirectInstallsOnly(t *testing.T) {
	bi := "registry.redhat.io/rhel9/rhel-bootc:9.4"
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaseImage: &bi,
		PackagesAdded: []schema.PackageEntry{
			{Name: "local-tool", SourceRepo: "(none)", Include: true},
			{Name: "manual-pkg", SourceRepo: "commandline", Include: true},
		},
	}

	// No podman commands needed — all direct installs, no repoquery needed
	fake := inspector.NewFakeExecutor(nil)
	result, err := RunPreflight(fake, PreflightOptions{Snapshot: snap})
	require.NoError(t, err)
	assert.Equal(t, "completed", result.Status)
	assert.Equal(t, []string{"local-tool", "manual-pkg"}, result.DirectInstall)
	assert.Equal(t, bi, result.BaseImage)
}

// ---------------------------------------------------------------------------
// extractQuotedRepoID tests
// ---------------------------------------------------------------------------

func TestExtractQuotedRepoID(t *testing.T) {
	tests := []struct {
		line string
		want string
	}{
		{"Failed to synchronize cache for repo 'epel'", "epel"},
		{`Failed to download metadata for repo "rpmfusion"`, "rpmfusion"},
		{"no quotes at all", ""},
		{"single 'quote only", ""},
	}

	for _, tt := range tests {
		t.Run(tt.line, func(t *testing.T) {
			got := extractQuotedRepoID(tt.line)
			assert.Equal(t, tt.want, got)
		})
	}
}

// ---------------------------------------------------------------------------
// truncate tests
// ---------------------------------------------------------------------------

func TestTruncate(t *testing.T) {
	assert.Equal(t, "short", truncate("short", 100))
	assert.Equal(t, "12345", truncate("1234567890", 5))

	long := strings.Repeat("x", 300)
	got := truncate(long, 200)
	assert.Len(t, got, 200)
}
