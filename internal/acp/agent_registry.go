package acp

import (
	"fmt"
	"sync"

	"github.com/adama/local-agent/internal/interfaces"
)

// agentRegistry holds the catalog of registered agent harnesses. It owns its
// own RWMutex so agent registration/listing no longer contends on Client.mu.
//
// Lock ordering: code may acquire Client.mu and then agentRegistry.mu (via a
// registry method); registry methods must NEVER call back into Client or
// acquire Client.mu. This avoids lock inversion.
//
// The registry stores defensively-copied AgentInfo values so callers cannot
// mutate a registered descriptor (Args/Models slices) concurrently without
// going through the registry.
type agentRegistry struct {
	mu     sync.RWMutex
	agents map[string]AgentInfo
}

func newAgentRegistry() *agentRegistry {
	return &agentRegistry{agents: make(map[string]AgentInfo)}
}

// register adds or replaces the agent descriptor for the given ID. The input
// slices (Args, Models) are defensively copied so later mutation by the caller
// cannot change the stored value.
func (r *agentRegistry) register(agent AgentInfo) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.agents[agent.ID] = cloneAgentInfo(agent)
}

// remove deletes the agent with the given ID. Removing an unknown ID is a
// no-op (matches the prior Client.RemoveAgent behavior).
func (r *agentRegistry) remove(id string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	delete(r.agents, id)
}

// list returns a projection of every registered agent containing only the
// public fields (ID, Name, Models, Warning). It intentionally omits Command
// and Args — this is the existing ListAgents contract and must not change.
// The returned slice is safe to mutate; it does not alias internal state.
func (r *agentRegistry) list() []interfaces.AgentInfo {
	r.mu.RLock()
	defer r.mu.RUnlock()
	out := make([]interfaces.AgentInfo, 0, len(r.agents))
	for _, a := range r.agents {
		models := make([]interfaces.AgentModel, 0, len(a.Models))
		for _, m := range a.Models {
			models = append(models, interfaces.AgentModel{ID: m.ID, Name: m.Name})
		}
		out = append(out, interfaces.AgentInfo{
			ID:      a.ID,
			Name:    a.Name,
			Models:  models,
			Warning: a.Warning,
		})
	}
	return out
}

// get returns a defensively-copied snapshot of the agent descriptor for the
// given ID, including Command and Args. Returns ok==false when the ID is not
// registered. The returned value is safe for the caller to retain beyond the
// registry lock; mutating it does not affect the stored entry.
func (r *agentRegistry) get(id string) (AgentInfo, bool) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	a, ok := r.agents[id]
	if !ok {
		return AgentInfo{}, false
	}
	return cloneAgentInfo(a), true
}

// resolve validates that agentID is registered and that modelID is offered by
// that agent, returning a copied snapshot of the resolved AgentInfo. Error
// strings are preserved byte-for-byte to match the prior
// validateAgentModelLocked behavior:
//   "agent not found: <id>"
//   "model <model> not available for agent <agent>"
func (r *agentRegistry) resolve(agentID, modelID string) (AgentInfo, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	agent, ok := r.agents[agentID]
	if !ok {
		return AgentInfo{}, fmt.Errorf("agent not found: %s", agentID)
	}
	modelValid := false
	for _, m := range agent.Models {
		if m.ID == modelID {
			modelValid = true
			break
		}
	}
	if !modelValid {
		return AgentInfo{}, fmt.Errorf("model %s not available for agent %s", modelID, agentID)
	}
	return cloneAgentInfo(agent), nil
}

// cloneAgentInfo returns a deep copy of a so callers cannot mutate the Args or
// Models slices and affect the stored value. Used on both register (ingress)
// and get/resolve (egress).
func cloneAgentInfo(a AgentInfo) AgentInfo {
	out := a
	if a.Args != nil {
		out.Args = append([]string(nil), a.Args...)
	}
	if a.Models != nil {
		out.Models = make([]interfaces.AgentModel, len(a.Models))
		copy(out.Models, a.Models)
	}
	return out
}
