package acp

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"sync"
	"time"

	"github.com/coder/acp-go-sdk"
	"github.com/pelletier/go-toml/v2"
)

// agentSpec defines a known agent and how to discover its models.
type agentSpec struct {
	id       string
	name     string
	commands []string // tried in order (e.g. "vibe-acp" before "vibe")
	// fallbackModels returned if both ACP and file-based detection fail.
	fallbackModels []AgentModel
	// fileModels reads agent-specific config files for model lists.
	fileModels func() []AgentModel
}

// knownAgents is the registry of agents we autodetect.
var knownAgents = []agentSpec{
	{
		id:             "claude-code",
		name:           "Claude Code",
		commands:       []string{"claude"},
		fallbackModels: []AgentModel{{ID: "claude-3-5-sonnet-20240620", Name: "Claude 3.5 Sonnet"}, {ID: "claude-3-opus-20240229", Name: "Claude 3 Opus"}},
	},
	{
		id:             "codex",
		name:           "Codex CLI",
		commands:       []string{"codex"},
		fallbackModels: []AgentModel{{ID: "gpt-4o", Name: "GPT-4o"}, {ID: "gpt-4-turbo", Name: "GPT-4 Turbo"}},
		fileModels:     getCodexModelsFromFile,
	},
	{
		id:             "mistral-vibe",
		name:           "Mistral Vibe",
		commands:       []string{"vibe-acp", "vibe"}, // prefer ACP bridge
		fallbackModels: []AgentModel{{ID: "mistral-large-latest", Name: "Mistral Large"}, {ID: "mistral-small-latest", Name: "Mistral Small"}},
		fileModels:     getVibeModelsFromFile,
	},
}

// Autodetect searches the system PATH for known agent executables
// and returns their discovered configurations.
//
// For each agent, model discovery follows a three-tier fallback:
//  1. ACP providers/list handshake (live query)
//  2. Agent-specific config file (e.g. ~/.codex/models_cache.json)
//  3. Hardcoded fallback list
func Autodetect() []AgentInfo {
	results := make([]AgentInfo, len(knownAgents))
	found := make([]bool, len(knownAgents))

	var wg sync.WaitGroup
	for i, spec := range knownAgents {
		wg.Add(1)
		go func(i int, spec agentSpec) {
			defer wg.Done()
			agent, ok := detectAgent(spec)
			if !ok {
				return
			}
			results[i] = agent
			found[i] = true
		}(i, spec)
	}
	wg.Wait()

	detected := make([]AgentInfo, 0, len(knownAgents))
	for i, agent := range results {
		if found[i] {
			detected = append(detected, agent)
		}
	}
	return detected
}

func detectAgent(spec agentSpec) (AgentInfo, bool) {
	path := findFirstCommand(spec.commands)
	if path == "" {
		return AgentInfo{}, false
	}

	models, acpWarning := tryACPProvidersList(path)
	var warning string
	if len(models) == 0 && spec.fileModels != nil {
		models = spec.fileModels()
	}
	if len(models) == 0 {
		models = spec.fallbackModels
		warning = "Using fallback model list"
		if acpWarning != "" {
			log.Printf("autodetect: %s — ACP probe failed (%s), no config file, using fallback models", spec.name, acpWarning)
		}
	} else if acpWarning != "" && len(models) > 0 {
		// ACP failed but file-based detection succeeded — quiet warning
		warning = ""
	}

	return AgentInfo{
		ID:      spec.id,
		Name:    spec.name,
		Command: path,
		Models:  models,
		Warning: warning,
	}, true
}

// findFirstCommand returns the first command from the list found in PATH,
// or empty string if none are found.
func findFirstCommand(commands []string) string {
	for _, cmd := range commands {
		if path, err := exec.LookPath(cmd); err == nil {
			return path
		}
	}
	return ""
}

// tryACPProvidersList spawns the agent, performs an ACP Initialize handshake,
// and queries available models via the unstable providers/list method.
// Returns nil plus a warning if any step fails.
func tryACPProvidersList(command string) ([]AgentModel, string) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, command)
	// Discard stderr — autodetect is a probe, not a real session.
	// Agent stderr (e.g. "stdin is not a terminal") should not pollute daemon output.
	cmd.Stderr = io.Discard
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return nil, fmt.Sprintf("open stdin pipe: %v", err)
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, fmt.Sprintf("open stdout pipe: %v", err)
	}
	if err := cmd.Start(); err != nil {
		warning := fmt.Sprintf("start %s: %v", command, err)
		return nil, warning
	}
	defer cleanupAutodetectProcess(cmd, stdin)

	client := acp.NewClientSideConnection(&dummyClientImpl{}, stdin, stdout)
	// Suppress ACP SDK diagnostic logging (e.g. "connection closed") during probing.
	client.SetLogger(slog.New(slog.NewTextHandler(io.Discard, nil)))
	if _, err = client.Initialize(ctx, acp.InitializeRequest{
		ClientInfo:         &acp.Implementation{Name: "local-agent-autodetect", Version: "1.0"},
		ClientCapabilities: acp.ClientCapabilities{},
	}); err != nil {
		warning := fmt.Sprintf("initialize failed: %v", err)
		return nil, warning
	}

	listRes, err := client.UnstableListProviders(ctx, acp.UnstableListProvidersRequest{})
	if err != nil {
		warning := fmt.Sprintf("providers/list failed: %v", err)
		return nil, warning
	}

	models := make([]AgentModel, 0, len(listRes.Providers))
	for _, p := range listRes.Providers {
		models = append(models, AgentModel{
			ID:   string(p.Id),
			Name: string(p.Id),
		})
	}
	return models, ""
}

func cleanupAutodetectProcess(cmd *exec.Cmd, stdin interface{ Close() error }) {
	_ = stdin.Close()

	done := make(chan struct{})
	go func() {
		_ = cmd.Wait()
		close(done)
	}()

	select {
	case <-done:
		return
	case <-time.After(250 * time.Millisecond):
	}

	if cmd.Process != nil {
		_ = cmd.Process.Kill()
	}
	<-done
}

// dummyClientImpl satisfies acp.Client for handshake-only connections.
// All methods are no-ops — we only need Initialize + UnstableListProviders.
type dummyClientImpl struct{}

func (dummyClientImpl) SessionUpdate(_ context.Context, _ acp.SessionNotification) error { return nil }
func (dummyClientImpl) RequestPermission(_ context.Context, _ acp.RequestPermissionRequest) (acp.RequestPermissionResponse, error) {
	return acp.RequestPermissionResponse{}, nil
}
func (dummyClientImpl) ReadTextFile(_ context.Context, _ acp.ReadTextFileRequest) (acp.ReadTextFileResponse, error) {
	return acp.ReadTextFileResponse{}, nil
}
func (dummyClientImpl) WriteTextFile(_ context.Context, _ acp.WriteTextFileRequest) (acp.WriteTextFileResponse, error) {
	return acp.WriteTextFileResponse{}, nil
}
func (dummyClientImpl) CreateTerminal(_ context.Context, _ acp.CreateTerminalRequest) (acp.CreateTerminalResponse, error) {
	return acp.CreateTerminalResponse{}, nil
}
func (dummyClientImpl) KillTerminal(_ context.Context, _ acp.KillTerminalRequest) (acp.KillTerminalResponse, error) {
	return acp.KillTerminalResponse{}, nil
}
func (dummyClientImpl) TerminalOutput(_ context.Context, _ acp.TerminalOutputRequest) (acp.TerminalOutputResponse, error) {
	return acp.TerminalOutputResponse{}, nil
}
func (dummyClientImpl) ReleaseTerminal(_ context.Context, _ acp.ReleaseTerminalRequest) (acp.ReleaseTerminalResponse, error) {
	return acp.ReleaseTerminalResponse{}, nil
}
func (dummyClientImpl) WaitForTerminalExit(_ context.Context, _ acp.WaitForTerminalExitRequest) (acp.WaitForTerminalExitResponse, error) {
	return acp.WaitForTerminalExitResponse{}, nil
}

// getCodexModelsFromFile reads ~/.codex/models_cache.json for model discovery.
func getCodexModelsFromFile() []AgentModel {
	b, err := readConfigFile(filepath.Join(".codex", "models_cache.json"))
	if err != nil {
		return nil
	}
	var data struct {
		Models []struct {
			Slug        string `json:"slug"`
			DisplayName string `json:"display_name"`
		} `json:"models"`
	}
	if err := json.Unmarshal(b, &data); err != nil {
		log.Printf("autodetect: codex models_cache.json parse error: %v", err)
		return nil
	}
	models := make([]AgentModel, 0, len(data.Models))
	for _, m := range data.Models {
		models = append(models, AgentModel{ID: m.Slug, Name: m.DisplayName})
	}
	return models
}

// getVibeModelsFromFile reads ~/.vibe/config.toml for model discovery.
func getVibeModelsFromFile() []AgentModel {
	b, err := readConfigFile(filepath.Join(".vibe", "config.toml"))
	if err != nil {
		return nil
	}
	var data struct {
		Models []struct {
			Name  string `toml:"name"`
			Alias string `toml:"alias"`
		} `toml:"models"`
	}
	if err := toml.Unmarshal(b, &data); err != nil {
		log.Printf("autodetect: vibe config.toml parse error: %v", err)
		return nil
	}
	models := make([]AgentModel, 0, len(data.Models))
	for _, m := range data.Models {
		// Use alias as ID if available (shorter, more stable); fall back to name.
		// Display name is always the full model name.
		id := m.Alias
		if id == "" {
			id = m.Name
		}
		models = append(models, AgentModel{ID: id, Name: m.Name})
	}
	return models
}

// readConfigFile reads a file relative to the user's home directory.
func readConfigFile(relPath string) ([]byte, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil, fmt.Errorf("get home dir: %w", err)
	}
	return os.ReadFile(filepath.Join(home, relPath)) //nolint:gosec // path is constructed from home dir + known relative path
}
