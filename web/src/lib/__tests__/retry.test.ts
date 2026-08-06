import { describe, it, expect, vi } from 'vitest'
import { withRetry, isTransientError, defaultBackoff } from '@/lib/retry'
import { ApiError } from '@/lib/api/client'

/** A sleep that resolves immediately — keeps tests fast and deterministic. */
const noSleep = vi.fn().mockResolvedValue(undefined)

/** Sentinel marking "no result provided" so `undefined` could be a valid result. */
const NO_RESULT = Symbol('no-result')

/** Builds a mock async function that throws the given errors in sequence,
 *  then optionally returns a value. When no `result` is given the last error
 *  keeps being thrown on every subsequent call (so the function never
 *  succeeds). */
function mockFn<T>(throws: unknown[], result: T | typeof NO_RESULT = NO_RESULT) {
  let calls = 0
  return vi.fn(async (): Promise<T> => {
    const idx = Math.min(calls, throws.length - 1)
    const err = throws[idx]
    calls++
    // Throw while there are still queued errors, or forever when no result.
    if (calls <= throws.length || result === NO_RESULT) throw err
    return result as T
  })
}

describe('isTransientError', () => {
  it('treats TypeError (network error) as transient', () => {
    expect(isTransientError(new TypeError('Failed to fetch'))).toBe(true)
  })

  it('treats 502, 503, 504 as transient', () => {
    expect(isTransientError(new ApiError('err', 502))).toBe(true)
    expect(isTransientError(new ApiError('err', 503))).toBe(true)
    expect(isTransientError(new ApiError('err', 504))).toBe(true)
  })

  it('does not treat 4xx as transient', () => {
    for (const status of [400, 401, 403, 404, 409]) {
      expect(isTransientError(new ApiError('err', status))).toBe(false)
    }
  })

  it('does not treat 500 as transient', () => {
    expect(isTransientError(new ApiError('err', 500))).toBe(false)
  })

  it('does not treat other 5xx as transient', () => {
    expect(isTransientError(new ApiError('err', 500))).toBe(false)
    expect(isTransientError(new ApiError('err', 599))).toBe(false)
  })

  it('does not treat generic errors as transient', () => {
    expect(isTransientError(new Error('something'))).toBe(false)
    expect(isTransientError('string')).toBe(false)
    expect(isTransientError(null)).toBe(false)
  })
})

describe('defaultBackoff', () => {
  it('returns 1s, 2s, 4s for attempts 0, 1, 2', () => {
    expect(defaultBackoff(0)).toBe(1000)
    expect(defaultBackoff(1)).toBe(2000)
    expect(defaultBackoff(2)).toBe(4000)
  })
})

describe('withRetry', () => {
  it('returns the result on the first success without retrying', async () => {
    const fn = vi.fn(async () => 'ok')
    const onRetry = vi.fn()
    const result = await withRetry(fn, { onRetry, sleep: noSleep })
    expect(result).toBe('ok')
    expect(fn).toHaveBeenCalledTimes(1)
    expect(onRetry).not.toHaveBeenCalled()
  })

  it('retries on network error (TypeError) and returns on success', async () => {
    const fn = mockFn([new TypeError('Failed to fetch')], 'ok')
    const onRetry = vi.fn()
    const result = await withRetry(fn, { onRetry, sleep: noSleep })
    expect(result).toBe('ok')
    expect(fn).toHaveBeenCalledTimes(2)
    expect(onRetry).toHaveBeenCalledTimes(1)
  })

  it('retries on 502, 503, 504 and returns on success', async () => {
    for (const status of [502, 503, 504]) {
      const fn = mockFn([new ApiError('err', status)], 'ok')
      const onRetry = vi.fn()
      const result = await withRetry(fn, { onRetry, sleep: noSleep })
      expect(result).toBe('ok')
      expect(fn).toHaveBeenCalledTimes(2)
      expect(onRetry).toHaveBeenCalledTimes(1)
    }
  })

  it('does not retry on 400', async () => {
    const fn = mockFn([new ApiError('bad request', 400)])
    const onRetry = vi.fn()
    await expect(withRetry(fn, { onRetry, sleep: noSleep })).rejects.toThrow('bad request')
    expect(fn).toHaveBeenCalledTimes(1)
    expect(onRetry).not.toHaveBeenCalled()
  })

  it('does not retry on 401', async () => {
    const fn = mockFn([new ApiError('unauthorized', 401)])
    await expect(withRetry(fn, { sleep: noSleep })).rejects.toThrow('unauthorized')
    expect(fn).toHaveBeenCalledTimes(1)
  })

  it('does not retry on 403', async () => {
    const fn = mockFn([new ApiError('forbidden', 403)])
    await expect(withRetry(fn, { sleep: noSleep })).rejects.toThrow('forbidden')
    expect(fn).toHaveBeenCalledTimes(1)
  })

  it('does not retry on 404', async () => {
    const fn = mockFn([new ApiError('not found', 404)])
    await expect(withRetry(fn, { sleep: noSleep })).rejects.toThrow('not found')
    expect(fn).toHaveBeenCalledTimes(1)
  })

  it('does not retry on 409', async () => {
    const fn = mockFn([new ApiError('conflict', 409)])
    await expect(withRetry(fn, { sleep: noSleep })).rejects.toThrow('conflict')
    expect(fn).toHaveBeenCalledTimes(1)
  })

  it('does not retry on 500', async () => {
    const fn = mockFn([new ApiError('server error', 500)])
    const onRetry = vi.fn()
    await expect(withRetry(fn, { onRetry, sleep: noSleep })).rejects.toThrow('server error')
    expect(fn).toHaveBeenCalledTimes(1)
    expect(onRetry).not.toHaveBeenCalled()
  })

  it('respects the max retry count (3 retries = 4 total attempts)', async () => {
    // Always throws a transient error — should exhaust all retries.
    const fn = mockFn([new TypeError('Failed to fetch')])
    const onRetry = vi.fn()
    await expect(withRetry(fn, { onRetry, sleep: noSleep })).rejects.toThrow('Failed to fetch')
    // 1 initial attempt + 3 retries = 4 total.
    expect(fn).toHaveBeenCalledTimes(4)
    expect(onRetry).toHaveBeenCalledTimes(3)
  })

  it('returns a successful response after multiple retries', async () => {
    const fn = mockFn(
      [new TypeError('Failed to fetch'), new ApiError('err', 503)],
      'recovered',
    )
    const onRetry = vi.fn()
    const result = await withRetry(fn, { onRetry, sleep: noSleep })
    expect(result).toBe('recovered')
    expect(fn).toHaveBeenCalledTimes(3)
    expect(onRetry).toHaveBeenCalledTimes(2)
  })

  it('surfaces the last error after exhausting retries', async () => {
    const fn = mockFn([new ApiError('gateway down', 502)])
    await expect(withRetry(fn, { sleep: noSleep })).rejects.toThrow('gateway down')
    expect(fn).toHaveBeenCalledTimes(4)
  })

  it('uses the provided getDelay for backoff timing', async () => {
    const fn = mockFn([new TypeError('Failed to fetch')], 'ok')
    const getDelay = vi.fn().mockReturnValue(50)
    const sleep = vi.fn().mockResolvedValue(undefined)
    await withRetry(fn, { getDelay, sleep })
    expect(getDelay).toHaveBeenCalledWith(0)
    expect(sleep).toHaveBeenCalledWith(50)
  })

  it('honors a custom maxRetries', async () => {
    const fn = mockFn([new TypeError('Failed to fetch')])
    await expect(withRetry(fn, { maxRetries: 1, sleep: noSleep })).rejects.toThrow()
    // 1 initial + 1 retry = 2 total.
    expect(fn).toHaveBeenCalledTimes(2)
  })
})
