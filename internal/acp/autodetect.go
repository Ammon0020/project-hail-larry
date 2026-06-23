package acp

import "os/exec"

// Autodetect searches the system PATH for known agent executables
// and returns their default configurations.
func Autodetect() []AgentInfo {
	var detected []AgentInfo

	if path, err := exec.LookPath("claude"); err == nil {
		detected = append(detected, AgentInfo{
			ID:      "claude-code",
			Name:    "Claude Code",
			Command: path,
			Models: []AgentModel{
				{ID: "claude-3-5-sonnet-20240620", Name: "Claude 3.5 Sonnet"},
				{ID: "claude-3-opus-20240229", Name: "Claude 3 Opus"},
			},
		})
	}

	if path, err := exec.LookPath("codex"); err == nil {
		detected = append(detected, AgentInfo{
			ID:      "codex",
			Name:    "Codex CLI",
			Command: path,
			Models: []AgentModel{
				{ID: "gpt-4o", Name: "GPT-4o"},
				{ID: "gpt-4-turbo", Name: "GPT-4 Turbo"},
			},
		})
	}

	if path, err := exec.LookPath("vibe"); err == nil {
		detected = append(detected, AgentInfo{
			ID:      "mistral-vibe",
			Name:    "Mistral Vibe",
			Command: path,
			Models: []AgentModel{
				{ID: "mistral-large-latest", Name: "Mistral Large"},
				{ID: "mistral-small-latest", Name: "Mistral Small"},
			},
		})
	}

	return detected
}
