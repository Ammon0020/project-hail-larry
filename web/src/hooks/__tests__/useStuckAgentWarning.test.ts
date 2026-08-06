import { describe, it, expect } from 'vitest'
import {
  DEFAULT_STUCK_TIMEOUT_MS,
  elapsedSince,
  isStuck,
  stuckSeconds,
} from '@/hooks/useStuckAgentWarning'

describe('useStuckAgentWarning pure logic', () => {
  describe('DEFAULT_STUCK_TIMEOUT_MS', () => {
    it('is 90 seconds (conservative, avoids false positives during tools)', () => {
      expect(DEFAULT_STUCK_TIMEOUT_MS).toBe(90_000)
    })
  })

  describe('elapsedSince', () => {
    it('returns null when there is no activity baseline', () => {
      expect(elapsedSince(null, 1000)).toBeNull()
    })

    it('returns the positive delta between now and the baseline', () => {
      expect(elapsedSince(1000, 5000)).toBe(4000)
    })

    it('clamps to zero when now precedes the baseline (clock skew)', () => {
      expect(elapsedSince(5000, 1000)).toBe(0)
    })
  })

  describe('isStuck', () => {
    it('is false when there is no baseline', () => {
      expect(isStuck(null, 90_000)).toBe(false)
    })

    it('is false before the timeout elapses', () => {
      expect(isStuck(89_999, 90_000)).toBe(false)
    })

    it('is true once the timeout elapses', () => {
      expect(isStuck(90_000, 90_000)).toBe(true)
      expect(isStuck(120_000, 90_000)).toBe(true)
    })
  })

  describe('stuckSeconds', () => {
    it('is zero when there is no baseline', () => {
      expect(stuckSeconds(null)).toBe(0)
    })

    it('floors elapsed ms to whole seconds', () => {
      expect(stuckSeconds(90_500)).toBe(90)
      expect(stuckSeconds(59_999)).toBe(59)
    })
  })
})
