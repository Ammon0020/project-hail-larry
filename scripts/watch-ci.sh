#!/usr/bin/env bash
#
# Watch the latest GitHub Actions run for the current branch and send a desktop
# notification when it finishes. Non-blocking: run it in the background after
# pushing, e.g. `./scripts/watch-ci.sh &`.
#
# Usage:
#   ./scripts/watch-ci.sh              # watch the latest run on the current branch
#   ./scripts/watch-ci.sh <run-id>     # watch a specific run
#   ./scripts/watch-ci.sh --foreground # block the terminal until the run finishes
#
# Requires: gh (authenticated), notify-send (Linux) or osascript (macOS).
# Exits 0 if the run succeeded, 1 if it failed or was cancelled.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

FOREGROUND=0
RUN_ID=""
if [[ "${1:-}" == "--foreground" ]]; then
    FOREGROUND=1
    shift
fi
if [[ $# -ge 1 ]]; then
    RUN_ID="$1"
fi

# Resolve the run to watch: explicit arg, else the latest run for the current
# branch. We query by branch so the script targets the run triggered by the
# push you just made, not a stray run on another branch.
if [[ -z "$RUN_ID" ]]; then
    BRANCH="$(git rev-parse --abbrev-ref HEAD)"
    RUN_ID="$(gh run list --branch "$BRANCH" --limit 1 --json databaseId --jq '.[0].databaseId')" || {
        echo "ERROR: could not find a CI run for branch '$BRANCH'." >&2
        exit 1
    }
fi

if [[ -z "$RUN_ID" ]]; then
    echo "ERROR: no CI run found for branch '$BRANCH'." >&2
    exit 1
fi

echo "Watching run $RUN_ID ($(gh run view "$RUN_ID" --json name,headBranch --jq '.name + " on " + .headBranch'))..."

# `gh run watch` blocks until the run completes and exits non-zero on failure.
# We capture the exit code so the notification reflects the outcome.
set +e
gh run watch "$RUN_ID" --exit-status > /dev/null 2>&1
WATCH_EXIT=$?
set -e

# Fetch the final conclusion for the notification body.
CONCLUSION="$(gh run view "$RUN_ID" --json conclusion --jq '.conclusion' 2>/dev/null || echo "unknown")"
RUN_URL="$(gh run view "$RUN_ID" --json url --jq '.url' 2>/dev/null || echo "")"

# Send a desktop notification if a notifier is available. Falls back to a
# terminal bell + stderr line on headless systems.
send_notification() {
    local title="$1" body="$2"
    if command -v notify-send >/dev/null 2>&1; then
        notify-send --app-name="CI" --icon="$([ "$WATCH_EXIT" -eq 0 ] && echo dialog-information || echo dialog-error)" \
            "$title" "$body"
    elif command -v osascript >/dev/null 2>&1; then
        osascript -e "display notification \"$body\" with title \"$title\""
    else
        echo -e '\a' # terminal bell
        echo "$title: $body" >&2
    fi
}

if [[ "$WATCH_EXIT" -eq 0 ]]; then
    send_notification "CI passed" "Run $RUN_ID succeeded ($CONCLUSION)."
    echo "CI passed: run $RUN_ID ($CONCLUSION). $RUN_URL"
else
    send_notification "CI failed" "Run $RUN_ID finished with $CONCLUSION."
    echo "CI failed: run $RUN_ID ($CONCLUSION). $RUN_URL"
fi

exit "$WATCH_EXIT"
