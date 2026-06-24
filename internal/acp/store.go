package acp

import (
	"encoding/json"
	"log"
	"os"
	"strings"
)

// conversationStoreFilePerm is the permission for the persisted conversation
// metadata file (owner read/write only).
const conversationStoreFilePerm = 0600

// maxConversationTitleLen bounds an auto-generated conversation title.
const maxConversationTitleLen = 60

// SetStorePath configures where conversation metadata is persisted. Pass an
// empty string to disable persistence (e.g. in tests).
func (c *Client) SetStorePath(path string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.storePath = path
}

// LoadConversations loads persisted conversation metadata from the store path
// into memory. Loaded sessions have no live transport (status "idle"); the agent
// process is (re)started lazily on the next prompt. Safe to call when no store
// file exists yet.
func (c *Client) LoadConversations() error {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.storePath == "" {
		return nil
	}
	data, err := os.ReadFile(c.storePath) //nolint:gosec // storePath is set from the daemon's data dir.
	if err != nil {
		if os.IsNotExist(err) {
			return nil
		}
		return err
	}

	var records []Session
	if err := json.Unmarshal(data, &records); err != nil {
		return err
	}
	for i := range records {
		r := records[i]
		r.Status = "idle"
		r.transport = nil
		r.acpSessionID = ""
		c.sessions[r.ID] = &r
	}
	return nil
}

// persistLocked writes all conversation metadata to the store path. The caller
// must hold c.mu. Persistence failures are logged but not fatal.
func (c *Client) persistLocked() {
	if c.storePath == "" {
		return
	}
	records := make([]Session, 0, len(c.sessions))
	for _, s := range c.sessions {
		records = append(records, *s)
	}
	data, err := json.MarshalIndent(records, "", "  ")
	if err != nil {
		log.Printf("acp: marshal conversations: %v", err)
		return
	}
	if err := os.WriteFile(c.storePath, data, conversationStoreFilePerm); err != nil {
		log.Printf("acp: persist conversations: %v", err)
	}
}

// titleFromPrompt derives a short conversation title from the first user prompt.
func titleFromPrompt(content string) string {
	title := strings.TrimSpace(content)
	// Use the first line only.
	if idx := strings.IndexAny(title, "\r\n"); idx >= 0 {
		title = strings.TrimSpace(title[:idx])
	}
	if len(title) > maxConversationTitleLen {
		title = strings.TrimSpace(title[:maxConversationTitleLen]) + "…"
	}
	if title == "" {
		return defaultConversationName
	}
	return title
}
