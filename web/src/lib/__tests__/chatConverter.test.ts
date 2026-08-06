import { describe, it, expect } from 'vitest'
import { eventsToMessages, stopReasonLabel } from '@/lib/chatConverter'
import type { AppEvent, StopReason } from '@/types'

/**
 * Tests for `stopReasonLabel`: a pure mapping from ACP stop reasons to
 * human-readable labels. Returns null for normal/empty ends, a friendly
 * label for known reasons, and the raw (trimmed) string for unknown ones.
 *
 * Inputs that are not part of the `StopReason` union (whitespace, unknown
 * reasons, padded strings) are cast to exercise the function's defensive
 * trimming and passthrough behavior — the runtime accepts any string.
 */
describe('eventsToMessages tool-call IDs', () => {
  const convert = (events: AppEvent[]) => eventsToMessages(events, [], new Map())
  const toolParts = (message: ReturnType<typeof eventsToMessages>[number]) =>
    Array.isArray(message.content)
      ? message.content.filter((part) => part.type === 'tool-call')
      : []

  it('replaces a started tool with its consecutive permission request', () => {
    const messages = eventsToMessages(
      [
        {
          id: 1,
          type: 'ToolStarted',
          sessionId: 'session-1',
          toolCallId: 'call-1',
          tool: 'Edit file',
          toolKind: 'edit',
        },
        {
          id: 2,
          type: 'PermissionRequested',
          sessionId: 'session-1',
          toolCallId: 'call-1',
          requestId: 'permission-1',
          tool: 'Edit file',
          toolKind: 'edit',
        },
      ],
      [
        {
          id: 'permission-1',
          sessionId: 'session-1',
          tool: 'Edit file',
          options: ['allow_once', 'deny'],
        },
      ],
      new Map(),
    )

    expect(messages).toHaveLength(1)
    expect(messages[0].status?.type).toBe('requires-action')
    expect(toolParts(messages[0])).toHaveLength(1)
    expect(toolParts(messages[0])[0]).toMatchObject({
      toolCallId: 'call-1',
      toolName: 'permission',
      approval: { id: 'permission-1', approved: undefined },
    })
  })

  it('replaces consecutive lifecycle completion for the same tool call', () => {
    const messages = convert([
      {
        id: 1,
        type: 'ToolStarted',
        sessionId: 'session-1',
        toolCallId: 'call-1',
        tool: 'Run tests',
      },
      {
        id: 2,
        type: 'ToolCompleted',
        sessionId: 'session-1',
        toolCallId: 'call-1',
        tool: 'Run tests',
        summary: 'completed',
        content: 'passed',
      },
    ])

    expect(messages).toHaveLength(1)
    expect(toolParts(messages[0])).toEqual([
      expect.objectContaining({ toolCallId: 'call-1', result: 'passed' }),
    ])
  })

  it('suffixes a nonconsecutive replay but merges its following completion', () => {
    const messages = convert([
      {
        id: 1,
        type: 'ToolStarted',
        sessionId: 'session-1',
        toolCallId: 'call-1',
        tool: 'Run tests',
      },
      {
        id: 2,
        type: 'StreamUpdate',
        sessionId: 'session-1',
        content: 'Later replay',
        streaming: false,
      },
      {
        id: 3,
        type: 'ToolStarted',
        sessionId: 'session-1',
        toolCallId: 'call-1',
        tool: 'Run tests',
      },
      {
        id: 4,
        type: 'ToolCompleted',
        sessionId: 'session-1',
        toolCallId: 'call-1',
        tool: 'Run tests',
        summary: 'completed',
        content: 'replayed result',
      },
    ])

    expect(messages).toHaveLength(3)
    const original = toolParts(messages[0])[0] as { toolCallId: string }
    const replay = toolParts(messages[2])[0] as { toolCallId: string; result?: string }
    expect(original.toolCallId).toBe('call-1')
    expect(replay.toolCallId).toMatch(/^call-1#evt-3-/)
    expect(replay.toolCallId).not.toBe(original.toolCallId)
    expect(replay.result).toBe('replayed result')
    expect(toolParts(messages[2])).toHaveLength(1)
  })
})

describe('stopReasonLabel', () => {
  it('returns null for undefined input', () => {
    expect(stopReasonLabel(undefined)).toBeNull()
  })

  it('returns null for an empty string', () => {
    expect(stopReasonLabel('' as StopReason)).toBeNull()
  })

  it('returns null for a whitespace-only string', () => {
    expect(stopReasonLabel('  ' as StopReason)).toBeNull()
  })

  it('returns null for "end_turn" (normal completion)', () => {
    expect(stopReasonLabel('end_turn')).toBeNull()
  })

  it('maps "max_tokens" to "hit token limit"', () => {
    expect(stopReasonLabel('max_tokens')).toBe('hit token limit')
  })

  it('maps "max_turn_requests" to "hit turn-request limit"', () => {
    expect(stopReasonLabel('max_turn_requests')).toBe('hit turn-request limit')
  })

  it('maps "refusal" to "refused"', () => {
    expect(stopReasonLabel('refusal')).toBe('refused')
  })

  it('maps "cancelled" to "cancelled"', () => {
    expect(stopReasonLabel('cancelled')).toBe('cancelled')
  })

  it('passes unknown reasons through as the raw string', () => {
    expect(stopReasonLabel('unknown_reason' as StopReason)).toBe('unknown_reason')
  })

  it('trims input before matching, so padded "end_turn" returns null', () => {
    expect(stopReasonLabel('  end_turn  ' as StopReason)).toBeNull()
  })
})
