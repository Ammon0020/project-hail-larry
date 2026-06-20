---
name: golang-linting
description: Run golangci-lint to lint and verify Go code (cross-platform)
---


# Run golangci-lint

Use this skill when asked to lint, verify, or check Go code quality.

## Prerequisites

- `golangci-lint` must be installed and on `PATH`.
- The repository root contains a `.golangci.yml` (v2 format) with the project's linter and formatter configuration.
- Run from the repository root so the config is picked up automatically.

## Run the linter

### Preferred: Makefile target

```sh
make lint
```

This runs `golangci-lint run` with no extra flags — the `.golangci.yml` config controls which linters and formatters are enabled.

### Direct invocation

If `make` is unavailable or you need flags:

```sh
golangci-lint run
```

Common flags (optional):

- `--fix` — auto-fix formatting issues (gofmt) where possible.
- `--timeout 10m` — override the default 5-minute timeout (also set in `.golangci.yml`).
- `--verbose` — show per-file processing output.

## Interpreting output

- Exit code `0` — clean, no issues.
- Exit code `1` — issues found. The summary at the bottom shows counts per linter (e.g. `errcheck: 37`, `revive: 30`).
- Each issue line format: `file:line:col: message (linter)`.

## Fixing issues

1. Address each issue at the root cause — don't suppress without reason.
2. Re-run `make lint` (or `golangci-lint run`) after fixes.
3. Repeat until the output is clean (exit code `0`).

## Suppressing rules

Only disable lint rules with a clear justification. Two approaches:

- **Inline** — add a `//nolint:<linter> // <reason>` comment on the offending line.
- **Config** — add an `exclude-rules` entry in `.golangci.yml` under the `issues` section.

## Troubleshooting

- **`command not found`** — install `golangci-lint`:
  - Windows: `winget install golangci-lint` or download from [releases](https://github.com/golangci/golangci-lint/releases).
  - Linux/macOS: `brew install golangci-lint` or use the [install script](https://golangci-lint.run/usage/install/).
- **`can't load config`** — ensure `.golangci.yml` is valid YAML and matches the installed major version (v2 uses `version: "2"`).
- **Timeout errors** — increase `run.timeout` in `.golangci.yml` or pass `--timeout`.
- **Linting `node_modules` or generated files** — these are excluded via `issues.exclude-rules` in `.golangci.yml`; verify the path patterns if new generated directories are added.
