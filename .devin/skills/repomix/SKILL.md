---
name: repomix
description: Pack the codebase (or a subdirectory) into a single AI-friendly file for full-context analysis, handoffs, or external LLM feeds. Use when you need whole-codebase context, are preparing a briefing for an external agent, or want to avoid dozens of read/grep round-trips.
argument-hint: "[optional path or --include glob]"
allowed-tools:
  - read
  - exec
  - grep
  - glob
  - todo_write
permissions:
  allow:
    - Exec(npx repomix*)
    - Exec(repomix*)
    - Read(**)
---

# Repomix — pack the repo for AI consumption

A single packed read replaces many `read`/`grep`/`code_search` round-trips, saving tool-call overhead and tokens. Best for whole-codebase questions, cross-cutting refactors, architecture reviews, and handoffs to external agents.

## Use when

- Whole-codebase or cross-module questions.
- Preparing a scoped briefing for an external agent / LLM web session.
- You'd otherwise read 15+ files to answer one question — pack instead.

Skip for targeted lookups (one function/file) — just `read`/`grep`.

## Commands

```bash
npx repomix@latest                    # whole repo (uses repomix.config.json)
npx repomix@latest src/acp            # subdirectory
npx repomix@latest --include "src/**/*.rs,docs/plans/**/*.md"
npx repomix@latest --compress         # ~70% token cut; drops bodies, keeps signatures
npx repomix@latest --split-output 1mb # chunk for file-size-limited tools
git ls-files | npx repomix@latest --stdin
```

Default output `repomix-output.xml` is gitignored and **unreadable by Devin's `read` tool** (gitignore is a sandbox boundary). For Devin consumption, write outside the repo:

```bash
npx repomix@latest src/acp --compress --output /tmp/repomix-out.xml
```

Then `read /tmp/repomix-out.xml`. Check the printed token count before feeding it anywhere.

## Workflow

1. Pick the narrowest scope that answers the question.
2. Run, note the token count, read the output in one pass.
3. Answer or hand off; cite real file paths from the pack.

## Notes

- `--compress` is great for architecture/Q&A but drops implementation detail — don't use it when exact line contents matter.
- Config has Secretlint on; flagged secret-bearing files are skipped. Don't disable.
- If `npx`/Node < 22 is unavailable, tell the user.
