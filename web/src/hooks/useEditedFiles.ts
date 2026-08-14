import { useMemo } from 'react'
import type { AppEvent, EditedFile } from '@/types'
import { aggregateEditedFiles } from '@/lib/editedFiles'

/** Tracks the files an agent has edited in a session from the live event stream. */
export function useEditedFiles(events: AppEvent[], sessionId: string | null): EditedFile[] {
  return useMemo(() => {
    if (!sessionId) return []

    const sessionEvents = events.filter((event) => event.sessionId === sessionId)
    return aggregateEditedFiles(sessionEvents)
  }, [events, sessionId])
}
