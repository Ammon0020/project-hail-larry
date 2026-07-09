# trivial agent body contains ambiguous, self-referential instruction

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `.devin/agents/trivial/AGENT.md`
- **Lines:** 7

## Description

Line 7 says 'Return specific details and instructions and follow your instructions specifically. Include notes of details you found that should be noted.' The phrase 'follow your instructions specifically' is ambiguous — 'your' could refer to the trivial agent's own output or to the delegating agent's task instructions — and 'Include notes of details you found that should be noted' is tautological. This is the only agent with ad hoc behavioral prose, and its wording is unclear enough that a fast/cheap model could misinterpret it (e.g., re-issuing instructions to the caller, or fixating on 'notes' rather than completing the task).

Note: `git diff .devin/agents/` shows no uncommitted changes — these files are already committed. This finding applies to the current committed state.

## Recommendation

Rewrite to a clear, unambiguous directive, e.g.: 'Keep tasks small and focused. Return specific, structured findings (what you changed/inspected, exact file paths and line numbers, and anything notable you discovered) rather than a status summary. Follow the delegating task's instructions exactly.'

## Verification

Read trivial/AGENT.md line 7; compared to the other four agent bodies, which contain no equivalent ambiguous prose. Confirmed AGENTS.md's Subagents section already specifies 'expect structured findings/results back, not just a status update', so the wording is redundant as well as unclear.
