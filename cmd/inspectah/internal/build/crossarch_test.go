package build

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestCrossArchCheck_SamePlatform(t *testing.T) {
	hostArch := runtime.GOARCH
	platform := "linux/" + hostArch
	warnings, err := CrossArchCheck(platform)
	assert.NoError(t, err)
	assert.Empty(t, warnings)
}

func TestCrossArchCheck_EmptyPlatform(t *testing.T) {
	warnings, err := CrossArchCheck("")
	assert.NoError(t, err)
	assert.Empty(t, warnings)
}

func TestCrossArchCheck_CrossArchWarning(t *testing.T) {
	// Pick an arch that differs from the host
	targetArch := "amd64"
	if runtime.GOARCH == "amd64" {
		targetArch = "arm64"
	}
	platform := "linux/" + targetArch

	warnings, err := CrossArchCheck(platform)
	assert.NoError(t, err)
	assert.Len(t, warnings, 1)

	// Warning must always say "linux/<hostArch>" as the build host,
	// even on macOS/Windows, because podman builds run in a Linux VM.
	assert.Contains(t, warnings[0], "linux/"+runtime.GOARCH)
	if runtime.GOOS != "linux" {
		assert.NotContains(t, warnings[0], runtime.GOOS+"/"+runtime.GOARCH,
			"warning should not use runtime.GOOS on non-Linux hosts")
	}
}

func TestCrossArchCheck_InvalidFormat(t *testing.T) {
	_, err := CrossArchCheck("justanarch")
	assert.ErrorContains(t, err, "format")
}

// --- Package mapping tests ---

func TestMapGoArchToRPMArch(t *testing.T) {
	tests := []struct {
		goarch  string
		rpmarch string
	}{
		{"amd64", "x86_64"},
		{"arm64", "aarch64"},
		{"s390x", "s390x"},
		{"ppc64le", "ppc64le"},
		{"unknown", ""},
	}
	for _, tc := range tests {
		t.Run(tc.goarch, func(t *testing.T) {
			assert.Equal(t, tc.rpmarch, MapGoArchToRPMArch(tc.goarch))
		})
	}
}

func TestFindArchSpecificPackages_AArch64ToX86(t *testing.T) {
	cf := `FROM quay.io/centos-bootc/centos-bootc:stream9
RUN dnf install -y \
    grub2-efi-aa64 \
    grub2-efi-aa64-cdboot \
    grub2-efi-aa64-modules \
    shim-aa64 \
    kernel-core \
    vim
`
	subs, unmapped, err := FindArchSpecificPackages(cf, "aarch64", "x86_64")
	require.NoError(t, err)
	assert.Len(t, subs, 4)
	assert.Empty(t, unmapped)

	// Verify specific substitutions
	expected := map[string]string{
		"grub2-efi-aa64":         "grub2-efi-x64",
		"grub2-efi-aa64-cdboot":  "grub2-efi-x64-cdboot",
		"grub2-efi-aa64-modules": "grub2-efi-x64-modules",
		"shim-aa64":              "shim-x64",
	}
	for _, s := range subs {
		exp, ok := expected[s.From]
		assert.True(t, ok, "unexpected substitution from %q", s.From)
		assert.Equal(t, exp, s.To)
	}
}

func TestFindArchSpecificPackages_X86ToAArch64(t *testing.T) {
	cf := `FROM quay.io/centos-bootc/centos-bootc:stream9
RUN dnf install -y grub2-efi-x64 grub2-efi-x64-cdboot shim-x64
`
	subs, unmapped, err := FindArchSpecificPackages(cf, "x86_64", "aarch64")
	require.NoError(t, err)
	assert.Len(t, subs, 3)
	assert.Empty(t, unmapped)

	// Verify all expected substitutions are present (order may vary).
	expected := map[string]string{
		"grub2-efi-x64":        "grub2-efi-aa64",
		"grub2-efi-x64-cdboot": "grub2-efi-aa64-cdboot",
		"shim-x64":             "shim-aa64",
	}
	for _, s := range subs {
		exp, ok := expected[s.From]
		assert.True(t, ok, "unexpected substitution from %q", s.From)
		assert.Equal(t, exp, s.To)
	}
}

func TestFindArchSpecificPackages_UnmappedX86Only(t *testing.T) {
	cf := `FROM quay.io/centos-bootc/centos-bootc:stream9
RUN dnf install -y grub2-pc grub2-pc-modules grub2-efi-ia32
`
	subs, unmapped, err := FindArchSpecificPackages(cf, "x86_64", "aarch64")
	require.NoError(t, err)
	assert.Empty(t, subs)
	assert.Len(t, unmapped, 3)
	assert.Contains(t, unmapped, "grub2-pc")
	assert.Contains(t, unmapped, "grub2-pc-modules")
	assert.Contains(t, unmapped, "grub2-efi-ia32")
}

func TestFindArchSpecificPackages_NoArchPackages(t *testing.T) {
	cf := `FROM quay.io/centos-bootc/centos-bootc:stream9
RUN dnf install -y kernel-core vim-enhanced bash-completion
`
	subs, unmapped, err := FindArchSpecificPackages(cf, "aarch64", "x86_64")
	require.NoError(t, err)
	assert.Empty(t, subs)
	assert.Empty(t, unmapped)
}

func TestFindArchSpecificPackages_SameArch(t *testing.T) {
	cf := `FROM quay.io/centos-bootc/centos-bootc:stream9
RUN dnf install -y grub2-efi-x64 shim-x64
`
	subs, unmapped, err := FindArchSpecificPackages(cf, "x86_64", "x86_64")
	require.NoError(t, err)
	assert.Empty(t, subs)
	assert.Empty(t, unmapped)
}

func TestFindArchSpecificPackages_MixedMappableAndUnmappable(t *testing.T) {
	cf := `FROM quay.io/centos-bootc/centos-bootc:stream9
RUN dnf install -y grub2-efi-aa64 grub2-pc shim-aa64
`
	subs, unmapped, err := FindArchSpecificPackages(cf, "aarch64", "x86_64")
	require.NoError(t, err)
	// grub2-efi-aa64 and shim-aa64 map to x64 equivalents
	assert.Len(t, subs, 2)
	// grub2-pc is x86_64-only; since we're targeting x86_64 it would be
	// detected as belonging to the target arch, not unmapped.
	// Only packages belonging to the source arch but unmappable are unmapped.
	assert.Empty(t, unmapped)
}

func TestFindArchSpecificPackages_TargetHasX86OnlyWithNoEquiv(t *testing.T) {
	// Building from x86_64 source to aarch64 target, x86-only packages
	// with no aarch64 equivalent should show up as unmapped
	cf := `FROM quay.io/centos-bootc/centos-bootc:stream9
RUN dnf install -y grub2-efi-x64 grub2-pc shim-x64
`
	subs, unmapped, err := FindArchSpecificPackages(cf, "x86_64", "aarch64")
	require.NoError(t, err)
	assert.Len(t, subs, 2) // grub2-efi-x64->aa64, shim-x64->aa64
	assert.Len(t, unmapped, 1)
	assert.Contains(t, unmapped, "grub2-pc")
}

func TestInferSourceArch(t *testing.T) {
	tests := []struct {
		name     string
		cf       string
		expected string
	}{
		{
			name:     "aarch64 packages",
			cf:       "RUN dnf install -y grub2-efi-aa64-cdboot shim-aa64",
			expected: "aarch64",
		},
		{
			name:     "x86_64 packages",
			cf:       "RUN dnf install -y grub2-efi-x64 grub2-pc",
			expected: "x86_64",
		},
		{
			name:     "no arch packages",
			cf:       "RUN dnf install -y kernel-core vim",
			expected: "",
		},
		{
			name:     "ia32 implies x86_64",
			cf:       "RUN dnf install -y grub2-efi-ia32",
			expected: "x86_64",
		},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			assert.Equal(t, tc.expected, InferSourceArch(tc.cf))
		})
	}
}

// --- Containerfile rewriting tests ---

func TestApplySubstitutions(t *testing.T) {
	original := `FROM quay.io/centos-bootc/centos-bootc:stream9
RUN dnf install -y \
    grub2-efi-aa64 \
    grub2-efi-aa64-cdboot \
    shim-aa64 \
    kernel-core \
    vim
`
	subs := []PkgSubstitution{
		{From: "grub2-efi-aa64", To: "grub2-efi-x64"},
		{From: "grub2-efi-aa64-cdboot", To: "grub2-efi-x64-cdboot"},
		{From: "shim-aa64", To: "shim-x64"},
	}

	result := ApplySubstitutions(original, subs)

	assert.Contains(t, result, "grub2-efi-x64")
	assert.Contains(t, result, "grub2-efi-x64-cdboot")
	assert.Contains(t, result, "shim-x64")
	assert.NotContains(t, result, "grub2-efi-aa64")
	assert.NotContains(t, result, "shim-aa64")
	// Non-arch packages should be unchanged
	assert.Contains(t, result, "kernel-core")
	assert.Contains(t, result, "vim")
}

func TestApplySubstitutions_PreservesFormatting(t *testing.T) {
	original := `RUN dnf install -y grub2-efi-aa64 shim-aa64`
	subs := []PkgSubstitution{
		{From: "grub2-efi-aa64", To: "grub2-efi-x64"},
		{From: "shim-aa64", To: "shim-x64"},
	}
	result := ApplySubstitutions(original, subs)
	assert.Equal(t, "RUN dnf install -y grub2-efi-x64 shim-x64", result)
}

func TestApplySubstitutions_NoPartialMatch(t *testing.T) {
	// "grub2-efi-aa64" substitution should NOT replace inside
	// "grub2-efi-aa64-cdboot" — longer match must be handled first
	original := `RUN dnf install -y grub2-efi-aa64-cdboot grub2-efi-aa64`
	subs := []PkgSubstitution{
		{From: "grub2-efi-aa64-cdboot", To: "grub2-efi-x64-cdboot"},
		{From: "grub2-efi-aa64", To: "grub2-efi-x64"},
	}
	result := ApplySubstitutions(original, subs)
	assert.Equal(t, "RUN dnf install -y grub2-efi-x64-cdboot grub2-efi-x64", result)
}

// --- WriteTempContainerfile tests ---

func TestWriteTempContainerfile(t *testing.T) {
	dir := t.TempDir()
	content := "FROM centos:stream9\nRUN dnf install -y grub2-efi-x64\n"

	path, cleanup, err := WriteTempContainerfile(dir, content)
	require.NoError(t, err)
	defer cleanup()

	assert.FileExists(t, path)
	assert.Equal(t, dir, filepath.Dir(path))

	data, err := os.ReadFile(path)
	require.NoError(t, err)
	assert.Equal(t, content, string(data))

	// Cleanup removes the file
	cleanup()
	assert.NoFileExists(t, path)
}

func TestCrossArchSubstitute_FullFlow(t *testing.T) {
	cfContent := `FROM quay.io/centos-bootc/centos-bootc:stream9
RUN dnf install -y \
    grub2-efi-aa64 \
    grub2-efi-aa64-cdboot \
    grub2-efi-aa64-modules \
    shim-aa64 \
    kernel-core
`
	result, err := CrossArchSubstitute(cfContent, "arm64", "amd64", false)
	require.NoError(t, err)
	assert.Len(t, result.Substitutions, 4)
	assert.Empty(t, result.Unmapped)
	assert.Contains(t, result.ModifiedContent, "grub2-efi-x64")
	assert.Contains(t, result.ModifiedContent, "shim-x64")
	assert.NotContains(t, result.ModifiedContent, "grub2-efi-aa64")
}

func TestCrossArchSubstitute_StrictMode(t *testing.T) {
	cfContent := `FROM quay.io/centos-bootc/centos-bootc:stream9
RUN dnf install -y grub2-efi-aa64 shim-aa64
`
	_, err := CrossArchSubstitute(cfContent, "arm64", "amd64", true)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "grub2-efi-aa64")
	assert.Contains(t, err.Error(), "grub2-efi-x64")
}

func TestCrossArchSubstitute_StrictModeNoArchPkgs(t *testing.T) {
	cfContent := `FROM quay.io/centos-bootc/centos-bootc:stream9
RUN dnf install -y kernel-core vim
`
	result, err := CrossArchSubstitute(cfContent, "arm64", "amd64", true)
	require.NoError(t, err)
	assert.Empty(t, result.Substitutions)
}

func TestCrossArchSubstitute_UnmappedError(t *testing.T) {
	cfContent := `FROM quay.io/centos-bootc/centos-bootc:stream9
RUN dnf install -y grub2-pc grub2-efi-x64
`
	// x86_64 -> aarch64: grub2-pc has no aarch64 equivalent
	_, err := CrossArchSubstitute(cfContent, "amd64", "arm64", false)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "grub2-pc")
	assert.Contains(t, err.Error(), "no equivalent")
}
