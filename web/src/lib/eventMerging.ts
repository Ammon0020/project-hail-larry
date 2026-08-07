import type { AppEvent } from '@/types'

/**
 * Index of the most recent event of `type` that a follow-up event belongs to.
 *
 * `strict` controls what happens when the started card has no `toolCallId`:
 * shell *output* only ever attaches to a card whose id matches, while a
 * *completion* also adopts an id-less card. That difference is preserved from
 * the original inline reducer — agents that omit `toolCallId` on the started
 * event still need their completion to land somewhere.
 */
function findStartedIndex(
  acc: AppEvent[],
  type: string,
  toolCallId: string | undefined,
  strict: boolean,
): number {
  for (let i = acc.length - 1; i >= 0; i--) {
    const candidate = acc[i]
    if (candidate.type !== type) continue
    if (
      !toolCallId ||
      candidate.toolCallId === toolCallId ||
      (!strict && !candidate.toolCallId)
    ) {
      return i
    }
  }
  return -1
}

/**
 * Collapse a raw event stream into the cards the chat actually renders.
 *
 * Three foldings happen here:
 * - consecutive `StreamUpdate` chunks of the same role/thought become one
 *   growing message, so streamed text reads as a single response;
 * - `ShellOutputStreamed` chunks append onto their running
 *   `ShellCommandStarted` card;
 * - `ShellCommandCompleted` / `ToolCompleted` replace their started card in
 *   place, keeping the original event id so React keys stay stable, and
 *   carrying over fields the completion event does not send.
 *
 * Pure and non-mutating: the input array and its events are left untouched.
 */
export function mergeChatEvents(events: AppEvent[]): AppEvent[] {
  return events.reduce((acc: AppEvent[], event: AppEvent) => {
    if (event.type === 'StreamUpdate') {
      const last = acc[acc.length - 1]
      if (
        last &&
        last.type === 'StreamUpdate' &&
        last.role === event.role &&
        !!last.thought === !!event.thought
      ) {
        acc[acc.length - 1] = {
          ...last,
          content: (last.content || '') + (event.content || ''),
          streaming: event.streaming,
        }
        return acc
      }
    }

    // Live shell stdout/stderr: append onto the running Started card.
    if (event.type === 'ShellOutputStreamed') {
      const startedIdx = findStartedIndex(acc, 'ShellCommandStarted', event.toolCallId, true)
      if (startedIdx !== -1) {
        const started = acc[startedIdx]
        acc[startedIdx] = {
          ...started,
          content: (started.content || '') + (event.content || ''),
        }
      }
      // Orphan chunk (no matching Started) — drop, matching prior UI behavior.
      return acc
    }

    // Completed replaces Started so exit code + streamed output share one card.
    if (event.type === 'ShellCommandCompleted') {
      const startedIdx = findStartedIndex(acc, 'ShellCommandStarted', event.toolCallId, false)
      if (startedIdx !== -1) {
        const started = acc[startedIdx]
        acc[startedIdx] = {
          ...event,
          id: started.id, // Preserve original event ID for stable React keys
          content: started.content || event.content || event.summary,
        }
        return acc
      }
    }

    // ToolCompleted replaces ToolStarted so the tool card transitions from
    // running to complete in place. ToolCompleted doesn't carry
    // `command`/`tool`/`target`, so preserve them from the Started event to
    // keep the completed card's args/label intact.
    if (event.type === 'ToolCompleted') {
      const startedIdx = findStartedIndex(acc, 'ToolStarted', event.toolCallId, false)
      if (startedIdx !== -1) {
        const started = acc[startedIdx]
        acc[startedIdx] = {
          ...event,
          id: started.id, // Preserve original event ID for stable React keys
          toolCallId: event.toolCallId ?? started.toolCallId,
          command: event.command ?? started.command,
          tool: event.tool ?? started.tool,
          target: event.target ?? started.target,
          toolKind: event.toolKind ?? started.toolKind,
        }
        return acc
      }
    }

    acc.push(event)
    return acc
  }, [])
}
