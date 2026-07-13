package acp

import (
	"fmt"
	"sync"
	"testing"

	"github.com/adama/local-agent/internal/interfaces"
)

// sampleAgent returns a fully-populated AgentInfo used across the table-driven
// tests. It includes Command, Args, Models, and Warning so every projection
// and copy path is exercised.
func sampleAgent(id string) AgentInfo {
	return AgentInfo{
		ID:      id,
		Name:    "Agent " + id,
		Command: "agent-bin",
		Args:    []string{"--flag", "value"},
		Models: []interfaces.AgentModel{
			{ID: "model-a", Name: "Model A"},
			{ID: "model-b", Name: "Model B"},
		},
		Warning: "handle with care",
	}
}

// findAgentByID locates an entry in a slice by ID, returning ok=false when
// absent. Used because list() does not guarantee ordering.
func findAgentByID(agents []interfaces.AgentInfo, id string) (interfaces.AgentInfo, bool) {
	for _, a := range agents {
		if a.ID == id {
			return a, true
		}
	}
	return interfaces.AgentInfo{}, false
}

func TestAgentRegistryListOmitsCommandAndArgs(t *testing.T) {
	r := newAgentRegistry()
	r.register(sampleAgent("alpha"))

	got := r.list()
	if len(got) != 1 {
		t.Fatalf("expected 1 agent in list, got %d", len(got))
	}
	a := got[0]
	if a.ID != "alpha" {
		t.Errorf("ID = %q, want alpha", a.ID)
	}
	if a.Name != "Agent alpha" {
		t.Errorf("Name = %q, want 'Agent alpha'", a.Name)
	}
	if a.Warning != "handle with care" {
		t.Errorf("Warning = %q, want 'handle with care'", a.Warning)
	}
	if a.Command != "" {
		t.Errorf("Command = %q, want empty (list must omit Command)", a.Command)
	}
	if a.Args != nil {
		t.Errorf("Args = %v, want nil (list must omit Args)", a.Args)
	}
	if len(a.Models) != 2 {
		t.Fatalf("expected 2 models, got %d", len(a.Models))
	}
	if a.Models[0] != (interfaces.AgentModel{ID: "model-a", Name: "Model A"}) {
		t.Errorf("Models[0] = %+v, want {model-a Model A}", a.Models[0])
	}
}

func TestAgentRegistryRegisterUpsertReplaces(t *testing.T) {
	r := newAgentRegistry()
	r.register(sampleAgent("alpha"))
	r.register(AgentInfo{
		ID:      "alpha",
		Name:    "Replaced",
		Command: "new-bin",
		Models:  []interfaces.AgentModel{{ID: "model-x", Name: "Model X"}},
	})

	got, ok := r.get("alpha")
	if !ok {
		t.Fatal("expected alpha to be present after upsert")
	}
	if got.Name != "Replaced" {
		t.Errorf("Name = %q, want 'Replaced'", got.Name)
	}
	if got.Command != "new-bin" {
		t.Errorf("Command = %q, want 'new-bin'", got.Command)
	}
	if len(got.Models) != 1 || got.Models[0].ID != "model-x" {
		t.Errorf("Models = %v, want single model-x", got.Models)
	}
	if got.Args != nil {
		t.Errorf("Args = %v, want nil", got.Args)
	}
}

func TestAgentRegistryRemove(t *testing.T) {
	t.Run("existing", func(t *testing.T) {
		r := newAgentRegistry()
		r.register(sampleAgent("alpha"))
		r.remove("alpha")
		if _, ok := r.get("alpha"); ok {
			t.Fatal("expected alpha to be removed")
		}
		if got := r.list(); len(got) != 0 {
			t.Fatalf("expected empty list, got %d entries", len(got))
		}
	})
	t.Run("unknown is no-op", func(t *testing.T) {
		r := newAgentRegistry()
		r.register(sampleAgent("alpha"))
		// Removing an unknown ID must not panic and must not affect existing
		// entries.
		r.remove("does-not-exist")
		if got := r.list(); len(got) != 1 {
			t.Fatalf("expected 1 entry after no-op remove, got %d", len(got))
		}
	})
}

func TestAgentRegistryResolve(t *testing.T) {
	r := newAgentRegistry()
	r.register(sampleAgent("alpha"))

	tests := []struct {
		name     string
		agentID  string
		modelID  string
		wantErr  string
		wantFull bool // expect a populated descriptor returned on success
	}{
		{
			name:     "known agent and model",
			agentID:  "alpha",
			modelID:  "model-a",
			wantFull: true,
		},
		{
			name:    "unknown agent",
			agentID: "missing",
			modelID: "model-a",
			wantErr: "agent not found: missing",
		},
		{
			name:    "unknown model",
			agentID: "alpha",
			modelID: "model-z",
			wantErr: "model model-z not available for agent alpha",
		},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got, err := r.resolve(tc.agentID, tc.modelID)
			if tc.wantErr != "" {
				if err == nil {
					t.Fatalf("expected error %q, got nil", tc.wantErr)
				}
				if err.Error() != tc.wantErr {
					t.Fatalf("error = %q, want %q", err.Error(), tc.wantErr)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if !tc.wantFull {
				return
			}
			// resolve returns the full descriptor including Command and Args.
			if got.ID != tc.agentID {
				t.Errorf("ID = %q, want %q", got.ID, tc.agentID)
			}
			if got.Command != "agent-bin" {
				t.Errorf("Command = %q, want 'agent-bin'", got.Command)
			}
			if len(got.Args) != 2 {
				t.Errorf("Args = %v, want 2 entries", got.Args)
			}
		})
	}

	t.Run("empty model list rejects all model IDs", func(t *testing.T) {
		r := newAgentRegistry()
		r.register(AgentInfo{ID: "nomodels", Name: "No Models", Command: "bin"})
		_, err := r.resolve("nomodels", "any-model")
		if err == nil {
			t.Fatal("expected error for agent with no models, got nil")
		}
		want := "model any-model not available for agent nomodels"
		if err.Error() != want {
			t.Fatalf("error = %q, want %q", err.Error(), want)
		}
	})
}

func TestAgentRegistryGet(t *testing.T) {
	r := newAgentRegistry()
	r.register(sampleAgent("alpha"))

	t.Run("known returns full descriptor", func(t *testing.T) {
		got, ok := r.get("alpha")
		if !ok {
			t.Fatal("expected ok==true for known agent")
		}
		if got.Command != "agent-bin" {
			t.Errorf("Command = %q, want 'agent-bin'", got.Command)
		}
		if len(got.Args) != 2 {
			t.Errorf("Args = %v, want 2 entries", got.Args)
		}
		if len(got.Models) != 2 {
			t.Errorf("Models = %v, want 2 entries", got.Models)
		}
		if got.Warning != "handle with care" {
			t.Errorf("Warning = %q, want 'handle with care'", got.Warning)
		}
	})
	t.Run("unknown returns ok false", func(t *testing.T) {
		got, ok := r.get("missing")
		if ok {
			t.Fatal("expected ok==false for unknown agent")
		}
		if got.ID != "" || got.Command != "" || got.Args != nil || got.Models != nil {
			t.Errorf("got = %+v, want zero AgentInfo", got)
		}
	})
}

// TestAgentRegistryDefensiveCopyIngress verifies that mutating the input
// Args/Models slices after register does not affect the stored value.
func TestAgentRegistryDefensiveCopyIngress(t *testing.T) {
	r := newAgentRegistry()
	in := sampleAgent("alpha")
	r.register(in)

	// Mutate the caller-side slices after registration.
	in.Args[0] = "tampered"
	in.Models[0] = interfaces.AgentModel{ID: "tampered", Name: "Tampered"}

	got, ok := r.get("alpha")
	if !ok {
		t.Fatal("expected alpha to be present")
	}
	if got.Args[0] != "--flag" {
		t.Errorf("Args[0] = %q, want '--flag' (ingress copy failed)", got.Args[0])
	}
	if got.Models[0].ID != "model-a" {
		t.Errorf("Models[0].ID = %q, want 'model-a' (ingress copy failed)", got.Models[0].ID)
	}
}

// TestAgentRegistryDefensiveCopyEgress verifies that mutating the value
// returned by get does not affect subsequent gets or the stored entry.
func TestAgentRegistryDefensiveCopyEgress(t *testing.T) {
	r := newAgentRegistry()
	r.register(sampleAgent("alpha"))

	first, _ := r.get("alpha")
	first.Args[0] = "mutated"
	first.Models[0] = interfaces.AgentModel{ID: "mutated", Name: "Mutated"}

	second, _ := r.get("alpha")
	if second.Args[0] != "--flag" {
		t.Errorf("second.Args[0] = %q, want '--flag' (egress copy failed)", second.Args[0])
	}
	if second.Models[0].ID != "model-a" {
		t.Errorf("second.Models[0].ID = %q, want 'model-a' (egress copy failed)", second.Models[0].ID)
	}

	// The list projection must also be unaffected.
	listed := r.list()
	if len(listed) != 1 {
		t.Fatalf("expected 1 entry in list, got %d", len(listed))
	}
	if listed[0].Models[0].ID != "model-a" {
		t.Errorf("list.Models[0].ID = %q, want 'model-a'", listed[0].Models[0].ID)
	}
}

// TestAgentRegistryConcurrent exercises register/upsert/get/resolve/list/remove
// concurrently across distinct IDs. Run with -race to detect data races; the
// final state must contain exactly the one deterministic entry per worker.
func TestAgentRegistryConcurrent(t *testing.T) {
	r := newAgentRegistry()

	const workers = 8
	const iterations = 200

	var wg sync.WaitGroup
	for w := 0; w < workers; w++ {
		w := w
		wg.Add(1)
		go func() {
			defer wg.Done()
			for i := 0; i < iterations; i++ {
				// Each worker owns a disjoint set of IDs derived from its
				// worker index so upserts collide only within a worker.
				id := fmt.Sprintf("agent-%d-%d", w, i%4)
				r.register(AgentInfo{
					ID:      id,
					Name:    "Worker " + id,
					Command: "bin",
					Args:    []string{"a", "b"},
					Models:  []interfaces.AgentModel{{ID: "m1", Name: "M1"}},
				})
				if _, err := r.resolve(id, "m1"); err != nil {
					t.Errorf("resolve(%s): %v", id, err)
					return
				}
				if _, ok := r.get(id); !ok {
					t.Errorf("get(%s): not found after register", id)
					return
				}
				_ = r.list()
				if i%5 == 0 {
					r.remove(id)
				}
			}
			// After the churn loop, each worker registers one deterministic
			// survivor ID and leaves it in the registry. This makes the final
			// state independent of the interleaving of removes/re-registers
			// above.
			survivor := fmt.Sprintf("survivor-%d", w)
			r.register(AgentInfo{
				ID:      survivor,
				Name:    "Survivor " + survivor,
				Command: "bin",
				Args:    []string{"a", "b"},
				Models:  []interfaces.AgentModel{{ID: "m1", Name: "M1"}},
			})
		}()
	}
	wg.Wait()

	got := r.list()
	for w := 0; w < workers; w++ {
		wantID := fmt.Sprintf("survivor-%d", w)
		a, ok := findAgentByID(got, wantID)
		if !ok {
			t.Fatalf("missing expected survivor %q (list len=%d)", wantID, len(got))
		}
		if a.Name != "Survivor "+wantID {
			t.Errorf("survivor %q Name = %q, want %q", wantID, a.Name, "Survivor "+wantID)
		}
	}
}
