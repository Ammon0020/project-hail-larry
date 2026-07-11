package acp

import (
	"testing"

	acpsdk "github.com/coder/acp-go-sdk"
)

// makeSelectOpt builds a SessionConfigOption with a Select variant and the
// given fields. category is nil when cat is empty.
func makeSelectOpt(id, name, cat, current string, values ...string) acpsdk.SessionConfigOption {
	opt := acpsdk.SessionConfigOption{
		Select: &acpsdk.SessionConfigOptionSelect{
			Id:           acpsdk.SessionConfigId(id),
			Name:         name,
			CurrentValue: acpsdk.SessionConfigValueId(current),
			Type:         "select",
		},
	}
	if cat != "" {
		c := acpsdk.SessionConfigOptionCategory(cat)
		opt.Select.Category = &c
	}
	if len(values) > 0 {
		ungrouped := make(acpsdk.SessionConfigSelectOptionsUngrouped, 0, len(values))
		for _, v := range values {
			ungrouped = append(ungrouped, acpsdk.SessionConfigSelectOption{
				Value: acpsdk.SessionConfigValueId(v),
				Name:  v,
			})
		}
		opt.Select.Options.Ungrouped = &ungrouped
	}
	return opt
}

// TestFindModelConfigID_CategoryMatch covers pass 1: explicit category "model".
func TestFindModelConfigID_CategoryMatch(t *testing.T) {
	opts := []acpsdk.SessionConfigOption{
		makeSelectOpt("thought", "Thought level", "thought_level", "low"),
		makeSelectOpt("model", "Model", "model", "claude-sonnet-4", "claude-sonnet-4", "claude-opus-4"),
	}
	got := findModelConfigID(opts, nil)
	if got != "model" {
		t.Errorf("category match: got %q, want %q", got, "model")
	}
}

// TestFindModelConfigID_CategoryNil covers the Mistral Vibe case: the agent
// omits category on its model selector (which the ACP spec permits). Pass 1
// fails, and a fallback pass must catch it.
func TestFindModelConfigID_CategoryNil(t *testing.T) {
	known := []AgentModel{{ID: "mistral-medium-3.5", Name: "Mistral Medium 3.5"}}
	opts := []acpsdk.SessionConfigOption{
		makeSelectOpt("thought", "Thought level", "thought_level", "low"),
		// Model selector with NO category — the spec-compliant case that
		// broke our original implementation.
		makeSelectOpt("model", "Model", "", "mistral-medium-3.5",
			"mistral-small", "mistral-medium-3.5", "mistral-large"),
	}
	got := findModelConfigID(opts, known)
	if got != "model" {
		t.Errorf("nil category fallback: got %q, want %q", got, "model")
	}
}

// TestFindModelConfigID_IdConventions covers pass 2: option id == "model"
// with no category and a non-obvious name.
func TestFindModelConfigID_IdConventions(t *testing.T) {
	opts := []acpsdk.SessionConfigOption{
		makeSelectOpt("model", "Provider", "", "gpt-4o", "gpt-4o", "gpt-4o-mini"),
	}
	got := findModelConfigID(opts, nil)
	if got != "model" {
		t.Errorf("id convention: got %q, want %q", got, "model")
	}
}

// TestFindModelConfigID_NameContainsModel covers pass 3: name contains
// "model" (case-insensitive) with no category and a non-conventional id.
func TestFindModelConfigID_NameContainsModel(t *testing.T) {
	opts := []acpsdk.SessionConfigOption{
		makeSelectOpt("cfg_1", "Choose Model", "", "gpt-4o", "gpt-4o", "gpt-4o-mini"),
	}
	got := findModelConfigID(opts, nil)
	if got != "cfg_1" {
		t.Errorf("name contains model: got %q, want %q", got, "cfg_1")
	}
}

// TestFindModelConfigID_NameCaseInsensitive verifies pass 3 is case-insensitive.
func TestFindModelConfigID_NameCaseInsensitive(t *testing.T) {
	opts := []acpsdk.SessionConfigOption{
		makeSelectOpt("cfg_2", "MODEL SELECTOR", "", "gpt-4o", "gpt-4o"),
	}
	got := findModelConfigID(opts, nil)
	if got != "cfg_2" {
		t.Errorf("case-insensitive name: got %q, want %q", got, "cfg_2")
	}
}

// TestFindModelConfigID_KnownModelCurrentValue covers pass 4: the option's
// current value matches a known model ID from the agent registry. This is
// the strongest fallback for agents that omit category and use generic
// id/name values.
func TestFindModelConfigID_KnownModelCurrentValue(t *testing.T) {
	known := []AgentModel{
		{ID: "mistral-medium-3.5", Name: "Mistral Medium 3.5"},
		{ID: "mistral-large", Name: "Mistral Large"},
	}
	opts := []acpsdk.SessionConfigOption{
		makeSelectOpt("cfg_42", "Provider", "", "mistral-medium-3.5",
			"mistral-small", "mistral-medium-3.5", "mistral-large"),
	}
	got := findModelConfigID(opts, known)
	if got != "cfg_42" {
		t.Errorf("known model current value: got %q, want %q", got, "cfg_42")
	}
}

// TestFindModelConfigID_KnownModelOptionValue covers pass 4: the current
// value doesn't match, but one of the option values does.
func TestFindModelConfigID_KnownModelOptionValue(t *testing.T) {
	known := []AgentModel{{ID: "mistral-large", Name: "Mistral Large"}}
	opts := []acpsdk.SessionConfigOption{
		makeSelectOpt("cfg_99", "Provider", "", "mistral-small",
			"mistral-small", "mistral-medium-3.5", "mistral-large"),
	}
	got := findModelConfigID(opts, known)
	if got != "cfg_99" {
		t.Errorf("known model option value: got %q, want %q", got, "cfg_99")
	}
}

// TestFindModelConfigID_NoMatch returns empty when no config option can be
// identified as the model selector.
func TestFindModelConfigID_NoMatch(t *testing.T) {
	opts := []acpsdk.SessionConfigOption{
		makeSelectOpt("thought", "Thought level", "thought_level", "low"),
		makeSelectOpt("theme", "Theme", "", "dark", "dark", "light"),
	}
	got := findModelConfigID(opts, []AgentModel{{ID: "gpt-4o"}})
	if got != "" {
		t.Errorf("no match: got %q, want empty", got)
	}
}

// TestFindModelConfigID_EmptyOpts returns empty for an empty slice.
func TestFindModelConfigID_EmptyOpts(t *testing.T) {
	got := findModelConfigID(nil, nil)
	if got != "" {
		t.Errorf("empty opts: got %q, want empty", got)
	}
}

// TestFindModelConfigID_BooleanOnly returns empty when the agent only
// advertises Boolean config options (no Select variant).
func TestFindModelConfigID_BooleanOnly(t *testing.T) {
	boolOpt := acpsdk.SessionConfigOption{
		Boolean: &acpsdk.SessionConfigOptionBoolean{
			Id:           "verbose",
			Name:         "Verbose output",
			CurrentValue: false,
			Type:         "boolean",
		},
	}
	got := findModelConfigID([]acpsdk.SessionConfigOption{boolOpt}, nil)
	if got != "" {
		t.Errorf("boolean only: got %q, want empty", got)
	}
}

// TestFindModelConfigID_CategoryPrecedence verifies that pass 1 (category)
// wins over pass 2/3/4 when multiple options could match.
func TestFindModelConfigID_CategoryPrecedence(t *testing.T) {
	opts := []acpsdk.SessionConfigOption{
		// Pass 2 would match this (id == "model"), but pass 1 should
		// match the next one first.
		makeSelectOpt("model", "Provider", "", "gpt-4o"),
		makeSelectOpt("real_model", "Model", "model", "claude-sonnet-4"),
	}
	got := findModelConfigID(opts, nil)
	if got != "real_model" {
		t.Errorf("category precedence: got %q, want %q", got, "real_model")
	}
}

// TestFindModelConfigID_GroupedOptions covers pass 4 with grouped options
// (SessionConfigSelectOptionsGrouped instead of Ungrouped).
func TestFindModelConfigID_GroupedOptions(t *testing.T) {
	known := []AgentModel{{ID: "claude-opus-4", Name: "Claude Opus 4"}}
	opt := makeSelectOpt("cfg_grouped", "Provider", "", "claude-sonnet-4")
	// Replace the options with grouped ones.
	group := acpsdk.SessionConfigSelectGroup{
		Group: "anthropic",
		Name:  "Anthropic",
		Options: []acpsdk.SessionConfigSelectOption{
			{Value: "claude-sonnet-4", Name: "Sonnet 4"},
			{Value: "claude-opus-4", Name: "Opus 4"},
		},
	}
	grouped := acpsdk.SessionConfigSelectOptionsGrouped{group}
	opt.Select.Options.Ungrouped = nil
	opt.Select.Options.Grouped = &grouped

	got := findModelConfigID([]acpsdk.SessionConfigOption{opt}, known)
	if got != "cfg_grouped" {
		t.Errorf("grouped options: got %q, want %q", got, "cfg_grouped")
	}
}
