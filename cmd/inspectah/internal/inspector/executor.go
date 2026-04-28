// Package inspector provides the Executor abstraction for running commands
// and reading files from the host filesystem. In production, RealExecutor
// uses os/exec and os.ReadFile. In tests, FakeExecutor returns canned data.
package inspector

import (
	"fmt"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

// ExecResult holds the output of a command execution.
type ExecResult struct {
	Stdout   string
	Stderr   string
	ExitCode int
}

// Executor abstracts host interaction — command execution and filesystem
// access. Inspectors accept an Executor so tests can inject canned data.
type Executor interface {
	// Run executes a command and returns its output.
	Run(name string, args ...string) ExecResult

	// ReadFile reads a file from the host filesystem.
	// Path is relative to the host root (e.g. "/etc/os-release").
	ReadFile(path string) (string, error)

	// FileExists checks whether a path exists on the host filesystem.
	FileExists(path string) bool

	// ReadDir lists directory entries at the given host path.
	ReadDir(path string) ([]os.DirEntry, error)

	// HostRoot returns the host root path (e.g. "/" or "/sysroot").
	HostRoot() string
}

// ---------------------------------------------------------------------------
// RealExecutor — production implementation
// ---------------------------------------------------------------------------

// RealExecutor runs real commands and reads real files, prepending hostRoot
// to all filesystem paths.
type RealExecutor struct {
	hostRoot string
}

// NewRealExecutor creates an Executor that operates against the given host
// root directory.
func NewRealExecutor(hostRoot string) *RealExecutor {
	return &RealExecutor{hostRoot: hostRoot}
}

func (r *RealExecutor) HostRoot() string { return r.hostRoot }

func (r *RealExecutor) hostPath(path string) string {
	return filepath.Join(r.hostRoot, path)
}

// Run executes a command via os/exec with a 5-minute timeout.
func (r *RealExecutor) Run(name string, args ...string) ExecResult {
	cmd := exec.Command(name, args...)
	cmd.Dir = r.hostRoot

	var stdout, stderr strings.Builder
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	done := make(chan error, 1)
	if err := cmd.Start(); err != nil {
		return ExecResult{
			Stdout:   "",
			Stderr:   fmt.Sprintf("failed to start: %v", err),
			ExitCode: 127,
		}
	}
	go func() { done <- cmd.Wait() }()

	select {
	case err := <-done:
		exitCode := 0
		if err != nil {
			if exitErr, ok := err.(*exec.ExitError); ok {
				exitCode = exitErr.ExitCode()
			} else {
				exitCode = 1
			}
		}
		return ExecResult{
			Stdout:   stdout.String(),
			Stderr:   stderr.String(),
			ExitCode: exitCode,
		}
	case <-time.After(5 * time.Minute):
		_ = cmd.Process.Kill()
		return ExecResult{
			Stdout:   stdout.String(),
			Stderr:   "command timed out after 5m",
			ExitCode: -1,
		}
	}
}

// ReadFile reads a file from the host, prepending hostRoot.
func (r *RealExecutor) ReadFile(path string) (string, error) {
	data, err := os.ReadFile(r.hostPath(path))
	if err != nil {
		return "", err
	}
	return string(data), nil
}

// FileExists checks if a path exists on the host.
func (r *RealExecutor) FileExists(path string) bool {
	_, err := os.Stat(r.hostPath(path))
	return err == nil
}

// ReadDir lists entries in a host directory.
func (r *RealExecutor) ReadDir(path string) ([]os.DirEntry, error) {
	return os.ReadDir(r.hostPath(path))
}

// ---------------------------------------------------------------------------
// FakeExecutor — test double
// ---------------------------------------------------------------------------

// FakeExecutor returns canned results for commands and virtual filesystem
// entries. Unknown commands return exit code 127.
type FakeExecutor struct {
	commands map[string]ExecResult
	files    map[string]string
	dirs     map[string][]string
}

// NewFakeExecutor creates a FakeExecutor with canned command results.
// Keys are "name arg1 arg2 ..." strings (space-joined).
func NewFakeExecutor(commands map[string]ExecResult) *FakeExecutor {
	if commands == nil {
		commands = make(map[string]ExecResult)
	}
	return &FakeExecutor{
		commands: commands,
		files:    make(map[string]string),
		dirs:     make(map[string][]string),
	}
}

// WithFiles adds virtual files to the fake filesystem. Returns the
// receiver for chaining.
func (f *FakeExecutor) WithFiles(files map[string]string) *FakeExecutor {
	for k, v := range files {
		f.files[k] = v
	}
	return f
}

// WithDirs adds virtual directory listings. Returns the receiver for
// chaining.
func (f *FakeExecutor) WithDirs(dirs map[string][]string) *FakeExecutor {
	for k, v := range dirs {
		f.dirs[k] = v
	}
	return f
}

// Commands returns the underlying command map for test inspection.
func (f *FakeExecutor) Commands() map[string]ExecResult { return f.commands }

// AddCommand adds or replaces a canned command result.
func (f *FakeExecutor) AddCommand(key string, result ExecResult) {
	f.commands[key] = result
}

func (f *FakeExecutor) HostRoot() string { return "/" }

// Run looks up the command key (space-joined name + args) and returns the
// canned result, or exit code 127 for unknown commands.
func (f *FakeExecutor) Run(name string, args ...string) ExecResult {
	parts := append([]string{name}, args...)
	key := strings.Join(parts, " ")
	if result, ok := f.commands[key]; ok {
		return result
	}
	return ExecResult{
		Stdout:   "",
		Stderr:   fmt.Sprintf("unknown command: %s", key),
		ExitCode: 127,
	}
}

// ReadFile returns the content of a virtual file.
func (f *FakeExecutor) ReadFile(path string) (string, error) {
	if content, ok := f.files[path]; ok {
		return content, nil
	}
	return "", fmt.Errorf("fake: file not found: %s", path)
}

// FileExists checks if a virtual file or directory exists.
func (f *FakeExecutor) FileExists(path string) bool {
	if _, ok := f.files[path]; ok {
		return true
	}
	if _, ok := f.dirs[path]; ok {
		return true
	}
	return false
}

// ReadDir returns virtual directory entries. An entry whose full path
// (parent + "/" + name) exists as a key in the dirs map is reported as a
// directory; everything else is reported as a file.
func (f *FakeExecutor) ReadDir(path string) ([]os.DirEntry, error) {
	names, ok := f.dirs[path]
	if !ok {
		return nil, fmt.Errorf("fake: directory not found: %s", path)
	}
	entries := make([]os.DirEntry, len(names))
	for i, name := range names {
		childPath := filepath.Join(path, name)
		_, isDir := f.dirs[childPath]
		entries[i] = fakeDirEntry{name: name, isDir: isDir}
	}
	return entries, nil
}

// fakeDirEntry implements os.DirEntry for test doubles.
type fakeDirEntry struct {
	name  string
	isDir bool
}

func (d fakeDirEntry) Name() string               { return d.name }
func (d fakeDirEntry) IsDir() bool                 { return d.isDir }
func (d fakeDirEntry) Type() fs.FileMode {
	if d.isDir {
		return fs.ModeDir
	}
	return 0
}
func (d fakeDirEntry) Info() (fs.FileInfo, error) { return nil, nil }
