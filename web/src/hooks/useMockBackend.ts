import { useCallback, useEffect, useRef, useState } from 'react'
import type { AppEvent, Attachment } from '@/types'
import type { UploadResult } from '@/lib/api'

/**
 * Mock backend hook — simulates the Go daemon's WebSocket + ACP layer.
 *
 * In production, the browser connects to:
 *   ws://app.local:7337/ws  (mDNS)  or
 *   ws://192.168.x.x:7337/ws  (direct IP)
 *
 * The daemon broadcasts events to all paired clients (Blueprint Sec 12).
 * This mock simulates: send prompt, receive streaming events, permission responses.
 */
export function useMockBackend(initialEvents: AppEvent[]) {
  const [events, setEvents] = useState<AppEvent[]>(initialEvents)
  // Tracks active setTimeout ids so they can be cleared on unmount, preventing
  // setState-after-unmount warnings and leaked timers if the mock is hot-reloaded
  // mid-stream.
  const timersRef = useRef<number[]>([])

  useEffect(
    () => () => {
      timersRef.current.forEach((id) => clearTimeout(id))
    },
    [],
  )

  /**
   * Simulates sending a prompt to the agent via ACP (Blueprint Sec 6).
   * In production: client sends PromptSubmitted → daemon forwards to agent
   * via ACP session/prompt → agent streams back responses.
   */
  const sendPrompt = useCallback(
    (sessionId: string, content: string, attachments?: Attachment[]) => {
      // Add user message immediately
      setEvents((prev) => [
        ...prev,
        {
          type: 'PromptSubmitted',
          sessionId,
          role: 'user',
          content,
          attachments,
        },
      ])

      // Simulate agent response after delay
      const id1 = window.setTimeout(() => {
        setEvents((prev) => [
          ...prev,
          {
            type: 'ResponseStarted',
            sessionId,
            role: 'agent',
            content: "I'll help with that. Let me analyze the current state of the code.",
          },
        ])

        // Simulate streaming indicator
        const id2 = window.setTimeout(() => {
          setEvents((prev) => [
            ...prev,
            {
              type: 'StreamUpdate',
              sessionId,
              role: 'agent',
              content: 'Analyzing file structure...',
              streaming: true,
            },
          ])
        }, 800)
        timersRef.current.push(id2)
      }, 500)
      timersRef.current.push(id1)
    },
    [],
  )

  /** Simulates a permission response (Blueprint Sec 8). */
  const respondPermission = useCallback((sessionId: string, decision: 'allow' | 'deny') => {
    setEvents((prev) => [
      ...prev,
      {
        type: decision === 'allow' ? 'PermissionGranted' : 'PermissionDenied',
        sessionId,
      },
    ])
  }, [])

  /** Mock upload — returns a static UploadResult matching api.uploadFile's shape. */
  const uploadFile = useCallback(
    async (_sessionId: string, file: File): Promise<UploadResult> => ({
      id: `mock-upload-${Date.now()}`,
      name: file.name,
      mimeType: file.type || 'application/octet-stream',
      url: URL.createObjectURL(file),
      size: file.size,
    }),
    [],
  )

  return { events, sendPrompt, respondPermission, uploadFile }
}
