# small agent not directed to development rules

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `.devin/agents/small/AGENT.md`
- **Lines:** 7

## Description

The body reads only 'See AGENTS.md for instructions' — it is missing the 'and development rules.' clause that every other subagent includes (primary-a, primary-b, routine, trivial all say 'See AGENTS.md for instructions and development rules.'). AGENTS.md's Development Standards section (build.sh/build.ps1, go test/vet, npm run build, golangci-lint/eslint, STATUS.md updates) is the core ruleset for the project. Because the small tier is used for 'Small feature implementation and ... UI work', a small agent that skips development rules could merge code without running builds/tests/lint or updating STATUS.md, causing the exact drift AGENTS.md warns against.

Note: `git diff .devin/agents/` shows no uncommitted changes — these files are already committed. This finding applies to the current committed state.

## Recommendation

Change line 7 to 'See AGENTS.md for instructions and development rules.' to match the other four agents.

## Verification

Read all five AGENT.md files; compared body text. Only small/AGENT.md omits 'and development rules'. Confirmed AGENTS.md contains the Development Standards section that the phrase points to.
