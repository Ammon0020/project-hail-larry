package workspace

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
)

// createTestDir creates a temporary directory structure for testing.
func createTestDir(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()

	// Create a simple project structure.
	os.Mkdir(filepath.Join(dir, "src"), 0755)
	os.Mkdir(filepath.Join(dir, "src", "routes"), 0755)
	os.WriteFile(filepath.Join(dir, "src", "server.js"), []byte("console.log('hello');"), 0644)
	os.WriteFile(filepath.Join(dir, "src", "routes", "index.js"), []byte("// routes"), 0644)
	os.WriteFile(filepath.Join(dir, "package.json"), []byte(`{"name":"test"}`), 0644)
	os.WriteFile(filepath.Join(dir, "README.md"), []byte("# Test Project"), 0644)

	// Create a hidden file that should be excluded.
	os.WriteFile(filepath.Join(dir, ".hidden"), []byte("hidden"), 0644)

	return dir
}

// TestRegister verifies that a directory can be registered as a workspace.
func TestRegister(t *testing.T) {
	m := NewManager()
	ctx := context.Background()
	dir := createTestDir(t)

	ws, err := m.Register(ctx, dir)
	if err != nil {
		t.Fatalf("register: %v", err)
	}

	if ws.ID == "" {
		t.Error("expected non-empty workspace ID")
	}
	if ws.Name != filepath.Base(dir) {
		t.Errorf("expected name %s, got %s", filepath.Base(dir), ws.Name)
	}
}

// TestRegisterNonexistent verifies that registering a nonexistent directory fails.
func TestRegisterNonexistent(t *testing.T) {
	m := NewManager()
	ctx := context.Background()

	_, err := m.Register(ctx, "/nonexistent/path/that/does/not/exist")
	if err == nil {
		t.Error("expected error for nonexistent directory")
	}
}

// TestRegisterFile verifies that registering a file (not directory) fails.
func TestRegisterFile(t *testing.T) {
	m := NewManager()
	ctx := context.Background()

	filePath := filepath.Join(t.TempDir(), "file.txt")
	os.WriteFile(filePath, []byte("test"), 0644)

	_, err := m.Register(ctx, filePath)
	if err == nil {
		t.Error("expected error for file instead of directory")
	}
}

// TestList verifies that registered workspaces are listed.
func TestList(t *testing.T) {
	m := NewManager()
	ctx := context.Background()

	dir1 := createTestDir(t)
	dir2 := t.TempDir()

	m.Register(ctx, dir1)
	m.Register(ctx, dir2)

	workspaces, err := m.List(ctx)
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	if len(workspaces) != 2 {
		t.Fatalf("expected 2 workspaces, got %d", len(workspaces))
	}
}

// TestFileTree verifies the file tree structure.
func TestFileTree(t *testing.T) {
	m := NewManager()
	ctx := context.Background()
	dir := createTestDir(t)

	ws, err := m.Register(ctx, dir)
	if err != nil {
		t.Fatalf("register: %v", err)
	}

	tree, err := m.FileTree(ctx, ws.ID)
	if err != nil {
		t.Fatalf("file tree: %v", err)
	}

	if len(tree) == 0 {
		t.Fatal("expected non-empty file tree")
	}

	// Directories should come first.
	if tree[0].Type != "folder" {
		t.Errorf("expected first node to be a folder, got %s", tree[0].Type)
	}

	// Find the src folder and verify it has children.
	var srcNode *struct {
		i     int
		found bool
	}
	for i, node := range tree {
		if node.Name == "src" {
			srcNode = &struct {
				i     int
				found bool
			}{i: i, found: true}
			if node.Type != "folder" {
				t.Error("expected src to be a folder")
			}
			if len(node.Children) == 0 {
				t.Error("expected src to have children")
			}
			break
		}
	}
	if srcNode == nil || !srcNode.found {
		t.Error("expected to find src folder in tree")
	}

	// Verify hidden files are excluded.
	for _, node := range tree {
		if node.Name == ".hidden" {
			t.Error("expected .hidden to be excluded from file tree")
		}
	}
}

// TestFileTreeNotFound verifies that querying a nonexistent workspace fails.
func TestFileTreeNotFound(t *testing.T) {
	m := NewManager()
	ctx := context.Background()

	_, err := m.FileTree(ctx, "nonexistent")
	if err == nil {
		t.Error("expected error for nonexistent workspace")
	}
}

// TestReadFile verifies file reading and revision.
func TestReadFile(t *testing.T) {
	m := NewManager()
	ctx := context.Background()
	dir := createTestDir(t)

	ws, _ := m.Register(ctx, dir)

	content, revision, err := m.ReadFile(ctx, ws.ID, "package.json")
	if err != nil {
		t.Fatalf("read file: %v", err)
	}

	if content != `{"name":"test"}` {
		t.Errorf("expected content '{\"name\":\"test\"}', got %s", content)
	}
	if revision == 0 {
		t.Error("expected non-zero revision")
	}
}

// TestReadFileTraversal verifies that path traversal is blocked.
func TestReadFileTraversal(t *testing.T) {
	m := NewManager()
	ctx := context.Background()
	dir := createTestDir(t)

	ws, _ := m.Register(ctx, dir)

	_, _, err := m.ReadFile(ctx, ws.ID, "../../../etc/passwd")
	if err == nil {
		t.Error("expected error for path traversal")
	}
}

// TestReadFileSymlinkEscape verifies that reading through a symlink that
// points outside the workspace is blocked (Finding 2.1). An agent could create
// such a symlink via an approved shell command (`ln -s /etc/passwd ./passwd`)
// and then attempt to read it through the API to escape the workspace boundary.
func TestReadFileSymlinkEscape(t *testing.T) {
	m := NewManager()
	ctx := context.Background()
	dir := createTestDir(t)

	ws, _ := m.Register(ctx, dir)

	// Create a target file outside the workspace and a symlink inside pointing
	// at it.
	outside := t.TempDir()
	target := filepath.Join(outside, "secret.txt")
	if err := os.WriteFile(target, []byte("secret"), 0644); err != nil {
		t.Fatalf("write target: %v", err)
	}
	link := filepath.Join(dir, "link.txt")
	if err := os.Symlink(target, link); err != nil {
		t.Fatalf("symlink: %v", err)
	}

	if _, _, err := m.ReadFile(ctx, ws.ID, "link.txt"); err == nil {
		t.Error("expected error when reading through a symlink escaping the workspace")
	}
}

// TestReadFileSymlinkInsideWorkspace verifies that even a symlink whose target
// is inside the workspace is rejected — the policy is "no symlinks at all" for
// workspace file access, since allowing them would let an agent pivot through
// links they created.
func TestReadFileSymlinkInsideWorkspace(t *testing.T) {
	m := NewManager()
	ctx := context.Background()
	dir := createTestDir(t)

	ws, _ := m.Register(ctx, dir)

	// Symlink inside the workspace pointing at another file inside the workspace.
	link := filepath.Join(dir, "link.json")
	if err := os.Symlink(filepath.Join(dir, "package.json"), link); err != nil {
		t.Fatalf("symlink: %v", err)
	}

	if _, _, err := m.ReadFile(ctx, ws.ID, "link.json"); err == nil {
		t.Error("expected error when reading through an in-workspace symlink")
	}
}

// TestWriteFileSymlinkEscape verifies that writing through a symlink that
// points outside the workspace is blocked (Finding 2.1, write path).
func TestWriteFileSymlinkEscape(t *testing.T) {
	m := NewManager()
	ctx := context.Background()
	dir := createTestDir(t)

	ws, _ := m.Register(ctx, dir)

	outside := t.TempDir()
	target := filepath.Join(outside, "pwned.txt")
	if err := os.WriteFile(target, []byte("orig"), 0644); err != nil {
		t.Fatalf("write target: %v", err)
	}
	link := filepath.Join(dir, "pwned.txt")
	if err := os.Symlink(target, link); err != nil {
		t.Fatalf("symlink: %v", err)
	}

	if _, err := m.WriteFile(ctx, ws.ID, "pwned.txt", "escaped", 0); err == nil {
		t.Error("expected error when writing through a symlink escaping the workspace")
	}

	// Confirm the out-of-workspace target was NOT modified.
	got, err := os.ReadFile(target)
	if err != nil {
		t.Fatalf("read target: %v", err)
	}
	if string(got) != "orig" {
		t.Errorf("out-of-workspace target was modified through symlink: got %q", got)
	}
}

// TestWriteFileSymlinkParentEscape verifies that a symlinked parent directory
// cannot be used to escape the workspace on a write to a not-yet-existing path.
func TestWriteFileSymlinkParentEscape(t *testing.T) {
	m := NewManager()
	ctx := context.Background()
	dir := createTestDir(t)

	ws, _ := m.Register(ctx, dir)

	outside := t.TempDir()
	// `ln -s <outside> ./etc` then write `./etc/passwd`.
	linkDir := filepath.Join(dir, "etc")
	if err := os.Symlink(outside, linkDir); err != nil {
		t.Fatalf("symlink: %v", err)
	}

	if _, err := m.WriteFile(ctx, ws.ID, "etc/passwd", "escaped", 0); err == nil {
		t.Error("expected error when writing through a symlinked parent escaping the workspace")
	}

	// Confirm nothing was written outside the workspace.
	if _, err := os.Stat(filepath.Join(outside, "passwd")); !os.IsNotExist(err) {
		t.Errorf("out-of-workspace file was created through symlinked parent: %v", err)
	}
}

// TestReadFileSizeLimit verifies that reading a file larger than maxReadFileSize
// is rejected without loading the content (Finding 4.2).
func TestReadFileSizeLimit(t *testing.T) {
	m := NewManager()
	ctx := context.Background()
	dir := t.TempDir()

	ws, err := m.Register(ctx, dir)
	if err != nil {
		t.Fatalf("register: %v", err)
	}

	// Create a file just over the cap. We write a sparse file to avoid actually
	// allocating maxReadFileSize+1 bytes on disk during the test.
	bigPath := filepath.Join(dir, "big.bin")
	f, err := os.Create(bigPath)
	if err != nil {
		t.Fatalf("create: %v", err)
	}
	// Seek to (maxReadFileSize) and write one byte beyond the cap.
	if _, err := f.Seek(maxReadFileSize, 0); err != nil {
		t.Fatalf("seek: %v", err)
	}
	if _, err := f.Write([]byte("x")); err != nil {
		t.Fatalf("write: %v", err)
	}
	if err := f.Close(); err != nil {
		t.Fatalf("close: %v", err)
	}

	if _, _, err := m.ReadFile(ctx, ws.ID, "big.bin"); err == nil {
		t.Error("expected error for file exceeding maxReadFileSize")
	} else if !strings.Contains(err.Error(), "too large") {
		t.Errorf("expected 'too large' error, got: %v", err)
	}
}

// TestReadFileUnderSizeLimit verifies that a file just under the cap reads fine.
func TestReadFileUnderSizeLimit(t *testing.T) {
	m := NewManager()
	ctx := context.Background()
	dir := t.TempDir()

	ws, err := m.Register(ctx, dir)
	if err != nil {
		t.Fatalf("register: %v", err)
	}

	// Create a small file and confirm it reads normally.
	smallPath := filepath.Join(dir, "small.txt")
	if writeErr := os.WriteFile(smallPath, []byte("hello"), 0644); writeErr != nil {
		t.Fatalf("write: %v", writeErr)
	}

	content, _, err := m.ReadFile(ctx, ws.ID, "small.txt")
	if err != nil {
		t.Fatalf("read file: %v", err)
	}
	if content != "hello" {
		t.Errorf("expected 'hello', got %q", content)
	}
}

// TestReadFileNotFound verifies that reading a nonexistent file fails.
func TestReadFileNotFound(t *testing.T) {
	m := NewManager()
	ctx := context.Background()
	dir := createTestDir(t)

	ws, _ := m.Register(ctx, dir)

	_, _, err := m.ReadFile(ctx, ws.ID, "nonexistent.txt")
	if err == nil {
		t.Error("expected error for nonexistent file")
	}
}

// TestConcurrentMapAccess exercises the workspaces map from many goroutines
// simultaneously (readers + writers) to verify the mutex prevents the
// "concurrent map read and map write" runtime panic. Run with -race to catch
// data races that don't necessarily panic.
func TestConcurrentMapAccess(t *testing.T) {
	m := NewManager()
	ctx := context.Background()
	dir := createTestDir(t)

	ws, err := m.Register(ctx, dir)
	if err != nil {
		t.Fatalf("register: %v", err)
	}

	// Use a separate directory for the writer goroutines so their deterministic
	// IDs do not collide with (and remove) the reader's workspace. Each writer
	// goroutine gets its own subdirectory so register/remove cycles are
	// independent and exercise genuine concurrent map writes with distinct keys.
	writerBase := t.TempDir()

	var wg sync.WaitGroup
	const goroutines = 50

	// Writers: repeatedly register and remove a workspace.
	for i := 0; i < goroutines; i++ {
		wg.Add(1)
		go func(n int) {
			defer wg.Done()
			// Unique directory per goroutine → unique deterministic ID.
			writerDir := filepath.Join(writerBase, fmt.Sprintf("w-%d", n))
			if err := os.Mkdir(writerDir, 0755); err != nil && !os.IsExist(err) {
				t.Errorf("mkdir: %v", err)
				return
			}
			for j := 0; j < 100; j++ {
				info, rerr := m.Register(ctx, writerDir)
				if rerr != nil {
					t.Errorf("register: %v", rerr)
					return
				}
				if rerr := m.Remove(ctx, info.ID); rerr != nil {
					t.Errorf("remove: %v", rerr)
					return
				}
			}
		}(i)
	}

	// Readers: repeatedly list, look up the file tree, and read a file.
	for i := 0; i < goroutines; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < 100; j++ {
				if _, err := m.List(ctx); err != nil {
					t.Errorf("list: %v", err)
					return
				}
				if _, err := m.FileTree(ctx, ws.ID); err != nil {
					t.Errorf("file tree: %v", err)
					return
				}
				if _, _, err := m.ReadFile(ctx, ws.ID, "package.json"); err != nil {
					t.Errorf("read file: %v", err)
					return
				}
			}
		}()
	}

	wg.Wait()
}
