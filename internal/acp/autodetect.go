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
	"regexp"
	"runtime"
	"strings"
	"sync"
	"time"

	"github.com/coder/acp-go-sdk"
	"github.com/pelletier/go-toml/v2"
)

// agentSpec defines a known agent and how to discover its models.
type agentSpec struct {
	id   string
	name string
	// commands are tried in order (e.g. "vibe-acp" before "vibe") via exec.LookPath.
	commands []string
	// args are extra arguments passed to the command when launching the agent
	// (e.g. ["acp"] for Cursor CLI's ACP subcommand).
	args []string
	// searchPaths are extra directories to check if none of commands are on PATH.
	// Entries may use ~ (expanded to the user home dir on all platforms) or
	// %LOCALAPPDATA%-style Windows env var syntax. Bare command names (and, on
	// Windows, .exe/.cmd variants) are looked up inside each expanded directory.
	searchPaths []string
	// fallbackModels returned if both ACP and file-based detection fail.
	fallbackModels []AgentModel
	// fileModels reads agent-specific config files for model lists.
	fileModels func() []AgentModel
}

// Agent IDs, names, and model IDs referenced by the autodetect registry.
// Hoisting these into constants keeps the spec table and its callers in sync
// and avoids magic-string drift.
const (
	agentIDCodex          = "codex"
	agentNameClaudeCode   = "Claude Code"
	agentNameMistralLarge = "Mistral Large"
	modelIDGPT4o          = "gpt-4o"
	modelIDClaudeSonnet4  = "claude-sonnet-4"
)

// cursorAgentCommands are the launch commands for the Cursor agent, reused by
// detectAgent and getCursorModelsFromCLI so the two stay in sync.
var cursorAgentCommands = []string{"agent", "cursor-agent"} //nolint:goconst // "agent" is the Cursor CLI binary name, not the chat role.

// vibeCommands are the launch commands for the Mistral Vibe agent.
var vibeCommands = []string{vibeACPCommand, vibeCommand}

const (
	vibeACPCommand = "vibe-acp"
	vibeCommand    = "vibe"
)

// knownAgents is the registry of agents we autodetect.
var knownAgents = []agentSpec{
	{
		id:             "claude-code",
		name:           agentNameClaudeCode,
		commands:       []string{"claude"},
		fallbackModels: []AgentModel{{ID: "claude-3-5-sonnet-20240620", Name: "Claude 3.5 Sonnet"}, {ID: "claude-3-opus-20240229", Name: "Claude 3 Opus"}},
	},
	{
		id:   agentIDCodex,
		name: "Codex CLI",
		// Only the dedicated ACP adapter ("codex-acp") speaks ACP over stdio.
		// The bare "codex" command is the OpenAI Codex CLI — an interactive TUI
		// that requires a TTY on stdin. Spawning it with pipes (as the ACP
		// transport does) makes it exit immediately with "stdin is not a
		// terminal", causing the ACP Initialize handshake to fail with "peer
		// disconnected before response". The codex CLI has no "acp" subcommand;
		// ACP support is provided solely by the separate @agentclientprotocol/
		// codex-acp (or @zed-industries/codex-acp) npm package, which installs
		// the "codex-acp" binary. Do NOT add "codex" as a fallback — it can
		// never work as an ACP agent.
		commands:       []string{"codex-acp"},
		fallbackModels: []AgentModel{{ID: modelIDGPT4o, Name: "GPT-4o"}, {ID: "gpt-4-turbo", Name: "GPT-4 Turbo"}},
		fileModels:     getCodexModelsFromFile,
	},
	{
		id:       "cursor",
		name:     "Cursor Agent",
		commands: cursorAgentCommands,
		args:     []string{"acp"},
		// The cursor.com/install CLI adds these to PATH, but if the daemon was
		// started before the install (or PATH wasn't refreshed), LookPath fails.
		// Fall back to the known install locations.
		searchPaths: []string{
			`%LOCALAPPDATA%\cursor-agent`, // Windows
			`~/.local/bin`,                // macOS / Linux
		},
		fallbackModels: []AgentModel{
			{ID: "auto", Name: "Auto"},
			{ID: "composer-2.5-fast", Name: "Composer 2.5 Fast (default)"},
			{ID: "composer-2.5", Name: "Composer 2.5"},
			{ID: "gpt-5.2", Name: "GPT-5.2"},
			{ID: "claude-opus-4-8-high", Name: "Opus 4.8 1M"},
			{ID: "claude-4.6-sonnet-medium", Name: "Sonnet 4.6 1M"},
			{ID: "gemini-3.1-pro", Name: "Gemini 3.1 Pro"},
			{ID: "grok-4.3", Name: "Grok 4.3 1M"},
		},
		fileModels: getCursorModelsFromCLI,
	},
	// Devin (formerly Windsurf / "chisel") — bundled with Devin Desktop.
	// ACP mode is the "acp" subcommand, like Cursor's "agent acp".
	// providers/list is not supported and there is no --list-models flag,
	// so fallbackModels (sourced from the --model help-text examples) are used.
	// NOTE: the devin binary is NOT on PATH by default — it lives inside the
	// Devin Desktop install. searchPaths covers the Devin and legacy Windsurf
	// bundle locations across Windows, macOS, and Linux.
	{
		id:       "devin",
		name:     "Devin",
		commands: []string{"devin"},
		args:     []string{"acp"},
		searchPaths: []string{
			// Devin Desktop (current naming).
			`%LOCALAPPDATA%\Programs\Devin\resources\app\extensions\windsurf\devin\bin`,    // Windows
			`/Applications/Devin.app/Contents/Resources/app/extensions/windsurf/devin/bin`, // macOS
			`~/.local/share/Devin/resources/app/extensions/windsurf/devin/bin`,             // Linux
			// Legacy Windsurf naming (older installs).
			`%LOCALAPPDATA%\Programs\Windsurf\resources\app\extensions\windsurf\devin\bin`,    // Windows
			`/Applications/Windsurf.app/Contents/Resources/app/extensions/windsurf/devin/bin`, // macOS
		},
		fallbackModels: []AgentModel{
			{ID: modelIDClaudeSonnet4, Name: "Claude Sonnet 4"},
			{ID: "claude-opus-4.6", Name: "Claude Opus 4.6"},
			{ID: "opus", Name: "Opus"},
			{ID: "codex", Name: "Codex"}, //nolint:goconst // "codex" is a Devin model ID that coincidentally equals the codex agent ID.
		},
	},
	{
		id:             "mistral-vibe",
		name:           "Mistral Vibe",
		commands:       vibeCommands, // prefer ACP bridge
		fallbackModels: []AgentModel{{ID: "mistral-large-latest", Name: agentNameMistralLarge}, {ID: "mistral-small-latest", Name: "Mistral Small"}},
		fileModels:     getVibeModelsFromFile,
	},
}

// ValidCommandsForAgent returns the list of command names (bare binaries, e.g.
// "codex-acp", "claude") that are considered valid launch commands for the
// known agent with the given ID. It returns nil when the ID does not match any
// registered agent spec, so callers can distinguish "unknown / user-defined
// agent" (no validation applies) from "known agent with no valid commands"
// (which should never happen).
//
// Callers use this to validate persisted agent entries: a configured command
// is acceptable if it equals one of the returned names or is a filesystem path
// whose base name equals one of them (e.g. "/usr/local/bin/codex-acp" matches
// "codex-acp"). This lets the daemon prune stale entries that point at a
// binary the spec no longer considers a valid ACP transport — for example, a
// persisted "codex" entry whose command is the bare "codex" TUI rather than
// the "codex-acp" adapter.
func ValidCommandsForAgent(id string) []string {
	for _, spec := range knownAgents {
		if spec.id == id {
			return append([]string(nil), spec.commands...)
		}
	}
	return nil
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
	path := findFirstCommand(spec.commands, spec.searchPaths)
	if path == "" {
		return AgentInfo{}, false
	}

	models, acpWarning := tryACPProvidersList(path, spec.args)
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
		Args:    spec.args,
		Models:  models,
		Warning: warning,
	}, true
}

// findFirstCommand returns the first command from the list found on PATH,
// falling back to the provided searchPaths if none are on PATH. For each search
// path it tries the bare command name and, on Windows, the .exe and .cmd
// variants. It returns the full path to the first match, or empty string if
// none are found.
func findFirstCommand(commands, searchPaths []string) string {
	// 1. Try PATH first.
	for _, cmd := range commands {
		if path, err := exec.LookPath(cmd); err == nil {
			return path
		}
	}
	// 2. Fall back to known install locations.
	for _, dir := range searchPaths {
		expandedDir := expandPath(dir)
		if expandedDir == "" {
			continue
		}
		for _, cmd := range commands {
			candidates := []string{cmd}
			if runtime.GOOS == osWindows {
				candidates = append(candidates, cmd+".exe", cmd+".cmd")
			}
			for _, c := range candidates {
				full := filepath.Join(expandedDir, c)
				if info, err := os.Stat(full); err == nil && !info.IsDir() {
					return full
				}
			}
		}
	}
	return ""
}

// expandPath expands a path that may contain a leading ~ (replaced with the
// user's home directory) or Windows %VAR% environment variable references.
// Returns an empty string if expansion fails (e.g. home dir unavailable).
func expandPath(p string) string {
	// Expand a leading ~ to the user's home directory.
	if strings.HasPrefix(p, "~") {
		home, err := os.UserHomeDir()
		if err != nil {
			return ""
		}
		p = filepath.Join(home, p[1:])
	}
	// Expand %VAR% references (Windows env var syntax). os.Expand only
	// handles $VAR and ${VAR}, so we do %VAR% manually.
	p = expandWindowsEnv(p)
	return p
}

// windowsEnvRe matches %VAR%-style environment variable references on Windows.
var windowsEnvRe = regexp.MustCompile(`%[A-Za-z_][A-Za-z0-9_]*%`)

// expandWindowsEnv replaces %VAR% references with the corresponding env value.
// On non-Windows platforms it is a no-op (the syntax is not used there), but it
// is kept platform-agnostic so tests can exercise it regardless of GOOS.
func expandWindowsEnv(s string) string {
	return windowsEnvRe.ReplaceAllStringFunc(s, func(m string) string {
		key := m[1 : len(m)-1] // strip the % delimiters
		return os.Getenv(key)
	})
}

// tryACPProvidersList spawns the agent, performs an ACP Initialize handshake,
// and queries available models via the unstable providers/list method.
// Returns nil plus a warning if any step fails.
func tryACPProvidersList(command string, args []string) ([]AgentModel, string) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, command, args...) //nolint:gosec // command comes from the trusted known-agents registry, not user input
	// Capture stderr into a bounded ring buffer instead of discarding it. The
	// probe still doesn't stream agent stderr to daemon output, but retaining
	// the tail lets us include it in the warning string when the probe fails,
	// so users can see *why* a detected agent failed the ACP handshake (e.g.
	// "stdin is not a terminal", missing auth, wrong subcommand) instead of a
	// bare "initialize failed".
	stderrBuf := newRingBuffer(4 << 10) // 4 KiB tail
	cmd.Stderr = stderrBuf
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return nil, fmt.Sprintf("open stdin pipe: %v", err)
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, fmt.Sprintf("open stdout pipe: %v", err)
	}
	if startErr := cmd.Start(); startErr != nil {
		warning := fmt.Sprintf("start %s: %v", command, startErr)
		return nil, warning
	}
	defer cleanupAutodetectProcess(cmd, stdin)

	// withStderr appends the captured agent stderr tail to a warning when
	// present, so failure diagnostics explain *why* the probe failed rather
	// than just naming the failing step. It is only meaningful after the
	// process has started (earlier failures can't have produced stderr).
	withStderr := func(warning string) string {
		if tail := strings.TrimSpace(stderrBuf.String()); tail != "" {
			return fmt.Sprintf("%s (agent stderr: %s)", warning, tail)
		}
		return warning
	}

	client := acp.NewClientSideConnection(&dummyClientImpl{}, stdin, stdout)
	// Suppress ACP SDK diagnostic logging (e.g. "connection closed") during probing.
	client.SetLogger(slog.New(slog.NewTextHandler(io.Discard, nil)))
	initReq := acp.InitializeRequest{
		ClientInfo:         &acp.Implementation{Name: "local-agent-autodetect", Version: "1.0"},
		ClientCapabilities: acp.ClientCapabilities{},
	}
	if err = initReq.Validate(); err != nil {
		warning := fmt.Sprintf("validate initialize request: %v", err)
		return nil, warning
	}
	if _, err = client.Initialize(ctx, initReq); err != nil {
		return nil, withStderr(fmt.Sprintf("initialize failed: %v", err))
	}

	listReq := acp.UnstableListProvidersRequest{}
	if err = listReq.Validate(); err != nil {
		warning := fmt.Sprintf("validate list providers request: %v", err)
		return nil, warning
	}
	listRes, err := client.UnstableListProviders(ctx, listReq)
	if err != nil {
		return nil, withStderr(fmt.Sprintf("providers/list failed: %v", err))
	}

	models := make([]AgentModel, 0, len(listRes.Providers))
	for _, p := range listRes.Providers {
		models = append(models, AgentModel{
			ID:   p.Id,
			Name: p.Id,
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

// getCursorModelsFromCLI runs `agent --list-models` (or `cursor-agent
// --list-models`) and parses the output. The Cursor CLI prints lines like:
//
//	auto - Auto
//	composer-2.5-fast - Composer 2.5 Fast (default)
//	gpt-5.2 - GPT-5.2
//
// We parse "id - display name" pairs. Lines that don't match (blank, headers,
// tips) are skipped. Falls back to nil (triggering fallbackModels) if the
// command is not found or fails.
func getCursorModelsFromCLI() []AgentModel {
	// Use the same search logic as detectAgent — PATH first, then the Cursor
	// CLI install directory. This ensures model discovery works even when the
	// daemon was started before the Cursor CLI installer added it to PATH.
	cmdPath := findFirstCommand(
		cursorAgentCommands,
		[]string{
			"%LOCALAPPDATA%/cursor-agent",
			"~/.local/bin",
		},
	)
	if cmdPath == "" {
		return nil
	}

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	out, err := exec.CommandContext(ctx, cmdPath, "--list-models").Output() //nolint:gosec // cmdPath is resolved from the trusted known-agents registry, not user input
	if err != nil {
		log.Printf("autodetect: cursor --list-models failed: %v", err)
		return nil
	}

	var models []AgentModel
	for _, line := range strings.Split(string(out), "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "Available models") || strings.HasPrefix(line, "Tip:") {
			continue
		}
		// Format: "id - Display Name"
		idx := strings.Index(line, " - ")
		if idx < 0 {
			continue
		}
		id := strings.TrimSpace(line[:idx])
		name := strings.TrimSpace(line[idx+3:])
		if id == "" {
			continue
		}
		models = append(models, AgentModel{ID: id, Name: name})
	}
	return models
}
