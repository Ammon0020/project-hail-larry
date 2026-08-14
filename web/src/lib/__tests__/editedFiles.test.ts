import { describe, it, expect } from 'vitest'
import { aggregateEditedFiles } from '@/lib/editedFiles'
import type { AppEvent } from '@/types'

function event(overrides: Partial<AppEvent> = {}): AppEvent {
  return {
    type: 'FileWritten',
    sessionId: 'session-1',
    ...overrides,
  }
}

describe('aggregateEditedFiles', () => {
  it('returns an empty list for empty input', () => {
    expect(aggregateEditedFiles([])).toEqual([])
  })

  it('ignores events other than FileWritten', () => {
    const events = [
      event({ type: 'SessionCreated', id: 1, target: 'src/ignored.ts' }),
      event({ type: 'SessionClosed', id: 2, target: 'src/also-ignored.ts' }),
    ]

    expect(aggregateEditedFiles(events)).toEqual([])
  })

  it('returns a single written file', () => {
    const files = aggregateEditedFiles([event({ id: 7, target: 'src/app.ts' })])

    expect(files).toEqual([{
      path: 'src/app.ts',
      eventId: 7,
      timestamp: '',
      addedLines: 0,
      removedLines: 0,
    }])
  })

  it('sorts files alphabetically by path', () => {
    const files = aggregateEditedFiles([
      event({ id: 1, target: 'src/zebra.ts' }),
      event({ id: 2, target: 'src/alpha.ts' }),
      event({ id: 3, target: 'README.md' }),
    ])

    expect(files).toEqual([
      { path: 'README.md', eventId: 3, timestamp: '', addedLines: 0, removedLines: 0 },
      { path: 'src/alpha.ts', eventId: 2, timestamp: '', addedLines: 0, removedLines: 0 },
      { path: 'src/zebra.ts', eventId: 1, timestamp: '', addedLines: 0, removedLines: 0 },
    ])
  })

  it('deduplicates paths, retaining the highest event ID', () => {
    const files = aggregateEditedFiles([
      event({ id: 9, target: 'src/app.ts' }),
      event({ id: 4, target: 'src/app.ts' }),
      event({ id: 12, target: 'src/app.ts' }),
    ])

    expect(files).toEqual([{
      path: 'src/app.ts',
      eventId: 12,
      timestamp: '',
      addedLines: 0,
      removedLines: 0,
    }])
  })

  it('treats events without IDs as event ID 0', () => {
    const files = aggregateEditedFiles([event({ target: 'src/app.ts' })])

    expect(files).toEqual([{
      path: 'src/app.ts',
      eventId: 0,
      timestamp: '',
      addedLines: 0,
      removedLines: 0,
    }])
  })

  it('skips FileWritten events without a target path', () => {
    const files = aggregateEditedFiles([
      event({ id: 1 }),
      event({ id: 2, target: '' }),
      event({ id: 3, target: 'src/app.ts' }),
    ])

    expect(files).toEqual([{
      path: 'src/app.ts',
      eventId: 3,
      timestamp: '',
      addedLines: 0,
      removedLines: 0,
    }])
  })
})
