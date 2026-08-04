import { describe, it, expect } from 'vitest'
import { stopReasonLabel } from '@/lib/chatConverter'
import type { StopReason } from '@/types'

/**
 * Tests for `stopReasonLabel`: a pure mapping from ACP stop reasons to
 * human-readable labels. Returns null for normal/empty ends, a friendly
 * label for known reasons, and the raw (trimmed) string for unknown ones.
 *
 * Inputs that are not part of the `StopReason` union (whitespace, unknown
 * reasons, padded strings) are cast to exercise the function's defensive
 * trimming and passthrough behavior — the runtime accepts any string.
 */
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
