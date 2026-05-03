package baseline

import (
	"fmt"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// FakeRunner — test double for CommandRunner
// ---------------------------------------------------------------------------

// FakeRunner returns canned results for commands. Unknown commands return
// exit code 127.
type FakeRunner struct {
	commands map[string]CommandResult
}

func newFakeRunner(commands map[string]CommandResult) *FakeRunner {
	if commands == nil {
		commands = make(map[string]CommandResult)
	}
	return &FakeRunner{commands: commands}
}

func (f *FakeRunner) Run(name string, args ...string) CommandResult {
	parts := append([]string{name}, args...)
	key := strings.Join(parts, " ")
	if result, ok := f.commands[key]; ok {
		return result
	}
	return CommandResult{
		Stderr:   fmt.Sprintf("unknown command: %s", key),
		ExitCode: 127,
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func sampleRpmQAOutput() string {
	return "0:bash-5.2.15-3.el9.x86_64\n0:coreutils-8.32-34.el9.x86_64\n(none):zlib-1.2.11-40.el9.x86_64\n"
}

func samplePresetOutput() string {
	return "enable sshd.service\ndisable debug-shell.service\n"
}

func sampleModuleOutput() string {
	return "[nodejs]\nname=nodejs\nstream=18\nprofiles=\nstate=enabled\n\n[postgresql]\nname=postgresql\nstream=15\nprofiles=\nstate=enabled\n"
}

func rpmQAKey(image string) string {
	return fmt.Sprintf("podman run --rm --cgroups=disabled %s rpm -qa --queryformat %s\\n",
		image, rpmQAQueryformat)
}

func presetKey(image string) string {
	return fmt.Sprintf("podman run --rm --cgroups=disabled %s bash -c cat /usr/lib/systemd/system-preset/*.preset 2>/dev/null || true",
		image)
}

func moduleKey(image string) string {
	return fmt.Sprintf("podman run --rm --cgroups=disabled %s bash -c cat /etc/dnf/modules.d/*.module 2>/dev/null || true",
		image)
}

func buildResolverFake(image string) *FakeRunner {
	return newFakeRunner(map[string]CommandResult{
		fmt.Sprintf("podman image exists %s", image): {ExitCode: 0},
		rpmQAKey(image):   {Stdout: sampleRpmQAOutput(), ExitCode: 0},
		presetKey(image):  {Stdout: samplePresetOutput(), ExitCode: 0},
		moduleKey(image):  {Stdout: sampleModuleOutput(), ExitCode: 0},
	})
}

func buildResolverFakeRHEL(image string) *FakeRunner {
	fake := buildResolverFake(image)
	fake.commands["podman login --get-login registry.redhat.io"] =
		CommandResult{Stdout: "testuser\n", ExitCode: 0}
	return fake
}

// ---------------------------------------------------------------------------
// Resolve tests
// ---------------------------------------------------------------------------

func TestResolve_TargetImageOverride(t *testing.T) {
	image := "quay.io/custom/image:latest"
	fake := buildResolverFake(image)
	r := NewResolver(fake)

	pkgs, ref, noBaseline, err := r.Resolve(ResolveOptions{
		TargetImage: image,
	})
	require.NoError(t, err)
	assert.False(t, noBaseline)
	assert.Equal(t, image, ref)
	assert.Len(t, pkgs, 3)
	assert.Contains(t, pkgs, "bash.x86_64")
}

func TestResolve_TargetImageWithBaselineFile(t *testing.T) {
	content := "0:bash-5.2.15-3.el9.x86_64\n0:vim-8.2.1-1.el9.x86_64\n"
	path := writeTempFile(t, content)

	r := NewResolver(nil) // no executor needed for file mode
	pkgs, ref, noBaseline, err := r.Resolve(ResolveOptions{
		TargetImage:  "quay.io/custom/image:latest",
		BaselineFile: path,
	})
	require.NoError(t, err)
	assert.False(t, noBaseline)
	assert.Equal(t, "quay.io/custom/image:latest", ref)
	assert.Len(t, pkgs, 2)
}

func TestResolve_BaselineFileOnly(t *testing.T) {
	content := "0:bash-5.2.15-3.el9.x86_64\n"
	path := writeTempFile(t, content)

	r := NewResolver(nil)
	pkgs, ref, noBaseline, err := r.Resolve(ResolveOptions{
		OsID:         "rhel",
		VersionID:    "9.6",
		BaselineFile: path,
	})
	require.NoError(t, err)
	assert.False(t, noBaseline)
	assert.Equal(t, "registry.redhat.io/rhel9/rhel-bootc:9.6", ref)
	assert.Len(t, pkgs, 1)
}

func TestResolve_AutoDetectCentOS(t *testing.T) {
	image := "quay.io/centos-bootc/centos-bootc:stream9"
	fake := buildResolverFake(image)
	r := NewResolver(fake)

	pkgs, ref, noBaseline, err := r.Resolve(ResolveOptions{
		OsID:      "centos",
		VersionID: "9",
	})
	require.NoError(t, err)
	assert.False(t, noBaseline)
	assert.Equal(t, image, ref)
	assert.Len(t, pkgs, 3)
}

func TestResolve_AutoDetectRHEL(t *testing.T) {
	image := "registry.redhat.io/rhel9/rhel-bootc:9.6"
	fake := buildResolverFakeRHEL(image)
	r := NewResolver(fake)

	pkgs, ref, noBaseline, err := r.Resolve(ResolveOptions{
		OsID:      "rhel",
		VersionID: "9.4",
	})
	require.NoError(t, err)
	assert.False(t, noBaseline)
	assert.Equal(t, image, ref)
	assert.Len(t, pkgs, 3)
}

func TestResolve_NoExecutor(t *testing.T) {
	r := NewResolver(nil)

	pkgs, ref, noBaseline, err := r.Resolve(ResolveOptions{
		OsID:      "rhel",
		VersionID: "9.4",
	})
	require.NoError(t, err)
	assert.True(t, noBaseline)
	assert.Equal(t, "registry.redhat.io/rhel9/rhel-bootc:9.6", ref)
	assert.Nil(t, pkgs)
}

func TestResolve_UnmappedOS(t *testing.T) {
	r := NewResolver(nil)

	pkgs, ref, noBaseline, err := r.Resolve(ResolveOptions{
		OsID:      "ubuntu",
		VersionID: "22.04",
	})
	require.NoError(t, err)
	assert.True(t, noBaseline)
	assert.Equal(t, "", ref)
	assert.Nil(t, pkgs)
}

func TestResolve_PodmanFails(t *testing.T) {
	image := "quay.io/centos-bootc/centos-bootc:stream9"
	fake := newFakeRunner(map[string]CommandResult{
		fmt.Sprintf("podman image exists %s", image): {ExitCode: 1},
		fmt.Sprintf("podman pull %s", image):         {ExitCode: 1, Stderr: "pull failed"},
	})
	r := NewResolver(fake)

	pkgs, ref, noBaseline, err := r.Resolve(ResolveOptions{
		OsID:      "centos",
		VersionID: "9",
	})
	require.Error(t, err)
	assert.Contains(t, err.Error(), "baseline query failed")
	assert.True(t, noBaseline)
	assert.Equal(t, image, ref)
	assert.Nil(t, pkgs)
}

func TestResolve_QueryReturnsEmptyPackages(t *testing.T) {
	image := "quay.io/centos-bootc/centos-bootc:stream9"
	// rpm -qa succeeds but returns unparseable output
	fake := newFakeRunner(map[string]CommandResult{
		fmt.Sprintf("podman image exists %s", image): {ExitCode: 0},
		rpmQAKey(image): {Stdout: "not-valid-nevra-format\n", ExitCode: 0},
	})
	r := NewResolver(fake)

	pkgs, ref, noBaseline, err := r.Resolve(ResolveOptions{
		OsID:      "centos",
		VersionID: "9",
	})
	require.Error(t, err)
	assert.Contains(t, err.Error(), "no packages parsed")
	assert.True(t, noBaseline)
	assert.Equal(t, image, ref)
	assert.Nil(t, pkgs)
}

func TestResolve_TargetImageQueryFails(t *testing.T) {
	image := "quay.io/custom/broken:latest"
	fake := newFakeRunner(map[string]CommandResult{
		fmt.Sprintf("podman image exists %s", image): {ExitCode: 0},
		rpmQAKey(image): {Stdout: "", Stderr: "exec failed", ExitCode: 1},
	})
	r := NewResolver(fake)

	pkgs, ref, noBaseline, err := r.Resolve(ResolveOptions{
		TargetImage: image,
	})
	require.Error(t, err)
	assert.Contains(t, err.Error(), "baseline query failed")
	assert.True(t, noBaseline)
	assert.Equal(t, image, ref)
	assert.Nil(t, pkgs)
}

// ---------------------------------------------------------------------------
// QueryPackages tests
// ---------------------------------------------------------------------------

func TestQueryPackages_ParsesNEVRA(t *testing.T) {
	image := "quay.io/test/image:latest"
	fake := buildResolverFake(image)
	r := NewResolver(fake)

	pkgs, err := r.QueryPackages(image)
	require.NoError(t, err)
	require.Len(t, pkgs, 3)

	bash := pkgs["bash.x86_64"]
	assert.Equal(t, "bash", bash.Name)
	assert.Equal(t, "0", bash.Epoch)
	assert.Equal(t, "5.2.15", bash.Version)
	assert.Equal(t, "3.el9", bash.Release)
	assert.Equal(t, "x86_64", bash.Arch)

	zlib := pkgs["zlib.x86_64"]
	assert.Equal(t, "0", zlib.Epoch) // (none) normalized
}

func TestQueryPackages_SessionCaching(t *testing.T) {
	image := "quay.io/test/image:latest"
	fake := buildResolverFake(image)
	r := NewResolver(fake)

	// First call populates cache.
	pkgs1, err := r.QueryPackages(image)
	require.NoError(t, err)

	// Remove the canned response — second call must hit cache.
	delete(fake.commands, rpmQAKey(image))

	pkgs2, err := r.QueryPackages(image)
	require.NoError(t, err)
	assert.Equal(t, pkgs1, pkgs2)
}

func TestQueryPackages_PodmanNotCached_PullsFirst(t *testing.T) {
	image := "quay.io/test/image:latest"
	fake := newFakeRunner(map[string]CommandResult{
		fmt.Sprintf("podman image exists %s", image): {ExitCode: 1},
		fmt.Sprintf("podman pull %s", image):         {ExitCode: 0},
		rpmQAKey(image):                               {Stdout: sampleRpmQAOutput(), ExitCode: 0},
	})
	r := NewResolver(fake)

	pkgs, err := r.QueryPackages(image)
	require.NoError(t, err)
	assert.Len(t, pkgs, 3)
}

// ---------------------------------------------------------------------------
// QueryPresets tests
// ---------------------------------------------------------------------------

func TestQueryPresets(t *testing.T) {
	image := "quay.io/test/image:latest"
	fake := buildResolverFake(image)
	r := NewResolver(fake)

	text, err := r.QueryPresets(image)
	require.NoError(t, err)
	assert.Contains(t, text, "enable sshd.service")
	assert.Contains(t, text, "disable debug-shell.service")
}

func TestQueryPresets_Caching(t *testing.T) {
	image := "quay.io/test/image:latest"
	fake := buildResolverFake(image)
	r := NewResolver(fake)

	text1, err := r.QueryPresets(image)
	require.NoError(t, err)

	delete(fake.commands, presetKey(image))

	text2, err := r.QueryPresets(image)
	require.NoError(t, err)
	assert.Equal(t, text1, text2)
}

func TestQueryPresets_EmptyOutput(t *testing.T) {
	image := "quay.io/test/image:latest"
	fake := newFakeRunner(map[string]CommandResult{
		fmt.Sprintf("podman image exists %s", image): {ExitCode: 0},
		presetKey(image):                              {Stdout: "", ExitCode: 0},
	})
	r := NewResolver(fake)

	text, err := r.QueryPresets(image)
	require.NoError(t, err)
	assert.Equal(t, "", text)
}

// ---------------------------------------------------------------------------
// QueryModuleStreams tests
// ---------------------------------------------------------------------------

func TestQueryModuleStreams(t *testing.T) {
	image := "quay.io/test/image:latest"
	fake := buildResolverFake(image)
	r := NewResolver(fake)

	streams, err := r.QueryModuleStreams(image)
	require.NoError(t, err)
	assert.Equal(t, "18", streams["nodejs"])
	assert.Equal(t, "15", streams["postgresql"])
}

func TestQueryModuleStreams_Caching(t *testing.T) {
	image := "quay.io/test/image:latest"
	fake := buildResolverFake(image)
	r := NewResolver(fake)

	streams1, err := r.QueryModuleStreams(image)
	require.NoError(t, err)

	delete(fake.commands, moduleKey(image))

	streams2, err := r.QueryModuleStreams(image)
	require.NoError(t, err)
	assert.Equal(t, streams1, streams2)
}

func TestQueryModuleStreams_Empty(t *testing.T) {
	image := "quay.io/test/image:latest"
	fake := newFakeRunner(map[string]CommandResult{
		fmt.Sprintf("podman image exists %s", image): {ExitCode: 0},
		moduleKey(image):                              {Stdout: "", ExitCode: 0},
	})
	r := NewResolver(fake)

	streams, err := r.QueryModuleStreams(image)
	require.NoError(t, err)
	assert.Empty(t, streams)
}

// ---------------------------------------------------------------------------
// Registry auth tests
// ---------------------------------------------------------------------------

func TestCheckRegistryAuth_RHEL_NoCredentials(t *testing.T) {
	image := "registry.redhat.io/rhel9/rhel-bootc:9.6"
	fake := newFakeRunner(map[string]CommandResult{
		"podman login --get-login registry.redhat.io": {ExitCode: 1, Stderr: "not logged in"},
	})
	r := NewResolver(fake)

	_, err := r.QueryPackages(image)
	require.Error(t, err)
	assert.Contains(t, err.Error(), "registry auth")
}

func TestCheckRegistryAuth_NonRHEL_NoCheck(t *testing.T) {
	image := "quay.io/centos-bootc/centos-bootc:stream9"
	fake := buildResolverFake(image)
	r := NewResolver(fake)

	pkgs, err := r.QueryPackages(image)
	require.NoError(t, err)
	assert.NotEmpty(t, pkgs)
}

// ---------------------------------------------------------------------------
// ensureImagePulled tests
// ---------------------------------------------------------------------------

func TestEnsureImagePulled_AlreadyCached(t *testing.T) {
	image := "quay.io/test/image:latest"
	fake := newFakeRunner(map[string]CommandResult{
		fmt.Sprintf("podman image exists %s", image): {ExitCode: 0},
	})
	r := NewResolver(fake)

	err := r.ensureImagePulled(image)
	require.NoError(t, err)
}

func TestEnsureImagePulled_PullFails(t *testing.T) {
	image := "quay.io/test/image:latest"
	fake := newFakeRunner(map[string]CommandResult{
		fmt.Sprintf("podman image exists %s", image): {ExitCode: 1},
		fmt.Sprintf("podman pull %s", image):         {ExitCode: 1, Stderr: "network error"},
	})
	r := NewResolver(fake)

	err := r.ensureImagePulled(image)
	require.Error(t, err)
	assert.Contains(t, err.Error(), "podman pull failed")
}
