package fsutil_test

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/adama/local-agent/internal/fsutil"
)

func TestWriteFileAtomicCreatesAndOverwrites(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	path := filepath.Join(dir, "nested", "state.json")

	if err := fsutil.WriteFileAtomic(path, []byte(`{"v":1}`), 0o600); err != nil {
		t.Fatalf("first write: %v", err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	if string(data) != `{"v":1}` {
		t.Fatalf("content = %q", data)
	}

	if err := fsutil.WriteFileAtomic(path, []byte(`{"v":2}`), 0o600); err != nil {
		t.Fatalf("second write: %v", err)
	}
	data, err = os.ReadFile(path)
	if err != nil {
		t.Fatalf("read2: %v", err)
	}
	if string(data) != `{"v":2}` {
		t.Fatalf("content2 = %q", data)
	}

	// No leftover temps after successful writes.
	entries, err := os.ReadDir(filepath.Dir(path))
	if err != nil {
		t.Fatalf("readdir: %v", err)
	}
	for _, e := range entries {
		name := e.Name()
		if len(name) > 4 && name[len(name)-4:] == ".tmp" {
			t.Fatalf("leftover temp: %s", name)
		}
	}
}

func TestWriteFileAtomicMode(t *testing.T) {
	t.Parallel()
	if runtime.GOOS == "windows" {
		t.Skip("file modes not meaningful on Windows")
	}
	dir := t.TempDir()
	path := filepath.Join(dir, "secret.json")
	if err := fsutil.WriteFileAtomic(path, []byte(`{}`), 0o600); err != nil {
		t.Fatalf("write: %v", err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatalf("stat: %v", err)
	}
	if mode := info.Mode().Perm(); mode != 0o600 {
		t.Fatalf("mode = %o, want 0600", mode)
	}
}
