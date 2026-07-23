# Rebind leaves the profile capability cache stale

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `/media/adam/extex/projects/project-hail-larry/src/acp/core.rs`
- **Lines:** 855-865

## Description

When a session is rebound, the replacement actor's startup data refreshes `caps`, `model_config_id`, and `acp_session_id`, but not `profile_config_id`. The session entry therefore retains the old agent's profile option ID or old lack of support. After a rebind, a profile change can be skipped even though the replacement agent supports mode configuration, or it can send `session/set_config_option` with an option ID advertised only by the previous agent.

## Recommendation

Assign `entry.profile_config_id = startup.profile_config_id` alongside `model_config_id`, and add rebind coverage where the old and replacement agents advertise different profile capability IDs/support.

## Verification

`ActorStartup` contains `profile_config_id` at lines 1202-1204, and initial registration stores it at line 432. The rebind update block at lines 855-865 consumes the other startup fields but has no assignment for `profile_config_id`; `session_for_profile_switch` later reads the unchanged entry field at lines 478-484.
