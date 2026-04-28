package renderer

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

func TestRunAllCreatesOutputDir(t *testing.T) {
	tmpDir := t.TempDir()
	outDir := filepath.Join(tmpDir, "output")

	snap := schema.NewSnapshot()
	err := RunAll(snap, outDir, RunAllOptions{})
	if err != nil {
		t.Fatalf("RunAll: %v", err)
	}

	info, err := os.Stat(outDir)
	if err != nil {
		t.Fatalf("output dir not created: %v", err)
	}
	if !info.IsDir() {
		t.Error("output path is not a directory")
	}
}

func TestRunAllMinimalSnapshot(t *testing.T) {
	outDir := t.TempDir()

	snap := schema.NewSnapshot()
	err := RunAll(snap, outDir, RunAllOptions{})
	if err != nil {
		t.Fatalf("RunAll with minimal snapshot: %v", err)
	}
}

func TestRunAllWithRefineMode(t *testing.T) {
	outDir := t.TempDir()

	snap := schema.NewSnapshot()
	err := RunAll(snap, outDir, RunAllOptions{RefineMode: true})
	if err != nil {
		t.Fatalf("RunAll refine mode: %v", err)
	}
}

func TestWriteFileHelper(t *testing.T) {
	tmpDir := t.TempDir()
	subDir := filepath.Join(tmpDir, "sub", "dir")

	err := writeFile(subDir, "test.txt", "hello world")
	if err != nil {
		t.Fatalf("writeFile: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(subDir, "test.txt"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	if string(data) != "hello world" {
		t.Errorf("got %q, want %q", string(data), "hello world")
	}
}
