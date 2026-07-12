#!/usr/bin/env python3
"""PreToolUse hook for exec: blocks dangerous shell commands.

Reads the Devin hook event JSON from stdin. If the command matches a
dangerous pattern, exits with code 2 (block) and prints a reason on stderr.
Otherwise exits 0 (allow). All decisions are logged to
.devin/logs/exec-hook.log relative to DEVIN_PROJECT_DIR (or cwd).

Dangerous patterns are matched as substrings (case-insensitive) against the
raw command string. This is intentionally conservative — false positives that
block a legitimately dangerous command are preferable to allowing one through
in a daemon that executes shell on behalf of AI agents.
"""
from __future__ import annotations

import json
import os
import sys
from datetime import datetime
from pathlib import Path

# Substrings that indicate a destructive or escape-prone command.
# Keep entries specific enough to avoid blocking normal dev workflows.
DANGEROUS_SUBSTRINGS = [
    "rm -rf /",
    "rm -rf ~",
    "rm -rf $HOME",
    "rm -rf /*",
    "rm -fr /",
    "mkfs.",
    "dd if=/dev/zero of=/dev/sd",
    "dd if=/dev/zero of=/dev/nvme",
    ":(){ :|:& };:",          # fork bomb
"> /dev/sda",
    "chmod -R 000 /",
    "chown -R root /",
    "git push --force",
    "git push -f",
    "git push --force-with-lease --force",  # still force; covered above
    "curl ... | sh",
    "curl ... | bash",
    "wget ... | sh",
    "wget ... | bash",
    "shutdown",
    "reboot",
    "halt -p",
    "init 0",
]

# Commands that pipe untrusted remote content straight to a shell.
REMOTE_PIPE_TO_SHELL = [
    "| sh",
    "| bash",
    "| zsh",
    "| python",
    "| python3",
]


def log(message: str) -> None:
    project_dir = os.environ.get("DEVIN_PROJECT_DIR", os.getcwd())
    log_dir = Path(project_dir) / ".devin" / "logs"
    try:
        log_dir.mkdir(parents=True, exist_ok=True)
        ts = datetime.now().isoformat(timespec="seconds")
        (log_dir / "exec-hook.log").open("a").write(f"{ts} {message}\n")
    except OSError:
        # Logging is best-effort; never let it mask the real decision.
        pass


def main() -> int:
    raw = sys.stdin.read()
    try:
        data = json.loads(raw)
    except json.JSONDecodeError:
        log("ERROR could not parse stdin as JSON")
        return 0  # fail open on malformed input; the permission system still prompts

    tool_input = data.get("tool_input") or {}
    command = str(tool_input.get("command", ""))
    if not command:
        return 0

    lowered = command.lower()

    for pattern in DANGEROUS_SUBSTRINGS:
        if pattern.lower() in lowered:
            reason = f"BLOCKED dangerous pattern '{pattern}' in command: {command}"
            log(reason)
            print(reason, file=sys.stderr)
            return 2

    for pattern in REMOTE_PIPE_TO_SHELL:
        if pattern in command:
            reason = f"BLOCKED remote-to-shell pipe '{pattern}' in command: {command}"
            log(reason)
            print(reason, file=sys.stderr)
            return 2

    log(f"ALLOW command: {command}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
