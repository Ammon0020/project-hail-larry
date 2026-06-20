package workspace

import (
	"context"
	"os"
	"path/filepath"
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
		i       int
		found   bool
	}
	for i, node := range tree {
		if node.Name == "src" {
			srcNode = &struct {
				i       int
				found   bool
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
