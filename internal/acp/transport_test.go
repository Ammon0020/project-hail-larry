package acp

import (
	"encoding/base64"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/adama/local-agent/internal/interfaces"
	"github.com/coder/acp-go-sdk"
)

// TestBuildPromptBlocks_EmbeddedContextResourceBlock verifies that when the
// agent advertises the embeddedContext prompt capability, each ContextResource
// is rendered as an inline ACP ResourceBlock (uri, mimeType, text) carrying the
// resource's URI/mimeType/text — the ACP-spec-compliant way to inject context.
func TestBuildPromptBlocks_EmbeddedContextResourceBlock(t *testing.T) {
	resources := []ContextResource{
		{URI: "context://workspace", MimeType: "text/markdown", Name: "Workspace Context", Text: "## Workspace Context\n- root: /tmp"},
		{URI: "file:///tmp/AGENTS.md", MimeType: "text/markdown", Name: "AGENTS.md", Text: "# Agents\nRules."},
	}
	blocks := buildPromptBlocks(acp.PromptCapabilities{EmbeddedContext: true}, "hello", resources, nil)

	// Expect: 1 text block + 2 resource blocks.
	if got, want := len(blocks), 3; got != want {
		t.Fatalf("expected %d blocks, got %d", want, got)
	}
	if blocks[0].Text == nil || blocks[0].Text.Text != "hello" {
		t.Errorf("expected first block to be text 'hello', got %+v", blocks[0].Text)
	}
	for i, b := range blocks[1:] {
		if b.Resource == nil {
			t.Errorf("block %d: expected ResourceBlock, got nil resource", i+1)
			continue
		}
		trc := b.Resource.Resource.TextResourceContents
		if trc == nil {
			t.Errorf("block %d: expected TextResourceContents, got nil", i+1)
			continue
		}
		if trc.Uri != resources[i].URI {
			t.Errorf("block %d: expected URI %q, got %q", i+1, resources[i].URI, trc.Uri)
		}
		if trc.Text != resources[i].Text {
			t.Errorf("block %d: expected text %q, got %q", i+1, resources[i].Text, trc.Text)
		}
		if trc.MimeType == nil || *trc.MimeType != resources[i].MimeType {
			mt := "<nil>"
			if trc.MimeType != nil {
				mt = *trc.MimeType
			}
			t.Errorf("block %d: expected mimeType %q, got %q", i+1, resources[i].MimeType, mt)
		}
		// ResourceBlocks must not also carry a resource link.
		if b.ResourceLink != nil {
			t.Errorf("block %d: ResourceBlock must not also set ResourceLink", i+1)
		}
	}
}

// TestBuildPromptBlocks_FallbackResourceLinkAndText verifies that when the
// agent does NOT advertise embeddedContext, each ContextResource is rendered as
// a ResourceLinkBlock (always supported per spec) followed by a TextBlock
// folding the resource text in, so non-embeddedContext agents still see the
// content.
func TestBuildPromptBlocks_FallbackResourceLinkAndText(t *testing.T) {
	resources := []ContextResource{
		{URI: "context://workspace", MimeType: "text/markdown", Name: "Workspace Context", Text: "## Workspace Context\n- root: /tmp"},
	}
	blocks := buildPromptBlocks(acp.PromptCapabilities{EmbeddedContext: false}, "hello", resources, nil)

	// Expect: 1 text block + 1 resource link + 1 fallback text block.
	if got, want := len(blocks), 3; got != want {
		t.Fatalf("expected %d blocks, got %d", want, got)
	}
	if blocks[0].Text == nil || blocks[0].Text.Text != "hello" {
		t.Errorf("expected first block to be text 'hello', got %+v", blocks[0].Text)
	}
	link := blocks[1]
	if link.ResourceLink == nil {
		t.Fatal("expected second block to be a ResourceLinkBlock")
	}
	if link.ResourceLink.Name != "Workspace Context" {
		t.Errorf("expected resource link name 'Workspace Context', got %q", link.ResourceLink.Name)
	}
	if link.ResourceLink.Uri != "context://workspace" {
		t.Errorf("expected resource link URI 'context://workspace', got %q", link.ResourceLink.Uri)
	}
	if link.Resource != nil {
		t.Error("fallback resource link block must not set Resource")
	}
	fallbackText := blocks[2]
	if fallbackText.Text == nil || fallbackText.Text.Text != resources[0].Text {
		t.Errorf("expected third block to fold resource text %q, got %+v", resources[0].Text, fallbackText.Text)
	}
}

// TestBuildPromptBlocks_NoResourcesJustText verifies that with no resources and
// no attachments, buildPromptBlocks returns a single text block.
func TestBuildPromptBlocks_NoResourcesJustText(t *testing.T) {
	blocks := buildPromptBlocks(acp.PromptCapabilities{}, "only text", nil, nil)
	if got, want := len(blocks), 1; got != want {
		t.Fatalf("expected %d block, got %d", want, got)
	}
	if blocks[0].Text == nil || blocks[0].Text.Text != "only text" {
		t.Errorf("expected single text block 'only text', got %+v", blocks[0].Text)
	}
}

// TestBuildPromptBlocks_AttachmentImageCapableValidFile verifies that when the
// agent advertises the Image prompt capability and the attachment file is
// readable, buildPromptBlocks emits a single ContentBlockImage carrying the
// base64-encoded file data, the attachment's MIME type, and its URI. This is
// the inline-image path used for image-capable agents.
func TestBuildPromptBlocks_AttachmentImageCapableValidFile(t *testing.T) {
	// Write a small PNG-ish blob to a temp file so os.ReadFile succeeds.
	dir := t.TempDir()
	imgPath := filepath.Join(dir, "snap.png")
	imgBytes := []byte("\x89PNG\r\n\x1a\nfake-png-body")
	if err := os.WriteFile(imgPath, imgBytes, 0o600); err != nil {
		t.Fatalf("write temp image: %v", err)
	}

	att := interfaces.Attachment{
		ID:       "att-1",
		Name:     "snap.png",
		MimeType: "image/png",
		URI:      "file://" + imgPath,
		Path:     imgPath,
	}
	blocks := buildPromptBlocks(acp.PromptCapabilities{Image: true}, "look at this", nil, []interfaces.Attachment{att})

	// Expect: 1 text block + 1 image block.
	if got, want := len(blocks), 2; got != want {
		t.Fatalf("expected %d blocks, got %d", want, got)
	}
	if blocks[0].Text == nil || blocks[0].Text.Text != "look at this" {
		t.Errorf("expected first block to be text 'look at this', got %+v", blocks[0].Text)
	}
	img := blocks[1]
	if img.Image == nil {
		t.Fatal("expected second block to be an Image block")
	}
	if got, want := img.Image.Data, base64.StdEncoding.EncodeToString(imgBytes); got != want {
		t.Errorf("expected base64 data %q, got %q", want, got)
	}
	if img.Image.MimeType != "image/png" {
		t.Errorf("expected mimeType 'image/png', got %q", img.Image.MimeType)
	}
	if img.Image.Type != "image" {
		t.Errorf("expected type 'image', got %q", img.Image.Type)
	}
	if img.Image.Uri == nil || *img.Image.Uri != att.URI {
		got := "<nil>"
		if img.Image.Uri != nil {
			got = *img.Image.Uri
		}
		t.Errorf("expected uri %q, got %q", att.URI, got)
	}
	// The inline image path must not also emit a resource link or fallback text.
	if img.ResourceLink != nil {
		t.Error("image block must not also set ResourceLink")
	}
}

// TestBuildPromptBlocks_AttachmentImageCapableMissingFile verifies that when the
// agent advertises the Image capability but os.ReadFile fails (e.g. the file was
// removed from the uploads store), buildPromptBlocks falls back to a
// ResourceLinkBlock + TextBlock pair so the agent can still locate the
// attachment by URI. The read error is logged at slog.Warn by the helper; this
// test only asserts the block shape.
func TestBuildPromptBlocks_AttachmentImageCapableMissingFile(t *testing.T) {
	att := interfaces.Attachment{
		ID:       "att-2",
		Name:     "gone.png",
		MimeType: "image/png",
		URI:      "file:///does/not/exist/gone.png",
		Path:     filepath.Join(t.TempDir(), "definitely-missing.png"),
	}
	blocks := buildPromptBlocks(acp.PromptCapabilities{Image: true}, "look at this", nil, []interfaces.Attachment{att})

	// Expect: 1 text block + 1 resource link + 1 fallback text block.
	if got, want := len(blocks), 3; got != want {
		t.Fatalf("expected %d blocks, got %d", want, got)
	}
	if blocks[0].Text == nil || blocks[0].Text.Text != "look at this" {
		t.Errorf("expected first block to be text 'look at this', got %+v", blocks[0].Text)
	}
	link := blocks[1]
	if link.ResourceLink == nil {
		t.Fatal("expected second block to be a ResourceLinkBlock")
	}
	if link.ResourceLink.Name != att.Name {
		t.Errorf("expected resource link name %q, got %q", att.Name, link.ResourceLink.Name)
	}
	if link.ResourceLink.Uri != att.URI {
		t.Errorf("expected resource link uri %q, got %q", att.URI, link.ResourceLink.Uri)
	}
	if link.Image != nil {
		t.Error("fallback resource link must not set Image")
	}
	fallbackText := blocks[2]
	if fallbackText.Text == nil {
		t.Fatal("expected third block to be a TextBlock")
	}
	if !strings.Contains(fallbackText.Text.Text, att.Name) || !strings.Contains(fallbackText.Text.Text, att.URI) {
		t.Errorf("expected fallback text to mention name %q and uri %q, got %q", att.Name, att.URI, fallbackText.Text.Text)
	}
}

// TestBuildPromptBlocks_AttachmentNonImageCapable verifies that when the agent
// does NOT advertise the Image capability, attachments are always rendered as a
// ResourceLinkBlock + TextBlock pair regardless of file readability — the
// inline-image path is skipped entirely.
func TestBuildPromptBlocks_AttachmentNonImageCapable(t *testing.T) {
	att := interfaces.Attachment{
		ID:       "att-3",
		Name:     "photo.jpg",
		MimeType: "image/jpeg",
		URI:      "file:///uploads/photo.jpg",
		Path:     filepath.Join(t.TempDir(), "photo.jpg"),
	}
	blocks := buildPromptBlocks(acp.PromptCapabilities{Image: false}, "see photo", nil, []interfaces.Attachment{att})

	// Expect: 1 text block + 1 resource link + 1 fallback text block.
	if got, want := len(blocks), 3; got != want {
		t.Fatalf("expected %d blocks, got %d", want, got)
	}
	if blocks[0].Text == nil || blocks[0].Text.Text != "see photo" {
		t.Errorf("expected first block to be text 'see photo', got %+v", blocks[0].Text)
	}
	link := blocks[1]
	if link.ResourceLink == nil {
		t.Fatal("expected second block to be a ResourceLinkBlock")
	}
	if link.ResourceLink.Name != att.Name {
		t.Errorf("expected resource link name %q, got %q", att.Name, link.ResourceLink.Name)
	}
	if link.ResourceLink.Uri != att.URI {
		t.Errorf("expected resource link uri %q, got %q", att.URI, link.ResourceLink.Uri)
	}
	if link.Image != nil {
		t.Error("non-image-capable path must not emit an Image block")
	}
	fallbackText := blocks[2]
	if fallbackText.Text == nil {
		t.Fatal("expected third block to be a TextBlock")
	}
	if !strings.Contains(fallbackText.Text.Text, att.Name) || !strings.Contains(fallbackText.Text.Text, att.URI) {
		t.Errorf("expected fallback text to mention name %q and uri %q, got %q", att.Name, att.URI, fallbackText.Text.Text)
	}
}
