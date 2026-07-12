// Package acp profile middleware: injects profile-specific system instructions
// into each prompt based on the user's selected mode (Code / Ask / Plan).
//
// The middleware dynamically chooses between structured resource blocks (for
// agents that advertise embeddedContext) and prepended text (for older agents),
// ensuring maximum compatibility. The profile instructions are externalized in
// configs/system-messages.json so they can be customized without editing source.
//
// The profile is set per-session by the REST handler before each prompt via
// SetProfile; the middleware reads it in BeforePrompt/BeforePromptResources.

package acp

import (
	"context"
	"fmt"
	"strings"
	"sync"
)

// profileContextURI is the resource URI for the profile instructions block.
const profileContextURI = "context://profile"

// ProfileMiddleware injects profile-specific system instructions (Code / Ask /
// Plan) into each prompt. It implements both PromptMiddleware (for the text
// fallback path used by agents without embeddedContext) and ResourceMiddleware
// (for structured resource blocks used by agents with embeddedContext).
//
// The middleware is stateful: the REST handler calls SetProfile before each
// prompt to indicate the user's selected mode. When no profile is set for a
// session, "Code" is assumed (the default mode).
type ProfileMiddleware struct {
	Messages *SystemMessages

	mu       sync.RWMutex
	profiles map[string]string // sessionID → profile name ("Code"|"Ask"|"Plan")
}

// NewProfileMiddleware constructs a ProfileMiddleware using the given templates.
// If messages is nil, DefaultSystemMessages is used.
func NewProfileMiddleware(messages *SystemMessages) *ProfileMiddleware {
	if messages == nil {
		messages = DefaultSystemMessages()
	}
	return &ProfileMiddleware{
		Messages: messages,
		profiles: make(map[string]string),
	}
}

// SetProfile records the user's selected profile for a session. Called by the
// REST handler before each prompt. An empty profile is treated as "Code".
func (m *ProfileMiddleware) SetProfile(sessionID, profile string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if profile == "" {
		profile = "Code"
	}
	m.profiles[sessionID] = profile
}

// ClearProfile removes the stored profile for a session (e.g. on close).
func (m *ProfileMiddleware) ClearProfile(sessionID string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	delete(m.profiles, sessionID)
}

// getProfile returns the profile for a session, defaulting to "Code".
func (m *ProfileMiddleware) getProfile(sessionID string) string {
	m.mu.RLock()
	defer m.mu.RUnlock()
	if p, ok := m.profiles[sessionID]; ok {
		return p
	}
	return "Code"
}

// messages returns the configured SystemMessages, falling back to defaults.
func (m *ProfileMiddleware) messages() *SystemMessages {
	if m.Messages == nil {
		return DefaultSystemMessages()
	}
	return m.Messages
}

// instructionsForProfile returns the system instruction text for the given
// profile name, sourced from the SystemMessages templates.
func (m *ProfileMiddleware) instructionsForProfile(profile string) string {
	sm := m.messages()
	switch strings.ToLower(profile) {
	case "ask":
		return sm.ProfileAskInstructions
	case "plan":
		return sm.ProfilePlanInstructions
	default:
		// "Code" or any unknown profile → default mode.
		return sm.ProfileCodeInstructions
	}
}

// buildText builds the full profile injection text (header + instructions).
func (m *ProfileMiddleware) buildText(profile string) string {
	sm := m.messages()
	header := sm.Render(sm.ProfileHeader, map[string]string{"profile": profile})
	instructions := m.instructionsForProfile(profile)
	return fmt.Sprintf("%s\n\n%s", header, instructions)
}

// BeforePrompt implements PromptMiddleware. When the agent does NOT support
// embeddedContext (PromptContext.EmbeddedContext is false), the profile
// instructions are injected as prepended text. When embeddedContext IS
// available, this returns ActionContinue and the instructions are delivered
// via BeforePromptResources instead.
func (m *ProfileMiddleware) BeforePrompt(_ context.Context, pc *PromptContext) (PromptAction, string) {
	// When the agent supports embeddedContext, deliver via resources instead.
	if pc.EmbeddedContext {
		return ActionContinue, ""
	}
	profile := m.getProfile(pc.SessionID)
	return ActionInject, m.buildText(profile)
}

// BeforePromptResources implements ResourceMiddleware. When the agent supports
// embeddedContext (PromptContext.EmbeddedContext is true), the profile
// instructions are emitted as a structured resource block. When embeddedContext
// is NOT available, this returns nil and BeforePrompt handles the text fallback.
func (m *ProfileMiddleware) BeforePromptResources(_ context.Context, pc *PromptContext) []ContextResource {
	// Only emit resources when the agent supports embeddedContext.
	if !pc.EmbeddedContext {
		return nil
	}
	profile := m.getProfile(pc.SessionID)
	return []ContextResource{{
		URI:      profileContextURI,
		MimeType: contextMimeType,
		Name:     "Profile Instructions",
		Text:     m.buildText(profile),
	}}
}
