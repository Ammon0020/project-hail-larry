# Harness session-history capability matrix (S-HIST-PROBE)

> **Last updated:** 2026-07-18.
> **Runtime source of truth:** live ACP `initialize` →
> `GET /api/sessions/{id}/capabilities` (auth-gated; live agent only).
> **Cold-start:** epic Q8 still open — do **not** spawn an agent solely to
> fill this matrix or the API; dormant sessions return `available: false`.

Cells are either:

- **unknown / not probed** — we have not run `initialize` against that binary
  in this repo / CI.
- **reported (source, version, date)** — third-party live probe; **not** our
  verification. Treat as a hint until re-probed here.

Do **not** invent ✅.

## Registry agents (`KNOWN_AGENTS` in `src/acp/autodetect.rs`)

| Agent id | Harness | list | loadSession | resume | close | delete | Last verified | Probe method / notes |
|----------|---------|------|-------------|--------|-------|--------|---------------|----------------------|
| `claude-code` | Claude Code ACP (`claude`) | unknown / not probed | unknown / not probed | unknown / not probed | unknown / not probed | unknown / not probed | — | Registry command is `claude`. Flightdeck (2026-03-19) probed **`claude-agent-acp` v0.21.0** with list+resume+fork and loadSession — **different binary**; do not copy those cells here without a local probe of `claude`. |
| `codex` | Codex ACP (`codex-acp`) | reported (Flightdeck, `codex-acp` v0.9.5, 2026-03-19): yes | reported (Flightdeck): no | reported (Flightdeck): no | unknown / not probed | unknown / not probed | 2026-03-19 (external) | Flightdeck live probe: `sessionCapabilities.list` only; no loadSession / resume / fork. **Re-probe locally before gating product UX.** |
| `cursor` | Cursor CLI ACP (`agent`/`cursor-agent` + `acp`) | unknown / not probed | unknown / not probed | unknown / not probed | unknown / not probed | unknown / not probed | — | Flightdeck: Cursor binary **not installed** — not probed. |
| `devin` | Devin ACP (`devin acp`) | unknown / not probed | unknown / not probed | unknown / not probed | unknown / not probed | unknown / not probed | — | No public probe found in Flightdeck matrix. |
| `mistral-vibe` | Mistral Vibe (`vibe-acp` / `vibe`) | unknown / not probed | unknown / not probed | unknown / not probed | unknown / not probed | unknown / not probed | — | No public probe found in Flightdeck matrix. |

## Mentioned in Blueprint / epic but **not** in registry

| Name | Typical launch | list | loadSession | resume | close | delete | Last verified | Notes |
|------|----------------|------|-------------|--------|-------|--------|---------------|-------|
| Gemini CLI ACP | `gemini --acp` (reported) | reported (Flightdeck v0.34.0, 2026-03-19): no | reported (Flightdeck): yes | reported (Flightdeck): no | unknown / not probed | unknown / not probed | 2026-03-19 (external) | Not in `KNOWN_AGENTS`. Session caps empty; loadSession only. |
| OpenCode | `opencode acp` (reported) | reported (Flightdeck v1.2.27, 2026-03-19): yes | unknown / not probed (Flightdeck summarized resume path; loadSession cell not separately asserted here) | reported (Flightdeck): yes (+ fork) | unknown / not probed | unknown / not probed | 2026-03-19 (external) | Not in `KNOWN_AGENTS`. Re-probe before relying on loadSession. |

## Runtime API shape (live projection)

`GET /api/sessions/{id}/capabilities` → JSON:

```json
{
  "available": true,
  "canListSessions": false,
  "canLoadSession": true,
  "canResumeSession": false,
  "canCloseSession": false,
  "canDeleteSession": false
}
```

| Field | Meaning |
|-------|---------|
| `available` | Caps came from a **live** `initialize`. `false` = dormant / not warm (Q8). |
| `canListSessions` | `sessionCapabilities.list` present |
| `canLoadSession` | `agentCapabilities.loadSession` |
| `canResumeSession` | `sessionCapabilities.resume` present |
| `canCloseSession` | `sessionCapabilities.close` present |
| `canDeleteSession` | `sessionCapabilities.delete` present |

**FALLBACK gate:** when `available && !canListSessions && !canLoadSession`, treat
as history-incapable for browse/open. When `!available`, do not assume either
way until Q8.

## How to re-probe locally

1. Start a session with the harness (warm agent).
2. `GET /api/sessions/{id}/capabilities` with a paired device token.
3. Update this table with the live booleans, binary version, and date — mark
   source as **this repo** (not Flightdeck).

Optional: use an external probe tool (e.g. `acp-probe` / Flightdeck
`query-acp-capabilities.ts`) and record command + version in Notes.
