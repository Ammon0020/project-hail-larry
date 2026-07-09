# Overlapping 'codebase exploration' responsibility between small and trivial

- **Difficulty:** easy
- **Urgency:** low
- **File:** `.devin/agents/small/AGENT.md`
- **Lines:** 3

## Description

small's description (line 3) lists 'large codebase exploration' while trivial's description (line 3) lists 'general codebase exploration'. AGENTS.md's Subagents section says each description states which tier/task type it is suited for so the delegator can 'match subtasks to the right tier'. With both tiers claiming exploration, the delegator has no clear signal for which to pick for an exploration-only task, risking inconsistent delegation (e.g., sending large exploration to the cheaper trivial model, or vice versa).

Note: `git diff .devin/agents/` shows no uncommitted changes — these files are already committed.

## Recommendation

Differentiate explicitly: e.g., trivial = 'small, targeted lookups and quick orientation in the codebase'; small = 'broader codebase exploration and reading across packages to summarize structure'. Make the size/scope boundary mirror the tier ordering.

## Verification

Read both description lines. Both contain the phrase 'codebase exploration' with only the adjective ('large' vs 'general') differing, which does not establish a clear tier boundary.
