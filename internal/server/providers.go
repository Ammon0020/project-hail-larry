package server

import (
	"errors"
	"net/http"
	"strings"

	"github.com/adama/local-agent/internal/acp"
)

// ACP UnstableLlmProtocol values a provider apiType may take.
const (
	llmProtocolAnthropic = "anthropic"
	llmProtocolOpenAI    = "openai"
	llmProtocolAzure     = "azure"
	llmProtocolVertex    = "vertex"
	llmProtocolBedrock   = "bedrock"
)

// validLLMProtocols is the set of accepted apiType values. The SDK's Validate()
// does not check apiType, so the handler rejects unknown values with 400 rather
// than forwarding an opaque error the agent would reject with a 500.
var validLLMProtocols = map[string]bool{
	llmProtocolAnthropic: true,
	llmProtocolOpenAI:    true,
	llmProtocolAzure:     true,
	llmProtocolVertex:    true,
	llmProtocolBedrock:   true,
}

// ----------------------------------------------------------------------------
// ACP Provider management handlers (plan item P4.11)
//
// These expose the unstable ACP providers/list|set|disable methods per live
// session. Providers are agent/connection-scoped in ACP, but our architecture
// spawns one transport per session, so management operates on the session's
// live transport (lazily started if nil, mirroring SwitchModel).
//
// Routes (registered in server.go apiRoutes, requireAuth):
//   GET    /api/sessions/{id}/providers            → list
//   PUT    /api/sessions/{id}/providers/{providerId} → set
//   DELETE /api/sessions/{id}/providers/{providerId} → disable
// ----------------------------------------------------------------------------

// handleListProviders returns the agent's configurable LLM providers for a
// session. Returns 501 when the agent did not advertise the providers
// capability (ErrProvidersUnsupported), 404 when the session doesn't exist,
// and 500 on transport errors.
func (s *Server) handleListProviders(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")
	providers, err := s.deps.ACPClient.ListProviders(r.Context(), sessionID)
	if err != nil {
		writeProviderError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, providers)
}

// handleSetProvider replaces the configuration for a single provider. The
// provider id comes from the path; the body carries apiType, baseUrl, and an
// optional headers map. Returns 400 on a malformed body or missing fields,
// 501 when the agent doesn't support providers, 404 when the session is gone.
func (s *Server) handleSetProvider(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")
	providerID := r.PathValue("providerId")
	if providerID == "" {
		writeError(w, http.StatusBadRequest, "missing provider id in path")
		return
	}

	var req struct {
		APIType string            `json:"apiType"`
		BaseURL string            `json:"baseUrl"`
		Headers map[string]string `json:"headers,omitempty"`
	}
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	if req.APIType == "" {
		writeError(w, http.StatusBadRequest, "apiType is required")
		return
	}
	if !validLLMProtocols[req.APIType] {
		writeError(w, http.StatusBadRequest, "invalid apiType: must be one of anthropic, openai, azure, vertex, bedrock")
		return
	}
	if req.BaseURL == "" {
		writeError(w, http.StatusBadRequest, "baseUrl is required")
		return
	}

	if err := s.deps.ACPClient.SetProvider(r.Context(), sessionID, providerID, req.APIType, req.BaseURL, req.Headers); err != nil {
		writeProviderError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]string{statusKey: "updated"})
}

// handleDisableProvider disables a provider. Refuses (400) when the provider is
// marked Required by the agent — the ACP spec forbids disabling required
// providers, so the guard is enforced here at the handler level (the client
// does not re-check). Returns 501 when the agent doesn't support providers,
// 404 when the session is gone.
func (s *Server) handleDisableProvider(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")
	providerID := r.PathValue("providerId")
	if providerID == "" {
		writeError(w, http.StatusBadRequest, "missing provider id in path")
		return
	}

	// Required-guard: fetch the provider list and refuse if the targeted
	// provider is marked Required. This is an extra round-trip but keeps the
	// guard at the API boundary (the client method is a thin pass-through).
	providers, err := s.deps.ACPClient.ListProviders(r.Context(), sessionID)
	if err != nil {
		writeProviderError(w, err)
		return
	}
	for _, p := range providers {
		if p.ID == providerID {
			if p.Required {
				writeError(w, http.StatusBadRequest, "provider "+providerID+" is required and cannot be disabled")
				return
			}
			break
		}
	}

	if err := s.deps.ACPClient.DisableProvider(r.Context(), sessionID, providerID); err != nil {
		writeProviderError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]string{statusKey: "disabled"})
}

// writeProviderError maps an ACP client error to an HTTP status. The
// ErrProvidersUnsupported sentinel yields 501 (the agent genuinely doesn't
// support the capability); other errors are treated as transport/session
// failures and surface as 500 with the underlying message.
func writeProviderError(w http.ResponseWriter, err error) {
	if errors.Is(err, acp.ErrProvidersUnsupported) {
		writeError(w, http.StatusNotImplemented, "agent does not support provider management")
		return
	}
	// Session-not-found is a client error (404), consistent with the other
	// session handlers in api.go which match the same message.
	if strings.Contains(err.Error(), "session not found") {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}
	writeError(w, http.StatusInternalServerError, err.Error())
}
