package acp

import (
	"encoding/json"
	"log"
	"os"
	"strings"

	"github.com/adama/local-agent/internal/fsutil"
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
	// storePath is set from the daemon's data dir.
	data, err := os.ReadFile(c.storePath)
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
		r.Status = statusIdle
		r.transport = nil
		// Preserve ACPSessionID so startTransportLocked can attempt session/load
		// to resume the agent's prior session on the next prompt. The live
		// transport is nil; it is (re)started lazily.
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
	// Atomic write so a crash mid-persist cannot truncate conversations.json.
	if err := fsutil.WriteFileAtomic(c.storePath, data, conversationStoreFilePerm); err != nil {
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
