package inspector

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

func loadNonRpmFixture(t *testing.T, name string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "nonrpm", name))
	require.NoError(t, err, "loading fixture %s", name)
	return string(data)
}

// ---------------------------------------------------------------------------
// Dev artifact filtering
// ---------------------------------------------------------------------------

func TestIsDevArtifactRel(t *testing.T) {
	tests := []struct {
		name     string
		relPath  string
		expected bool
	}{
		{
			name:     "normal path",
			relPath:  "opt/myapp/bin/server",
			expected: false,
		},
		{
			name:     "git directory",
			relPath:  "opt/myapp/.git/config",
			expected: true,
		},
		{
			name:     "node_modules",
			relPath:  "opt/myapp/node_modules/express",
			expected: true,
		},
		{
			name:     "__pycache__",
			relPath:  "opt/myapp/__pycache__/mod.pyc",
			expected: true,
		},
		{
			name:     ".vscode",
			relPath:  "opt/project/.vscode/settings.json",
			expected: true,
		},
		{
			name:     ".idea",
			relPath:  "opt/project/.idea/workspace.xml",
			expected: true,
		},
		{
			name:     "svn directory",
			relPath:  "opt/legacy/.svn/entries",
			expected: true,
		},
		{
			name:     ".mypy_cache",
			relPath:  "opt/app/.mypy_cache/3.11/foo.json",
			expected: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			assert.Equal(t, tt.expected, isDevArtifactRel(tt.relPath))
		})
	}
}

// ---------------------------------------------------------------------------
// Binary classification
// ---------------------------------------------------------------------------

func TestClassifyBinary_GoStatic(t *testing.T) {
	sectionsOut := loadNonRpmFixture(t, "readelf-sections-go.txt")
	dynamicOut := loadNonRpmFixture(t, "readelf-dynamic-static.txt")

	fake := NewFakeExecutor(map[string]ExecResult{
		"readelf --version":         {Stdout: "readelf 2.35", ExitCode: 0},
		"readelf -S /opt/myapp/bin": {Stdout: sectionsOut, ExitCode: 0},
		"readelf -d /opt/myapp/bin": {Stdout: dynamicOut, ExitCode: 0},
	})

	tools := binaryTools{hasReadelf: true, hasFile: true}
	bc := classifyBinary(fake, tools, "/opt/myapp/bin")

	require.NotNil(t, bc)
	assert.Equal(t, "go", bc.lang)
	assert.True(t, bc.static)
	assert.Empty(t, bc.sharedLibs)
}

func TestClassifyBinary_DynamicLinked(t *testing.T) {
	dynamicOut := loadNonRpmFixture(t, "readelf-dynamic-linked.txt")

	fake := NewFakeExecutor(map[string]ExecResult{
		"readelf -S /opt/myapp/bin": {Stdout: ".text PROGBITS", ExitCode: 0},
		"readelf -d /opt/myapp/bin": {Stdout: dynamicOut, ExitCode: 0},
	})

	tools := binaryTools{hasReadelf: true, hasFile: true}
	bc := classifyBinary(fake, tools, "/opt/myapp/bin")

	require.NotNil(t, bc)
	assert.Equal(t, "c/c++", bc.lang)
	assert.False(t, bc.static)
	assert.Contains(t, bc.sharedLibs, "libpthread.so.0")
	assert.Contains(t, bc.sharedLibs, "libc.so.6")
}

func TestClassifyBinary_NoReadelf(t *testing.T) {
	fake := NewFakeExecutor(nil)
	tools := binaryTools{hasReadelf: false, hasFile: false}

	bc := classifyBinary(fake, tools, "/opt/myapp/bin")
	assert.Nil(t, bc)
}

func TestClassifyBinary_ReadelfFails(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"readelf -S /opt/somefile": {Stderr: "not an ELF", ExitCode: 1},
	})
	tools := binaryTools{hasReadelf: true, hasFile: true}

	bc := classifyBinary(fake, tools, "/opt/somefile")
	assert.Nil(t, bc)
}

// ---------------------------------------------------------------------------
// isBinary
// ---------------------------------------------------------------------------

func TestIsBinary(t *testing.T) {
	tests := []struct {
		name     string
		fileOut  string
		expected bool
	}{
		{
			name:     "ELF binary",
			fileOut:  "ELF 64-bit LSB executable, x86-64",
			expected: true,
		},
		{
			name:     "shell script",
			fileOut:  "Bourne-Again shell script, ASCII text executable",
			expected: true,
		},
		{
			name:     "python script",
			fileOut:  "Python script, ASCII text executable",
			expected: true,
		},
		{
			name:     "plain text",
			fileOut:  "ASCII text",
			expected: false,
		},
		{
			name:     "JPEG image",
			fileOut:  "JPEG image data",
			expected: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			fake := NewFakeExecutor(map[string]ExecResult{
				"file -b /opt/test": {Stdout: tt.fileOut, ExitCode: 0},
			})
			tools := binaryTools{hasReadelf: true, hasFile: true}
			assert.Equal(t, tt.expected, isBinary(fake, tools, "/opt/test"))
		})
	}
}

func TestIsBinary_FileCommandUnavailable(t *testing.T) {
	fake := NewFakeExecutor(nil)
	tools := binaryTools{hasReadelf: true, hasFile: false}
	assert.False(t, isBinary(fake, tools, "/opt/test"))
}

// ---------------------------------------------------------------------------
// Version extraction
// ---------------------------------------------------------------------------

func TestStringsVersion(t *testing.T) {
	tests := []struct {
		name    string
		output  string
		deep    bool
		want    string
	}{
		{
			name:   "version equals pattern",
			output: "version = 1.2.3\nother stuff",
			want:   "1.2.3",
		},
		{
			name:   "v prefix pattern",
			output: "something v2.0.1-beta stuff",
			want:   "2.0.1",
		},
		{
			name:   "semver pattern",
			output: "lib 3.14.159\n",
			want:   "3.14.159",
		},
		{
			name:   "go version deep",
			output: "go1.21.5 something",
			deep:   true,
			want:   "1.21.5",
		},
		{
			name:   "no version",
			output: "no version info here",
			want:   "",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			fake := NewFakeExecutor(map[string]ExecResult{
				"strings /opt/bin": {Stdout: tt.output, ExitCode: 0},
			})
			got := stringsVersion(fake, "/opt/bin", 0, tt.deep)
			assert.Equal(t, tt.want, got)
		})
	}
}

func TestStringsVersion_LimitedHead(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"sh -c head -c 4096 /opt/bin | strings": {
			Stdout:   "version = 5.6.7\n",
			ExitCode: 0,
		},
	})
	got := stringsVersion(fake, "/opt/bin", 4, false)
	assert.Equal(t, "5.6.7", got)
}

// ---------------------------------------------------------------------------
// Git repository detection
// ---------------------------------------------------------------------------

func TestScanGitRepo(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/opt/myapp/.git/config": loadNonRpmFixture(t, "git-config"),
			"/opt/myapp/.git/HEAD":   "ref: refs/heads/main\n",
			"/opt/myapp/.git/refs/heads/main": "abc123def456\n",
		})
	// Mark .git as existing.
	fake.WithDirs(map[string][]string{
		"/opt/myapp/.git": {"config", "HEAD", "refs"},
	})

	item := scanGitRepo(fake, "/opt/myapp", "opt/myapp")

	require.NotNil(t, item)
	assert.Equal(t, "opt/myapp", item.Path)
	assert.Equal(t, "myapp", item.Name)
	assert.Equal(t, "git repository", item.Method)
	assert.Equal(t, "high", item.Confidence)
	assert.Equal(t, "https://github.com/example/myapp.git", item.GitRemote)
	assert.Equal(t, "abc123def456", item.GitCommit)
	assert.Equal(t, "main", item.GitBranch)
}

func TestScanGitRepo_DetachedHead(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/opt/app/.git/config": "[remote]\n",
			"/opt/app/.git/HEAD":   "deadbeef12345678\n",
		})
	fake.WithDirs(map[string][]string{
		"/opt/app/.git": {"config", "HEAD"},
	})

	item := scanGitRepo(fake, "/opt/app", "opt/app")

	require.NotNil(t, item)
	assert.Equal(t, "deadbeef12345678", item.GitCommit)
	assert.Equal(t, "", item.GitBranch)
}

func TestScanGitRepo_NoGitDir(t *testing.T) {
	fake := NewFakeExecutor(nil)
	item := scanGitRepo(fake, "/opt/app", "opt/app")
	assert.Nil(t, item)
}

// ---------------------------------------------------------------------------
// Venv detection
// ---------------------------------------------------------------------------

func TestFindVenvs(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/opt/myapp/venv/pyvenv.cfg": loadNonRpmFixture(t, "pyvenv.cfg"),
		}).
		WithDirs(map[string][]string{
			"/opt":            {"myapp"},
			"/opt/myapp":      {"venv"},
			"/opt/myapp/venv": {"pyvenv.cfg", "bin", "lib"},
		})

	venvs := findVenvs(fake, "/opt")

	require.Len(t, venvs, 1)
	assert.Equal(t, "/opt/myapp/venv", venvs[0].path)
	assert.True(t, venvs[0].systemSitePackages)
}

func TestFindVenvs_NoSystemSitePackages(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/srv/app/venv/pyvenv.cfg": "home = /usr/bin\ninclude-system-site-packages = false\nversion = 3.9.0\n",
		}).
		WithDirs(map[string][]string{
			"/srv":          {"app"},
			"/srv/app":      {"venv"},
			"/srv/app/venv": {"pyvenv.cfg"},
		})

	venvs := findVenvs(fake, "/srv")

	require.Len(t, venvs, 1)
	assert.False(t, venvs[0].systemSitePackages)
}

// ---------------------------------------------------------------------------
// pip list parsing
// ---------------------------------------------------------------------------

func TestParsePipList(t *testing.T) {
	input := loadNonRpmFixture(t, "pip-list-output.txt")
	packages := parsePipList(input)

	require.Len(t, packages, 4)
	assert.Equal(t, "flask", packages[0].Name)
	assert.Equal(t, "2.3.3", packages[0].Version)
	assert.Equal(t, "gunicorn", packages[2].Name)
	assert.Equal(t, "21.2.0", packages[2].Version)
}

func TestParsePipList_Empty(t *testing.T) {
	packages := parsePipList("")
	assert.Empty(t, packages)
}

// ---------------------------------------------------------------------------
// dist-info name parsing
// ---------------------------------------------------------------------------

func TestParseDistInfoName(t *testing.T) {
	tests := []struct {
		input       string
		wantName    string
		wantVersion string
	}{
		{"flask-2.3.3", "flask", "2.3.3"},
		{"requests-2.31.0", "requests", "2.31.0"},
		{"my_package-1.0.0", "my_package", "1.0.0"},
		{"noversion", "noversion", ""},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			name, version := parseDistInfoName(tt.input)
			assert.Equal(t, tt.wantName, name)
			assert.Equal(t, tt.wantVersion, version)
		})
	}
}

// ---------------------------------------------------------------------------
// npm detection
// ---------------------------------------------------------------------------

func TestScanNpm(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/opt/webapp/package-lock.json": `{"name": "webapp"}`,
			"/opt/webapp/package.json":      `{"name": "webapp", "version": "1.0.0"}`,
		}).
		WithDirs(map[string][]string{
			"/opt":        {"webapp"},
			"/opt/webapp": {"package-lock.json", "package.json", "src"},
		})

	section := &schema.NonRpmSoftwareSection{
		Items:    []schema.NonRpmItem{},
		EnvFiles: []schema.ConfigFileEntry{},
	}

	scanNpm(fake, section, false)

	require.Len(t, section.Items, 1)
	assert.Equal(t, "opt/webapp", section.Items[0].Path)
	assert.Equal(t, "webapp", section.Items[0].Name)
	assert.Equal(t, "npm package-lock.json", section.Items[0].Method)
	assert.Equal(t, "high", section.Items[0].Confidence)
	require.NotNil(t, section.Items[0].Files)
	files := *section.Items[0].Files
	assert.Contains(t, files, "package-lock.json")
	assert.Contains(t, files, "package.json")
}

// ---------------------------------------------------------------------------
// Gem detection
// ---------------------------------------------------------------------------

func TestScanGem(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/srv/railsapp/Gemfile.lock": "GEM\n  remote: https://rubygems.org/\n",
			"/srv/railsapp/Gemfile":      "source 'https://rubygems.org'\ngem 'rails'\n",
		}).
		WithDirs(map[string][]string{
			"/srv":          {"railsapp"},
			"/srv/railsapp": {"Gemfile.lock", "Gemfile", "app"},
		})

	section := &schema.NonRpmSoftwareSection{
		Items:    []schema.NonRpmItem{},
		EnvFiles: []schema.ConfigFileEntry{},
	}

	scanGem(fake, section, false)

	require.Len(t, section.Items, 1)
	assert.Equal(t, "srv/railsapp", section.Items[0].Path)
	assert.Equal(t, "gem Gemfile.lock", section.Items[0].Method)
}

// ---------------------------------------------------------------------------
// .env file scanning
// ---------------------------------------------------------------------------

func TestScanEnvFiles(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/opt/app/.env":            "DB_HOST=localhost\nDB_PASS=secret123\n",
			"/opt/app/.env.production": "DB_HOST=prod.example.com\n",
			"/opt/app/.env.bak":        "should be ignored",
		}).
		WithDirs(map[string][]string{
			"/opt":     {"app"},
			"/opt/app": {".env", ".env.production", ".env.bak"},
		})

	section := &schema.NonRpmSoftwareSection{
		Items:    []schema.NonRpmItem{},
		EnvFiles: []schema.ConfigFileEntry{},
	}

	scanEnvFiles(fake, section)

	require.Len(t, section.EnvFiles, 2)

	paths := make(map[string]bool)
	for _, e := range section.EnvFiles {
		paths[e.Path] = true
		assert.Equal(t, schema.ConfigFileKindUnowned, e.Kind)
	}
	assert.True(t, paths["opt/app/.env"])
	assert.True(t, paths["opt/app/.env.production"])
	// .env.bak should NOT be included.
	assert.False(t, paths["opt/app/.env.bak"])
}

// ---------------------------------------------------------------------------
// Deduplication
// ---------------------------------------------------------------------------

func TestDeduplicateItems(t *testing.T) {
	section := &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{Path: "opt/app", Name: "app", Confidence: "low", Method: "directory scan"},
			{Path: "opt/app", Name: "app", Confidence: "high", Method: "readelf (go)"},
			{Path: "opt/other", Name: "other", Confidence: "medium", Method: "strings"},
		},
	}

	deduplicateItems(section)

	require.Len(t, section.Items, 2)
	assert.Equal(t, "opt/app", section.Items[0].Path)
	assert.Equal(t, "high", section.Items[0].Confidence)
	assert.Equal(t, "readelf (go)", section.Items[0].Method)
	assert.Equal(t, "opt/other", section.Items[1].Path)
}

func TestDeduplicateItems_PreservesOrder(t *testing.T) {
	section := &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{Path: "opt/z", Name: "z", Confidence: "low"},
			{Path: "opt/a", Name: "a", Confidence: "high"},
			{Path: "opt/m", Name: "m", Confidence: "medium"},
		},
	}

	deduplicateItems(section)

	require.Len(t, section.Items, 3)
	assert.Equal(t, "opt/z", section.Items[0].Path)
	assert.Equal(t, "opt/a", section.Items[1].Path)
	assert.Equal(t, "opt/m", section.Items[2].Path)
}

// ---------------------------------------------------------------------------
// Ostree filtering
// ---------------------------------------------------------------------------

func TestFilterOstreeVarPaths(t *testing.T) {
	section := &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{Path: "opt/myapp", Name: "myapp"},
			{Path: "var/lib/ostree/deploy", Name: "deploy"},
			{Path: "var/lib/rpm-ostree/data", Name: "data"},
			{Path: "var/lib/flatpak/app", Name: "app"},
			{Path: "var/lib/myservice", Name: "myservice"},
		},
	}

	filterOstreeVarPaths(section)

	require.Len(t, section.Items, 2)
	assert.Equal(t, "opt/myapp", section.Items[0].Path)
	assert.Equal(t, "var/lib/myservice", section.Items[1].Path)
}

// ---------------------------------------------------------------------------
// Tool probe
// ---------------------------------------------------------------------------

func TestProbeCommand(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"readelf --version": {Stdout: "readelf 2.35", ExitCode: 0},
		"file --version":    {Stdout: "file-5.41", ExitCode: 0},
	})

	assert.True(t, probeCommand(fake, "readelf"))
	assert.True(t, probeCommand(fake, "file"))
	assert.False(t, probeCommand(fake, "nonexistent"))
}

// ---------------------------------------------------------------------------
// Integration test: RunNonRpmSoftware
// ---------------------------------------------------------------------------

func TestRunNonRpmSoftware_Basic(t *testing.T) {
	goSections := loadNonRpmFixture(t, "readelf-sections-go.txt")
	goDynamic := loadNonRpmFixture(t, "readelf-dynamic-static.txt")

	fake := NewFakeExecutor(map[string]ExecResult{
		"readelf --version": {Stdout: "readelf 2.35", ExitCode: 0},
		"file --version":    {Stdout: "file-5.41", ExitCode: 0},
		// Binary classification for the go binary inside myapp.
		"readelf -S /opt/myapp/gobin":         {Stdout: goSections, ExitCode: 0},
		"readelf -d /opt/myapp/gobin":         {Stdout: goDynamic, ExitCode: 0},
		"file -b /opt/myapp/gobin":            {Stdout: "ELF 64-bit LSB executable", ExitCode: 0},
	}).
		WithFiles(map[string]string{
			"/opt/myapp/.git/config":          loadNonRpmFixture(t, "git-config"),
			"/opt/myapp/.git/HEAD":            "ref: refs/heads/main\n",
			"/opt/myapp/.git/refs/heads/main": "abc123def456\n",
			"/opt/envapp/.env":                "SECRET=value\n",
		}).
		WithDirs(map[string][]string{
			"/opt":        {"myapp", "envapp"},
			"/opt/myapp":  {"gobin", ".git"},
			"/opt/myapp/.git": {"config", "HEAD", "refs"},
			"/opt/envapp": {".env", "app.py"},
		})

	section, warnings, err := RunNonRpmSoftware(fake, NonRpmOptions{
		DeepBinaryScan: false,
		SystemType:     schema.SystemTypePackageMode,
	})

	require.NoError(t, err)
	assert.Empty(t, warnings) // readelf and file are available
	require.NotNil(t, section)

	// Should find: git repo (myapp) + envapp (directory scan).
	require.GreaterOrEqual(t, len(section.Items), 2)

	// Verify git repo detected.
	var gitItem *schema.NonRpmItem
	for i := range section.Items {
		if section.Items[i].Method == "git repository" {
			gitItem = &section.Items[i]
			break
		}
	}
	require.NotNil(t, gitItem)
	assert.Equal(t, "myapp", gitItem.Name)
	assert.Equal(t, "https://github.com/example/myapp.git", gitItem.GitRemote)

	// Verify env file detected.
	require.Len(t, section.EnvFiles, 1)
	assert.Equal(t, "opt/envapp/.env", section.EnvFiles[0].Path)
	assert.Equal(t, schema.ConfigFileKindUnowned, section.EnvFiles[0].Kind)
}

func TestRunNonRpmSoftware_OstreeSkipsUsrLocal(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"readelf --version": {Stdout: "readelf 2.35", ExitCode: 0},
		"file --version":    {Stdout: "file-5.41", ExitCode: 0},
	}).
		WithDirs(map[string][]string{
			"/opt":       {"app"},
			"/opt/app":   {"run.sh"},
			"/usr/local": {"bin", "lib"},
		})

	section, _, err := RunNonRpmSoftware(fake, NonRpmOptions{
		SystemType: schema.SystemTypeBootc,
	})

	require.NoError(t, err)

	// /usr/local should NOT be scanned for ostree.
	for _, item := range section.Items {
		assert.False(t, hasPrefix(item.Path, "usr/local"),
			"ostree should not scan /usr/local, found: %s", item.Path)
	}
}

func TestRunNonRpmSoftware_WarningsWhenToolsMissing(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/opt": {},
		})

	_, warnings, err := RunNonRpmSoftware(fake, NonRpmOptions{
		SystemType: schema.SystemTypePackageMode,
	})

	require.NoError(t, err)
	require.Len(t, warnings, 1)
	assert.Contains(t, warnings[0]["message"].(string), "readelf not available")
}

func TestRunNonRpmSoftware_PipDistInfo(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"readelf --version": {Stdout: "readelf 2.35", ExitCode: 0},
		"file --version":    {Stdout: "file-5.41", ExitCode: 0},
	}).
		WithFiles(map[string]string{
			"/usr/lib/python3.11/site-packages/requests-2.31.0.dist-info/RECORD": "requests/__init__.py,sha256=...,1234\nrequests/_internal.cpython-311-x86_64-linux-gnu.so,sha256=...,5678\n",
		}).
		WithDirs(map[string][]string{
			"/opt":          {},
			"/usr/lib":      {"python3.11"},
			"/usr/lib/python3.11": {"site-packages"},
			"/usr/lib/python3.11/site-packages": {"requests-2.31.0.dist-info"},
			"/usr/lib/python3.11/site-packages/requests-2.31.0.dist-info": {"RECORD", "METADATA"},
		})

	section, _, err := RunNonRpmSoftware(fake, NonRpmOptions{
		SystemType: schema.SystemTypePackageMode,
	})

	require.NoError(t, err)

	var pipItem *schema.NonRpmItem
	for i := range section.Items {
		if section.Items[i].Method == "pip dist-info" {
			pipItem = &section.Items[i]
			break
		}
	}
	require.NotNil(t, pipItem, "should detect pip dist-info package")
	assert.Equal(t, "requests", pipItem.Name)
	assert.Equal(t, "2.31.0", pipItem.Version)
	assert.True(t, pipItem.HasCExtensions, "should detect .so as C extension")
}

// ---------------------------------------------------------------------------
// scanPip RPM ownership filtering
// ---------------------------------------------------------------------------

func TestScanPip_SkipsRpmOwnedDistInfo(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		// rpm -qf returns the owning package — this dist-info is RPM-managed
		"rpm -qf /usr/lib/python3.12/site-packages/dnf-4.18.0.dist-info": {
			ExitCode: 0,
			Stdout:   "python3-dnf-4.18.0-1.fc40.noarch",
		},
		// rpm -qf fails — this dist-info is pip-installed
		"rpm -qf /usr/lib/python3.12/site-packages/requests-2.31.0.dist-info": {
			ExitCode: 1,
			Stderr:   "file /usr/lib/python3.12/site-packages/requests-2.31.0.dist-info is not owned by any package",
		},
	}).WithDirs(map[string][]string{
		"/usr/lib":                          {"python3.12"},
		"/usr/lib/python3.12":               {"site-packages"},
		"/usr/lib/python3.12/site-packages": {"dnf-4.18.0.dist-info", "requests-2.31.0.dist-info"},
		"/usr/lib/python3.12/site-packages/dnf-4.18.0.dist-info":      {"RECORD"},
		"/usr/lib/python3.12/site-packages/requests-2.31.0.dist-info": {"RECORD"},
	}).WithFiles(map[string]string{
		"/usr/lib/python3.12/site-packages/dnf-4.18.0.dist-info/RECORD":      "",
		"/usr/lib/python3.12/site-packages/requests-2.31.0.dist-info/RECORD": "",
	})

	section := &schema.NonRpmSoftwareSection{}
	scanPip(exec, section, false)

	// Should only find requests, not dnf
	if len(section.Items) != 1 {
		t.Fatalf("got %d items, want 1 (only non-RPM pip packages)", len(section.Items))
	}
	if section.Items[0].Name != "requests" {
		t.Errorf("item name = %q, want %q", section.Items[0].Name, "requests")
	}
}

func TestScanPip_SkipsRpmCheckOnOstree(t *testing.T) {
	// On ostree systems, scanPip scans /usr/local/ paths only.
	// RPM check should not be invoked because ostree paths are
	// outside RPM's domain. This test verifies the scanner still
	// finds items in /usr/local/ without rpm -qf calls.
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/usr/local/lib":                          {"python3.12"},
			"/usr/local/lib/python3.12":               {"site-packages"},
			"/usr/local/lib/python3.12/site-packages": {"custom_lib-1.0.0.dist-info"},
			"/usr/local/lib/python3.12/site-packages/custom_lib-1.0.0.dist-info": {"RECORD"},
		}).
		WithFiles(map[string]string{
			"/usr/local/lib/python3.12/site-packages/custom_lib-1.0.0.dist-info/RECORD": "",
		})

	section := &schema.NonRpmSoftwareSection{}
	scanPip(exec, section, true)

	if len(section.Items) != 1 {
		t.Fatalf("got %d items, want 1", len(section.Items))
	}
	if section.Items[0].Name != "custom_lib" {
		t.Errorf("item name = %q, want %q", section.Items[0].Name, "custom_lib")
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func hasPrefix(s, prefix string) bool {
	return len(s) >= len(prefix) && s[:len(prefix)] == prefix
}

// ---------------------------------------------------------------------------
// itoa
// ---------------------------------------------------------------------------

func TestItoa(t *testing.T) {
	tests := []struct {
		input int
		want  string
	}{
		{0, "0"},
		{1, "1"},
		{42, "42"},
		{1024, "1024"},
		{4096, "4096"},
	}

	for _, tt := range tests {
		t.Run(tt.want, func(t *testing.T) {
			assert.Equal(t, tt.want, itoa(tt.input))
		})
	}
}
