import { describe, it, expect } from 'vitest'
import { describeSessionError, isSessionNotFound, SESSION_GONE_MESSAGE } from '../errors'

describe('isSessionNotFound', () => {
  it('matches the backend 404 text and the already-friendly form', () => {
    expect(isSessionNotFound('session not found: sess-abc')).toBe(true)
    expect(isSessionNotFound('Session Not Found: sess-abc')).toBe(true)
    expect(isSessionNotFound(SESSION_GONE_MESSAGE)).toBe(true)
    expect(isSessionNotFound('workspace not found: ws-1')).toBe(false)
  })
})

describe('describeSessionError', () => {
  it('reads the message off an Error and flags a missing session', () => {
    const result = describeSessionError(new Error('session not found: sess-1'), 'fallback')

    expect(result.sessionGone).toBe(true)
    expect(result.message).toBe('session not found: sess-1')
  })

  it('passes other failures through for the caller to display', () => {
    const result = describeSessionError(new Error('agent exited'), 'fallback')

    expect(result.sessionGone).toBe(false)
    expect(result.message).toBe('agent exited')
  })

  it('falls back when the thrown value is not an Error', () => {
    // Rejected fetches and thrown strings both reach these handlers, so a
    // non-Error must not produce "[object Object]" in the UI.
    expect(describeSessionError({ code: 500 }, 'Failed to send message')).toEqual({
      message: 'Failed to send message',
      sessionGone: false,
    })
  })
})
