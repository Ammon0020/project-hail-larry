# Fuzzy boundary between primary-a and primary-b for code review

- **Difficulty:** easy
- **Urgency:** low
- **File:** `.devin/agents/primary-b/AGENT.md`
- **Lines:** 3

## Description

primary-a's description (line 3) says 'Major feature development and code review. Prefer this for development.' and primary-b's says 'Planning help, complex problem solving, and code review of complex code or large problems.' Both claim code review, split only by the subjective qualifier 'complex code or large problems'. Since primary-a is also marked 'Prefer this for development', a delegator facing a moderately complex review has no objective rule for choosing between the two tiers, which can lead to over-using the more expensive primary-b (claude-opus) for reviews primary-a could handle, or under-using it for genuinely complex reviews.

Note: `git diff .devin/agents/` shows no uncommitted changes — these files are already committed.

## Recommendation

Add an objective trigger to primary-b's description, e.g. 'code review of changes spanning multiple packages/subsystems or touching concurrency, security, or merge logic — otherwise prefer primary-a for review.'

## Verification

Read both description lines; both contain 'code review'. The only differentiator is the undefined term 'complex code or large problems', confirmed against AGENTS.md which gives no definition of 'complex'/'large'.
