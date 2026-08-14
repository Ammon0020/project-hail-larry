import { useEffect, useMemo, useState } from 'react'
import type { AppEvent, EditedFile } from '@/types'
import { api } from '@/lib/api'
import { aggregateEditedFiles } from '@/lib/editedFiles'

/** Tracks files written by an agent, enriching live WS paths with REST diff counts. */
export function useEditedFiles(events: AppEvent[], sessionId: string | null): EditedFile[] {
  const liveSignature = useMemo(() => {
    if (!sessionId) return ''

    const sessionEvents = events.filter((event) => event.sessionId === sessionId)
    return aggregateEditedFiles(sessionEvents)
      .map((file) => `${file.path}:${file.eventId}`)
      .join('\n')
  }, [events, sessionId])

  const [serverData, setServerData] = useState<{
    sessionId: string | null
    loaded: boolean
    files: EditedFile[]
  }>({
    sessionId: null,
    loaded: false,
    files: [],
  })
  useEffect(() => {
    let cancelled = false
    let requestGeneration = 0
    if (!sessionId) return () => { cancelled = true }

    const load = () => {
      const generation = ++requestGeneration
      void api.getEditedFiles(sessionId)
        .then((files) => {
          if (!cancelled && generation === requestGeneration) {
            setServerData({ sessionId, loaded: true, files })
          }
        })
        .catch(() => {
          if (!cancelled && generation === requestGeneration) {
            // Keep a prior verified response for this session. Historical
            // FileWritten events are not proof that an edit is still pending.
            setServerData((previous) =>
              previous.sessionId === sessionId
                ? previous
                : { sessionId, loaded: false, files: [] },
            )
          }
        })
    }

    // Coalesce bursts of FileWritten events, then poll lightly so accept/revert
    // actions performed by another connected client invalidate this view.
    const timer = window.setTimeout(load, 100)
    const interval = window.setInterval(load, 5_000)

    return () => {
      cancelled = true
      window.clearTimeout(timer)
      window.clearInterval(interval)
    }
  }, [sessionId, liveSignature])

  return useMemo(() => {
    // Only a successful cache response can assert that an edit is pending.
    // This prevents historical events and cache-limit exclusions from exposing
    // diff/revert actions that can only return 404.
    return serverData.loaded && serverData.sessionId === sessionId ? serverData.files : []
  }, [serverData, sessionId])
}
