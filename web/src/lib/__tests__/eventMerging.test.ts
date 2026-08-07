import { describe, it, expect } from 'vitest'
import { mergeChatEvents } from '../eventMerging'
import type { AppEvent } from '@/types'

/** Minimal event factory — only the fields the merger reads. */
function event(partial: Partial<AppEvent> & { type: AppEvent['type'] }): AppEvent {
  return { id: 0, sessionId: 's1', ...partial } as AppEvent
}

describe('mergeChatEvents', () => {
  it('folds consecutive StreamUpdate chunks into one growing message', () => {
    const merged = mergeChatEvents([
      event({ id: 1, type: 'StreamUpdate', role: 'agent', content: 'Hel', streaming: true }),
      event({ id: 2, type: 'StreamUpdate', role: 'agent', content: 'lo', streaming: false }),
    ])

    expect(merged).toHaveLength(1)
    expect(merged[0].content).toBe('Hello')
    // The final chunk's streaming flag wins, so the UI stops the caret.
    expect(merged[0].streaming).toBe(false)
  })

  it('does not merge stream chunks across a role or thought boundary', () => {
    const merged = mergeChatEvents([
      event({ id: 1, type: 'StreamUpdate', role: 'agent', content: 'a' }),
      event({ id: 2, type: 'StreamUpdate', role: 'user', content: 'b' }),
      event({ id: 3, type: 'StreamUpdate', role: 'user', content: 'c', thought: true }),
    ])

    expect(merged.map((e) => e.content)).toEqual(['a', 'b', 'c'])
  })

  it('appends streamed shell output onto its running command card', () => {
    const merged = mergeChatEvents([
      event({ id: 1, type: 'ShellCommandStarted', toolCallId: 't1', content: '' }),
      event({ id: 2, type: 'ShellOutputStreamed', toolCallId: 't1', content: 'line1\n' }),
      event({ id: 3, type: 'ShellOutputStreamed', toolCallId: 't1', content: 'line2\n' }),
    ])

    expect(merged).toHaveLength(1)
    expect(merged[0].content).toBe('line1\nline2\n')
  })

  it('drops shell output with no matching started card', () => {
    const merged = mergeChatEvents([
      event({ id: 1, type: 'ShellOutputStreamed', toolCallId: 'orphan', content: 'x' }),
    ])

    expect(merged).toEqual([])
  })

  it('replaces the started card on completion while keeping streamed output and key', () => {
    const merged = mergeChatEvents([
      event({ id: 7, type: 'ShellCommandStarted', toolCallId: 't1', content: '' }),
      event({ id: 8, type: 'ShellOutputStreamed', toolCallId: 't1', content: 'out' }),
      event({ id: 9, type: 'ShellCommandCompleted', toolCallId: 't1', summary: 'exit 0' }),
    ])

    expect(merged).toHaveLength(1)
    expect(merged[0].type).toBe('ShellCommandCompleted')
    // The original id is preserved so React keys stay stable across the swap.
    expect(merged[0].id).toBe(7)
    expect(merged[0].content).toBe('out')
  })

  it('carries started-only tool fields onto the completed card', () => {
    const merged = mergeChatEvents([
      event({
        id: 3,
        type: 'ToolStarted',
        toolCallId: 't9',
        tool: 'read_file',
        target: 'a.ts',
        toolKind: 'read',
      }),
      event({ id: 4, type: 'ToolCompleted', toolCallId: 't9' }),
    ])

    expect(merged).toHaveLength(1)
    expect(merged[0].type).toBe('ToolCompleted')
    expect(merged[0].id).toBe(3)
    // ToolCompleted carries none of these; without the carry-over the card
    // would lose its label and arguments on completion.
    expect(merged[0].tool).toBe('read_file')
    expect(merged[0].target).toBe('a.ts')
    expect(merged[0].toolKind).toBe('read')
  })

  it('keeps unrelated events in order and leaves an unmatched completion alone', () => {
    const merged = mergeChatEvents([
      event({ id: 1, type: 'PromptSubmitted', content: 'hi' }),
      event({ id: 2, type: 'ToolCompleted', toolCallId: 'nope' }),
    ])

    expect(merged.map((e) => e.type)).toEqual(['PromptSubmitted', 'ToolCompleted'])
  })

  it('does not mutate the input array', () => {
    const input = [
      event({ id: 1, type: 'StreamUpdate', role: 'agent', content: 'a' }),
      event({ id: 2, type: 'StreamUpdate', role: 'agent', content: 'b' }),
    ]
    const snapshot = JSON.stringify(input)

    mergeChatEvents(input)

    expect(JSON.stringify(input)).toBe(snapshot)
  })
})
