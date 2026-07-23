# Missing test for prompt-injection fallback when mode capability is absent

- **Difficulty:** medium
- **Urgency:** medium
- **File:** `docs/plans/profiles-over-acp/done-acp-set-config-option-send-hard.md`
- **Lines:** 49-52

## Description

Acceptance criterion #2 (line 49) is marked `[x]` done:

> Agent WITHOUT the capability falls back to prompt injection; instructions
> still applied (fallback branch is the `profile_config_id == None` path;
> `MOCKAGENT_NO_MODE_CAP=1` fallback test deferred — env wiring per-test is
> not in the existing harness, noted in `docs/known-issues.md` if needed).

However, the fallback path is **not tested**. The mock agent supports
`MOCKAGENT_NO_MODE_CAP=1` (cmd/mockagent/main.go:37,129) to suppress the
mode/profile config option advertisement, but no test in
`tests/acp_core_lifecycle.rs` sets this env var. The existing
`mockagent_initial_profile_sent_over_acp_when_capability_advertised` test
only covers the capability-present branch.

The "noted in `docs/known-issues.md` if needed" qualifier is misleading —
the deferral is **not** actually recorded in `docs/known-issues.md`
(verified by grep). Marking an acceptance criterion `[x]` done while the
test is deferred (and unrecorded) makes the plan's acceptance criteria
dishonest, which violates AGENTS.md's "Keep `docs/STATUS.md` honest and
current" spirit.

## Recommendation

Either:

1. **Add the test:** Start the mockagent subprocess with
   `MOCKAGENT_NO_MODE_CAP=1` in a new test, send a prompt, and assert the
   reply does NOT start with `[profile: code]` (proving the
   `set_config_option` was not sent) but the profile instructions are
   still injected (visible in the prompt payload or agent behavior). This
   requires per-test env var wiring in the harness, which the story notes
   is not currently supported.

2. **Or record the gap honestly:** Add an entry to
   `docs/known-issues.md` documenting the missing fallback test, and
   change the criterion checkbox from `[x]` to `[ ]` (or add a partial
   note like `[~]`) so the plan accurately reflects that the fallback
   branch is untested.

## Verification

- `grep -rn "MOCKAGENT_NO_MODE_CAP" tests/ src/` returns no matches — no
  test exercises the no-capability path.
- `grep -n "MOCKAGENT_NO_MODE_CAP\|no.mode.cap\|S-PROF-ACP"
  docs/known-issues.md` returns no matches — the deferral is not
  recorded.
- `cmd/mockagent/main.go:37` defines `envNoModeCap = "MOCKAGENT_NO_MODE_CAP"`
  and line 129-131 shows it suppresses the config option advertisement.
- `src/acp/providers.rs:309` `find_profile_config_id` returns `None` when
  the option is absent, triggering the fallback branch.
