package acp

import (
	"testing"

	"github.com/coder/acp-go-sdk"
)

func TestSelectAgentAuthMethod(t *testing.T) {
	if got := selectAgentAuthMethod(nil); got != "" {
		t.Errorf("nil methods = %q, want empty", got)
	}

	// Prefer Agent method ID (Devin's browser PKCE shape).
	methods := []acp.AuthMethod{
		{Agent: &acp.AuthMethodAgent{Id: "devin-browser", Name: "Log in with browser"}},
	}
	if got := selectAgentAuthMethod(methods); got != "devin-browser" {
		t.Errorf("agent method = %q, want devin-browser", got)
	}

	// Env-var methods need UI credentials — skip them.
	methods = []acp.AuthMethod{
		{EnvVar: &acp.AuthMethodEnvVarInline{Id: "api-key", Name: "API Key", Type: "env_var"}},
	}
	if got := selectAgentAuthMethod(methods); got != "" {
		t.Errorf("env_var method = %q, want empty (needs UI)", got)
	}

	// Agent with empty Id is skipped.
	methods = []acp.AuthMethod{{Agent: &acp.AuthMethodAgent{Id: "", Name: "x"}}}
	if got := selectAgentAuthMethod(methods); got != "" {
		t.Errorf("empty id = %q, want empty", got)
	}
}
