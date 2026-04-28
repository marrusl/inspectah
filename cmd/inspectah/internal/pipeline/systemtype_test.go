package pipeline

import (
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/inspector"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// DetectSystemType tests
// ---------------------------------------------------------------------------

func TestDetectSystemType_PackageMode(t *testing.T) {
	// No /ostree directory -> package-mode
	fake := inspector.NewFakeExecutor(nil)
	// No files set, so FileExists("/ostree") returns false

	st, err := DetectSystemType(fake)
	require.NoError(t, err)
	assert.Equal(t, schema.SystemTypePackageMode, st)
}

func TestDetectSystemType_Bootc(t *testing.T) {
	// /ostree exists + bootc status succeeds
	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		"bootc status": {Stdout: "running", ExitCode: 0},
	}).WithFiles(map[string]string{
		"/ostree": "", // directory marker
	})

	st, err := DetectSystemType(fake)
	require.NoError(t, err)
	assert.Equal(t, schema.SystemTypeBootc, st)
}

func TestDetectSystemType_RpmOstree(t *testing.T) {
	// /ostree exists + bootc fails + rpm-ostree succeeds
	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		"bootc status":      {Stderr: "not found", ExitCode: 127},
		"rpm-ostree status": {Stdout: "State: idle", ExitCode: 0},
	}).WithFiles(map[string]string{
		"/ostree": "",
	})

	st, err := DetectSystemType(fake)
	require.NoError(t, err)
	assert.Equal(t, schema.SystemTypeRpmOstree, st)
}

func TestDetectSystemType_OstreeDetectionError(t *testing.T) {
	// /ostree exists but both bootc and rpm-ostree fail
	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		"bootc status":      {ExitCode: 127},
		"rpm-ostree status": {ExitCode: 127},
	}).WithFiles(map[string]string{
		"/ostree": "",
	})

	_, err := DetectSystemType(fake)
	require.Error(t, err)

	var oErr *OstreeDetectionError
	assert.ErrorAs(t, err, &oErr)
	assert.Contains(t, err.Error(), "ostree")
}

// ---------------------------------------------------------------------------
// MapOstreeBaseImage tests
// ---------------------------------------------------------------------------

func TestMapBaseImage_TargetImageOverride(t *testing.T) {
	fake := inspector.NewFakeExecutor(nil)
	osRel := &schema.OsRelease{ID: "rhel", VersionID: "9.4"}
	override := "registry.example.com/custom:latest"

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeBootc, override)
	require.NoError(t, err)
	assert.Equal(t, override, ref)
}

func TestMapBaseImage_RHEL9_Bootc(t *testing.T) {
	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		// bootc status --json fails, so we fall back to os-release
		"bootc status --json": {ExitCode: 1},
	})
	osRel := &schema.OsRelease{ID: "rhel", VersionID: "9.4"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeBootc, "")
	require.NoError(t, err)
	assert.Equal(t, "registry.redhat.io/rhel9/rhel-bootc:9.4", ref)
}

func TestMapBaseImage_RHEL10_Bootc(t *testing.T) {
	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		"bootc status --json": {ExitCode: 1},
	})
	osRel := &schema.OsRelease{ID: "rhel", VersionID: "10.0"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeBootc, "")
	require.NoError(t, err)
	assert.Equal(t, "registry.redhat.io/rhel10/rhel-bootc:10.0", ref)
}

func TestMapBaseImage_CentOSStream9_Bootc(t *testing.T) {
	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		"bootc status --json": {ExitCode: 1},
	})
	osRel := &schema.OsRelease{ID: "centos", VersionID: "9"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeBootc, "")
	require.NoError(t, err)
	assert.Equal(t, "quay.io/centos-bootc/centos-bootc:stream9", ref)
}

func TestMapBaseImage_Fedora_Bootc(t *testing.T) {
	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		"bootc status --json": {ExitCode: 1},
	})
	osRel := &schema.OsRelease{ID: "fedora", VersionID: "41"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeBootc, "")
	require.NoError(t, err)
	assert.Equal(t, "quay.io/fedora/fedora-bootc:41", ref)
}

func TestMapBaseImage_BootcStatusJSON(t *testing.T) {
	// bootc status --json succeeds and returns the image ref
	jsonOut := `{"status":{"booted":{"image":{"image":{"image":"registry.redhat.io/rhel9/rhel-bootc:9.4"}}}}}`
	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		"bootc status --json": {Stdout: jsonOut, ExitCode: 0},
	})
	osRel := &schema.OsRelease{ID: "rhel", VersionID: "9.4"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeBootc, "")
	require.NoError(t, err)
	assert.Equal(t, "registry.redhat.io/rhel9/rhel-bootc:9.4", ref)
}

func TestMapBaseImage_RpmOstree_Silverblue(t *testing.T) {
	fake := inspector.NewFakeExecutor(nil)
	osRel := &schema.OsRelease{ID: "fedora", VersionID: "41", VariantID: "silverblue"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeRpmOstree, "")
	require.NoError(t, err)
	assert.Equal(t, "quay.io/fedora-ostree-desktops/silverblue:41", ref)
}

func TestMapBaseImage_RpmOstree_Kinoite(t *testing.T) {
	fake := inspector.NewFakeExecutor(nil)
	osRel := &schema.OsRelease{ID: "fedora", VersionID: "40", VariantID: "kinoite"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeRpmOstree, "")
	require.NoError(t, err)
	assert.Equal(t, "quay.io/fedora-ostree-desktops/kinoite:40", ref)
}

func TestMapBaseImage_RpmOstree_AllDesktopVariants(t *testing.T) {
	variants := []string{
		"silverblue", "kinoite", "sway-atomic", "budgie-atomic",
		"lxqt-atomic", "xfce-atomic", "cosmic-atomic",
	}
	for _, variant := range variants {
		t.Run(variant, func(t *testing.T) {
			fake := inspector.NewFakeExecutor(nil)
			osRel := &schema.OsRelease{ID: "fedora", VersionID: "41", VariantID: variant}
			ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeRpmOstree, "")
			require.NoError(t, err)
			assert.Equal(t, "quay.io/fedora-ostree-desktops/"+variant+":41", ref)
		})
	}
}

func TestMapBaseImage_RpmOstree_UnknownVariant(t *testing.T) {
	fake := inspector.NewFakeExecutor(nil)
	osRel := &schema.OsRelease{ID: "fedora", VersionID: "41", VariantID: "unknown-desktop"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeRpmOstree, "")
	require.NoError(t, err)
	assert.Equal(t, "", ref) // unknown -> empty string
}

func TestMapBaseImage_UnknownOS_Bootc(t *testing.T) {
	fake := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		"bootc status --json": {ExitCode: 1},
	})
	osRel := &schema.OsRelease{ID: "alpine", VersionID: "3.19"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeBootc, "")
	require.NoError(t, err)
	assert.Equal(t, "", ref) // unknown -> empty string
}

func TestMapBaseImage_UBlue_WithImageRef(t *testing.T) {
	ublueJSON := `{"image-name":"bazzite","image-vendor":"ublue-os","image-ref":"ghcr.io/ublue-os/bazzite:stable"}`
	fake := inspector.NewFakeExecutor(nil).WithFiles(map[string]string{
		"/usr/share/ublue-os/image-info.json": ublueJSON,
	})
	osRel := &schema.OsRelease{ID: "fedora", VersionID: "41", VariantID: "silverblue"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeRpmOstree, "")
	require.NoError(t, err)
	assert.Equal(t, "ghcr.io/ublue-os/bazzite:stable", ref)
}

func TestMapBaseImage_UBlue_SynthesizedRef(t *testing.T) {
	// image-ref missing, but vendor/name/tag present -> synthesize
	ublueJSON := `{"image-name":"bazzite","image-vendor":"ublue-os","image-tag":"40"}`
	fake := inspector.NewFakeExecutor(nil).WithFiles(map[string]string{
		"/usr/share/ublue-os/image-info.json": ublueJSON,
	})
	osRel := &schema.OsRelease{ID: "fedora", VersionID: "40"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeBootc, "")
	require.NoError(t, err)
	assert.Equal(t, "ghcr.io/ublue-os/bazzite:40", ref)
}

func TestMapBaseImage_UBlue_MalformedJSON(t *testing.T) {
	fake := inspector.NewFakeExecutor(nil).WithFiles(map[string]string{
		"/usr/share/ublue-os/image-info.json": "not valid json{{{",
	})
	osRel := &schema.OsRelease{ID: "fedora", VersionID: "41"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeBootc, "")
	require.NoError(t, err)
	assert.Equal(t, "", ref) // malformed -> empty string (refuse to guess)
}

func TestMapBaseImage_UBlue_MissingRequiredFields(t *testing.T) {
	// Has valid JSON but missing image-name or image-vendor
	ublueJSON := `{"image-tag":"40"}`
	fake := inspector.NewFakeExecutor(nil).WithFiles(map[string]string{
		"/usr/share/ublue-os/image-info.json": ublueJSON,
	})
	osRel := &schema.OsRelease{ID: "fedora", VersionID: "41"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeBootc, "")
	require.NoError(t, err)
	assert.Equal(t, "", ref)
}

func TestMapBaseImage_TargetOverrideTakesPrecedenceOverUBlue(t *testing.T) {
	ublueJSON := `{"image-name":"bazzite","image-vendor":"ublue-os","image-ref":"ghcr.io/ublue-os/bazzite:stable"}`
	fake := inspector.NewFakeExecutor(nil).WithFiles(map[string]string{
		"/usr/share/ublue-os/image-info.json": ublueJSON,
	})
	osRel := &schema.OsRelease{ID: "fedora", VersionID: "41"}
	override := "registry.example.com/custom:v1"

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypeBootc, override)
	require.NoError(t, err)
	assert.Equal(t, override, ref)
}

func TestMapBaseImage_PackageMode(t *testing.T) {
	// Package-mode should not be passed to MapOstreeBaseImage in practice,
	// but if it is, return empty string (unexpected system type)
	fake := inspector.NewFakeExecutor(nil)
	osRel := &schema.OsRelease{ID: "rhel", VersionID: "9.4"}

	ref, err := MapOstreeBaseImage(fake, osRel, schema.SystemTypePackageMode, "")
	require.NoError(t, err)
	assert.Equal(t, "", ref)
}
