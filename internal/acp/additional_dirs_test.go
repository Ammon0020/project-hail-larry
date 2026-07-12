package acp

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/adama/local-agent/internal/interfaces"
	"github.com/adama/local-agent/internal/search"
	acpsdk "github.com/coder/acp-go-sdk"
)

// multiRootWorkspaceManager is a stub interfaces.WorkspaceManager that returns
// a fixed list of registered workspaces and serves ReadFile by routing the
// request to whichever workspace root contains the (absolute) path the test
// supplied. It is used by the multi-root tests for resolveWorkspaceFile and
// resolveCwdMulti, which need a List() that reports several roots and a
// ReadFile that can act on any of them.
type multiRootWorkspaceManager struct {
	workspaces []interfaces.WorkspaceInfo
	// files maps workspaceID -> relPath -> content. ReadFile consults this map
	// and returns the content for the requested (workspaceID, relPath) pair,
	// so resolveWorkspaceFile can be exercised end-to-end without touching disk.
	files map[string]map[string]string
}

func (m *multiRootWorkspaceManager) Register(_ context.Context, path string) (interfaces.WorkspaceInfo, error) {
	return interfaces.WorkspaceInfo{ID: path, Path: path, Name: filepath.Base(path)}, nil
}

func (m *multiRootWorkspaceManager) List(_ context.Context) ([]interfaces.WorkspaceInfo, error) {
	out := make([]interfaces.WorkspaceInfo, len(m.workspaces))
	copy(out, m.workspaces)
	return out, nil
}

func (m *multiRootWorkspaceManager) FileTree(_ context.Context, _ string) ([]interfaces.FileNode, error) {
	return nil, nil
}

func (m *multiRootWorkspaceManager) ReadFile(_ context.Context, workspaceID, relPath string) (string, int64, bool, bool, error) {
	if m.files == nil {
		return "", 0, false, false, nil
	}
	if wsFiles, ok := m.files[workspaceID]; ok {
		if content, ok := wsFiles[relPath]; ok {
			return content, 1, false, false, nil
		}
	}
	return "", 0, false, false, errors.New("file not found")
}

func (m *multiRootWorkspaceManager) Search(_ context.Context, _, _ string, _ search.Options) ([]search.Result, error) {
	return nil, nil
}

func (m *multiRootWorkspaceManager) FilePath(_ context.Context, _, _ string) (string, error) {
	return "", errors.New("not implemented")
}

func (m *multiRootWorkspaceManager) Remove(_ context.Context, _ string) error { return nil }

// absPath returns an absolute path that is stable across platforms for use in
// test fixtures. On Windows it produces a C:\... path; elsewhere /tmp/...
func absPath(parts ...string) string {
	root := "/tmp"
	if runtime.GOOS == "windows" {
		root = `C:\tmp`
	}
	return filepath.Join(append([]string{root}, parts...)...)
}

// TestCollectAdditionalDirs verifies the helper that builds the ACP
// additionalDirectories list from the registered workspaces. The primary
// workspace must be excluded (both by ID and by cleaned path), non-absolute
// paths skipped, duplicates collapsed, and an empty result returned when only
// the primary workspace is registered.
func TestCollectAdditionalDirs(t *testing.T) {
	primary := absPath("primary")
	otherA := absPath("other-a")
	otherB := absPath("other-b")

	cases := []struct {
		name        string
		workspaces  []interfaces.WorkspaceInfo
		primaryID   string
		primaryPath string
		want        []string
	}{
		{
			name: "single workspace returns nil",
			workspaces: []interfaces.WorkspaceInfo{
				{ID: "ws1", Path: primary},
			},
			primaryID:   "ws1",
			primaryPath: primary,
			want:        nil,
		},
		{
			name: "excludes primary by id, includes others",
			workspaces: []interfaces.WorkspaceInfo{
				{ID: "ws1", Path: primary},
				{ID: "ws2", Path: otherA},
				{ID: "ws3", Path: otherB},
			},
			primaryID:   "ws1",
			primaryPath: primary,
			want:        []string{otherA, otherB},
		},
		{
			name: "excludes primary by path even when id differs",
			workspaces: []interfaces.WorkspaceInfo{
				{ID: "ws1", Path: primary},
				{ID: "ws-alias", Path: primary}, // same path, different id
				{ID: "ws2", Path: otherA},
			},
			primaryID:   "ws1",
			primaryPath: primary,
			want:        []string{otherA},
		},
		{
			name: "skips non-absolute paths",
			workspaces: []interfaces.WorkspaceInfo{
				{ID: "ws1", Path: primary},
				{ID: "rel", Path: "relative/path"},
				{ID: "ws2", Path: otherA},
			},
			primaryID:   "ws1",
			primaryPath: primary,
			want:        []string{otherA},
		},
		{
			name: "dedupes identical absolute paths",
			workspaces: []interfaces.WorkspaceInfo{
				{ID: "ws1", Path: primary},
				{ID: "dup-a", Path: otherA},
				{ID: "dup-b", Path: otherA},
			},
			primaryID:   "ws1",
			primaryPath: primary,
			want:        []string{otherA},
		},
		{
			name:        "nil workspace manager returns nil",
			workspaces:  nil,
			primaryID:   "ws1",
			primaryPath: primary,
			want:        nil,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			c := NewClient(nil, nil)
			if tc.workspaces != nil {
				c.workspaceMgr = &multiRootWorkspaceManager{workspaces: tc.workspaces}
			}
			got := c.collectAdditionalDirsLocked(context.Background(), tc.primaryID, tc.primaryPath)
			if len(got) == 0 && len(tc.want) == 0 {
				return
			}
			if len(got) != len(tc.want) {
				t.Fatalf("got %v, want %v", got, tc.want)
			}
			// Compare as sets — collectAdditionalDirs preserves List() order
			// but the test fixtures are unordered by intent.
			wantSet := map[string]bool{}
			for _, p := range tc.want {
				wantSet[p] = true
			}
			for _, p := range got {
				if !wantSet[p] {
					t.Errorf("unexpected dir %q (want set %v)", p, tc.want)
				}
			}
		})
	}
}

// TestResolveACPSessionAdditionalDirs verifies that resolveACPSession forwards
// the additionalDirs argument to both NewSession and LoadSession unchanged.
func TestResolveACPSessionAdditionalDirs(t *testing.T) {
	dirs := []string{absPath("other-a"), absPath("other-b")}

	t.Run("new session receives additional dirs", func(t *testing.T) {
		mt := &mockTransport{newSessionResult: "new-1"}
		c := NewClient(nil, nil)
		initResp := acpsdk.InitializeResponse{}
		session := &Session{}
		id, _, err := c.resolveACPSession(context.Background(), mt, initResp, session, "/ws", dirs)
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if id != "new-1" {
			t.Errorf("id = %q, want %q", id, "new-1")
		}
		if !mt.newSessionCalled {
			t.Fatal("expected NewSession to be called")
		}
		if !equalStringSlices(mt.newSessionDirs, dirs) {
			t.Errorf("NewSession dirs = %v, want %v", mt.newSessionDirs, dirs)
		}
	})

	t.Run("load session receives additional dirs", func(t *testing.T) {
		mt := &mockTransport{
			loadSessionResult: "acp-1",
			newSessionResult:  "new-1",
		}
		c := NewClient(nil, nil)
		initResp := acpsdk.InitializeResponse{
			AgentCapabilities: acpsdk.AgentCapabilities{LoadSession: true},
		}
		session := &Session{ACPSessionID: "acp-1"}
		id, _, err := c.resolveACPSession(context.Background(), mt, initResp, session, "/ws", dirs)
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if id != "acp-1" {
			t.Errorf("id = %q, want %q", id, "acp-1")
		}
		if !mt.loadSessionCalled {
			t.Fatal("expected LoadSession to be called")
		}
		if !equalStringSlices(mt.loadSessionDirs, dirs) {
			t.Errorf("LoadSession dirs = %v, want %v", mt.loadSessionDirs, dirs)
		}
		if mt.newSessionCalled {
			t.Error("NewSession should not be called when load succeeds")
		}
	})

	t.Run("nil dirs pass through to new session", func(t *testing.T) {
		mt := &mockTransport{newSessionResult: "new-1"}
		c := NewClient(nil, nil)
		initResp := acpsdk.InitializeResponse{}
		session := &Session{}
		if _, _, err := c.resolveACPSession(context.Background(), mt, initResp, session, "/ws", nil); err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if len(mt.newSessionDirs) != 0 {
			t.Errorf("NewSession dirs = %v, want nil/empty", mt.newSessionDirs)
		}
	})
}

// TestNewSessionRequestAdditionalDirectoriesOmitEmpty verifies that the ACP
// NewSessionRequest and LoadSessionRequest serialize additionalDirectories as
// omitted (not null) when the slice is nil/empty, and present when non-empty.
// This keeps the wire payload clean for agents that don't advertise the
// capability while still sending the field to agents that do.
func TestNewSessionRequestAdditionalDirectoriesOmitEmpty(t *testing.T) {
	t.Run("nil omits field", func(t *testing.T) {
		req := acpsdk.NewSessionRequest{
			Cwd:        "/tmp",
			McpServers: []acpsdk.McpServer{},
		}
		data, err := json.Marshal(req)
		if err != nil {
			t.Fatalf("marshal: %v", err)
		}
		if strings.Contains(string(data), "additionalDirectories") {
			t.Errorf("expected additionalDirectories to be omitted, got %s", data)
		}
	})
	t.Run("empty omits field", func(t *testing.T) {
		req := acpsdk.NewSessionRequest{
			Cwd:                   "/tmp",
			McpServers:            []acpsdk.McpServer{},
			AdditionalDirectories: []string{},
		}
		data, err := json.Marshal(req)
		if err != nil {
			t.Fatalf("marshal: %v", err)
		}
		if strings.Contains(string(data), "additionalDirectories") {
			t.Errorf("expected empty additionalDirectories to be omitted, got %s", data)
		}
	})
	t.Run("non-empty serializes field", func(t *testing.T) {
		req := acpsdk.NewSessionRequest{
			Cwd:                   "/tmp",
			McpServers:            []acpsdk.McpServer{},
			AdditionalDirectories: []string{"/other/a", "/other/b"},
		}
		data, err := json.Marshal(req)
		if err != nil {
			t.Fatalf("marshal: %v", err)
		}
		if !strings.Contains(string(data), `"additionalDirectories":["/other/a","/other/b"]`) {
			t.Errorf("expected additionalDirectories present, got %s", data)
		}
	})
	t.Run("load request non-empty serializes field", func(t *testing.T) {
		req := acpsdk.LoadSessionRequest{
			SessionId:             "s-1",
			Cwd:                   "/tmp",
			McpServers:            []acpsdk.McpServer{},
			AdditionalDirectories: []string{"/other/a"},
		}
		data, err := json.Marshal(req)
		if err != nil {
			t.Fatalf("marshal: %v", err)
		}
		if !strings.Contains(string(data), `"additionalDirectories":["/other/a"]`) {
			t.Errorf("expected additionalDirectories present, got %s", data)
		}
	})
}

// TestResolveWorkspaceFile verifies the multi-root path resolver used by
// ReadTextFile/WriteTextFile. Relative paths and absolute paths inside the
// primary workspace resolve to the primary; absolute paths inside another
// registered workspace resolve to that workspace; paths outside every
// registered workspace fall back to the primary (where the workspace manager's
// safeJoin will reject them).
func TestResolveWorkspaceFile(t *testing.T) {
	primary := absPath("primary")
	other := absPath("other")

	wm := &multiRootWorkspaceManager{
		workspaces: []interfaces.WorkspaceInfo{
			{ID: "ws-primary", Path: primary},
			{ID: "ws-other", Path: other},
		},
		files: map[string]map[string]string{
			"ws-primary": {"readme.md": "primary readme"},
			"ws-other":   {"notes.md": "other notes"},
		},
	}

	c := &acpClientImpl{
		workspaceID:   "ws-primary",
		workspacePath: primary,
		workspaceMgr:  wm,
	}

	cases := []struct {
		name           string
		path           string
		wantWorkspace  string
		wantRel        string
		wantReadResult string // content expected from ReadFile via the resolved (workspace, rel)
	}{
		{
			name:           "relative path uses primary",
			path:           "readme.md",
			wantWorkspace:  "ws-primary",
			wantRel:        "readme.md",
			wantReadResult: "primary readme",
		},
		{
			name:           "absolute inside primary uses primary",
			path:           filepath.Join(primary, "readme.md"),
			wantWorkspace:  "ws-primary",
			wantRel:        "readme.md",
			wantReadResult: "primary readme",
		},
		{
			name:           "absolute inside other workspace uses other",
			path:           filepath.Join(other, "notes.md"),
			wantWorkspace:  "ws-other",
			wantRel:        "notes.md",
			wantReadResult: "other notes",
		},
		{
			name:           "primary root itself",
			path:           primary,
			wantWorkspace:  "ws-primary",
			wantRel:        ".",
			wantReadResult: "",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			wsID, rel := c.resolveWorkspaceFile(context.Background(), tc.path)
			if wsID != tc.wantWorkspace {
				t.Errorf("workspaceID = %q, want %q", wsID, tc.wantWorkspace)
			}
			if rel != tc.wantRel {
				t.Errorf("relPath = %q, want %q", rel, tc.wantRel)
			}
			// End-to-end: ReadTextFile should return the expected content for
			// paths that have a backing file in the stub.
			if tc.wantReadResult != "" {
				content, _, _, _, rerr := wm.ReadFile(context.Background(), wsID, rel)
				if rerr != nil {
					t.Fatalf("ReadFile: %v", rerr)
				}
				if content != tc.wantReadResult {
					t.Errorf("ReadFile content = %q, want %q", content, tc.wantReadResult)
				}
			}
		})
	}

	// Path outside every registered workspace falls back to primary so the
	// workspace manager returns its standard traversal error (rather than the
	// client silently allowing reads of arbitrary filesystem locations).
	t.Run("absolute outside all workspaces falls back to primary", func(t *testing.T) {
		outside := absPath("escape", "secret.txt")
		wsID, rel := c.resolveWorkspaceFile(context.Background(), outside)
		if wsID != "ws-primary" {
			t.Errorf("workspaceID = %q, want %q (fallback to primary)", wsID, "ws-primary")
		}
		// rel will be ../escape/secret.txt — safeJoin in the workspace manager
		// will reject it. We only assert the fallback target here; the
		// rejection is exercised by the workspace package's traversal tests.
		if !strings.HasPrefix(rel, "..") {
			t.Errorf("expected relPath to escape primary (.. prefix), got %q", rel)
		}
	})
}

// TestResolveCwdMulti verifies that a terminal Cwd pointing inside any
// registered workspace is honored, while a Cwd outside every registered
// workspace falls back to the primary root (preserving the escape-prevention
// guarantee).
func TestResolveCwdMulti(t *testing.T) {
	primary := absPath("primary")
	other := absPath("other")

	wm := &multiRootWorkspaceManager{
		workspaces: []interfaces.WorkspaceInfo{
			{ID: "ws-primary", Path: primary},
			{ID: "ws-other", Path: other},
		},
	}
	c := &acpClientImpl{
		workspaceID:   "ws-primary",
		workspacePath: primary,
		workspaceMgr:  wm,
	}

	cases := []struct {
		name      string
		candidate string
		want      string
	}{
		{name: "empty falls back to primary", candidate: "", want: primary},
		{name: "primary root itself", candidate: primary, want: primary},
		{name: "inside primary", candidate: filepath.Join(primary, "src"), want: filepath.Join(primary, "src")},
		{name: "inside other workspace is honored", candidate: filepath.Join(other, "sub"), want: filepath.Join(other, "sub")},
		{name: "other workspace root itself", candidate: other, want: other},
		{name: "outside all workspaces falls back to primary", candidate: absPath("escape"), want: primary},
		{name: "traversal escapes fall back to primary", candidate: filepath.Join(primary, "..", ".."), want: primary},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := c.resolveCwdMulti(context.Background(), tc.candidate)
			if got != tc.want {
				t.Errorf("resolveCwdMulti(%q) = %q, want %q", tc.candidate, got, tc.want)
			}
		})
	}
}

// TestPathWithinRoot covers the shared containment primitive used by both
// resolveCwd and resolveCwdMulti.
func TestPathWithinRoot(t *testing.T) {
	root := absPath("root")
	cases := []struct {
		name      string
		candidate string
		wantOK    bool
		wantPath  string
	}{
		{name: "empty not inside", candidate: "", wantOK: false},
		{name: "root itself", candidate: root, wantOK: true, wantPath: root},
		{name: "inside root", candidate: filepath.Join(root, "sub", "file.go"), wantOK: true, wantPath: filepath.Join(root, "sub", "file.go")},
		{name: "parent escapes", candidate: filepath.Dir(root), wantOK: false},
		{name: "sibling escapes", candidate: filepath.Join(filepath.Dir(root), "other"), wantOK: false},
		{name: "traversal escapes", candidate: filepath.Join(root, "..", ".."), wantOK: false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			gotPath, gotOK := pathWithinRoot(root, tc.candidate)
			if gotOK != tc.wantOK {
				t.Errorf("pathWithinRoot ok = %v, want %v", gotOK, tc.wantOK)
			}
			if gotOK && gotPath != tc.wantPath {
				t.Errorf("pathWithinRoot path = %q, want %q", gotPath, tc.wantPath)
			}
		})
	}
}

// equalStringSlices reports whether two string slices are element-wise equal.
func equalStringSlices(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
