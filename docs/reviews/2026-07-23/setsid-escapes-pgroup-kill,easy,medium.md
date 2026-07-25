# setsid / nohup escape process-group kill (Unix) — persistent process survives session teardown

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `src/shell/mod.rs`
- **Lines:** 310-326 (cancel kill); `src/procutil/mod.rs:79-85, 95-112`

## Description

Containment relies on `setpgid(0,0)` putting the child in a dedicated group, then `kill(-pgid, SIGKILL)` on cancel (procutil/mod.rs:79-85). This works for ordinary `sh -c "sleep 30 &"` grandchildren (the test at procutil/mod.rs:131-177 confirms it). However the agent can invoke `setsid` (or `nohup` + `disown`, or call `setpgid`/`setsid` via a tiny C shim) to create a **new session**, detaching the grandchild from the daemon's process group. `kill(-pgid, SIGKILL)` then misses the detached process, which is reparented to init and keeps running with full access to workspace files (and the daemon user's privileges) after the session closes. Because `create_terminal` uses `run_async_args` (direct exec), the agent can set `command="setsid", args=["-f", "sleep", "1000000"]` directly. This is a persistent-process escape that survives session teardown.

## Recommendation

This is hard to fully prevent without a PID namespace / cgroup / Job Object. Mitigations: (a) on Linux, run agent commands in a cgroup with `cgroup.kill` on session close; (b) reject `setsid`/`nohup`/`disown` in an argv allowlist (fragile); (c) document the limitation explicitly in the threat model. At minimum, scan the resolved binary path against a denylist of session-detaching helpers.

## Verification

procutil/mod.rs:103-110 — `pre_exec` calls `setpgid(0,0)` once for the direct child only; grandchildren that call `setsid()` themselves move to a new session. procutil/mod.rs:83 — `kill(-pgid, SIGKILL)` targets only the original group. No `prctl(PR_SET_PDEATHSIG)` / cgroup / pidns is configured, so a detached child is not killed when the daemon exits either.
