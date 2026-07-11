package acp

import (
	"testing"

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
