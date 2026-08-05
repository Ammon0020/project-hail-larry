import { describe, it, expect } from 'vitest'
import {
  type ContextUsage,
  contextFillFraction,
  contextFillPercent,
  formatTokens,
  formatCost,
} from '@/lib/contextUsage'

describe('contextFillFraction', () => {
  it('returns 0 when usage is null', () => {
    expect(contextFillFraction(null)).toBe(0)
  })

  it('returns 0 when size is 0 (avoids division by zero)', () => {
    const usage: ContextUsage = { used: 100, size: 0 }
    expect(contextFillFraction(usage)).toBe(0)
  })

  it('returns 0 when size is negative', () => {
    const usage: ContextUsage = { used: 100, size: -5 }
    expect(contextFillFraction(usage)).toBe(0)
  })

  it('computes the fraction for partial fill', () => {
    const usage: ContextUsage = { used: 53_000, size: 200_000 }
    expect(contextFillFraction(usage)).toBeCloseTo(0.265, 5)
  })

  it('clamps at 1 when used exceeds size', () => {
    const usage: ContextUsage = { used: 250_000, size: 200_000 }
    expect(contextFillFraction(usage)).toBe(1)
  })

  it('returns 1 when exactly full', () => {
    const usage: ContextUsage = { used: 200_000, size: 200_000 }
    expect(contextFillFraction(usage)).toBe(1)
  })
})

describe('contextFillPercent', () => {
  it('returns "0%" when usage is null', () => {
    expect(contextFillPercent(null)).toBe('0%')
  })

  it('rounds to nearest integer', () => {
    const usage: ContextUsage = { used: 53_000, size: 200_000 }
    // 26.5% rounds to 27%
    expect(contextFillPercent(usage)).toBe('27%')
  })

  it('returns "100%" when full', () => {
    const usage: ContextUsage = { used: 200_000, size: 200_000 }
    expect(contextFillPercent(usage)).toBe('100%')
  })
})

describe('formatTokens', () => {
  it('formats thousands without decimal when whole', () => {
    expect(formatTokens(53_000)).toBe('53k')
  })

  it('formats thousands with one decimal when fractional', () => {
    expect(formatTokens(12_500)).toBe('12.5k')
  })

  it('formats raw number below 1000', () => {
    expect(formatTokens(500)).toBe('500')
  })

  it('formats exactly 1000 as 1k', () => {
    expect(formatTokens(1000)).toBe('1k')
  })
})

describe('formatCost', () => {
  it('returns empty string when amount is undefined', () => {
    expect(formatCost(undefined, 'USD')).toBe('')
  })

  it('returns empty string when amount is null', () => {
    expect(formatCost(null as unknown as undefined, 'USD')).toBe('')
  })

  it('formats USD with dollar symbol', () => {
    // 0.045.toFixed(2) → "0.04" (banker's rounding / float repr)
    expect(formatCost(0.045, 'USD')).toBe('$0.04')
  })

  it('formats EUR with euro symbol', () => {
    expect(formatCost(1.5, 'EUR')).toBe('€1.50')
  })

  it('falls back to currency code for unknown symbols', () => {
    expect(formatCost(2.0, 'CAD')).toBe('2.00 CAD')
  })

  it('handles missing currency code', () => {
    expect(formatCost(2.0)).toBe('2.00')
  })
})
