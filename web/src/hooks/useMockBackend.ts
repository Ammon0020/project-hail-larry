import { useCallback, useState } from 'react'
import type { AppEvent } from '@/types'

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

  /**
   * Simulates sending a prompt to the agent via ACP (Blueprint Sec 6).
   * In production: client sends PromptSubmitted → daemon forwards to agent
   * via ACP session/prompt → agent streams back responses.
   */
  const sendPrompt = useCallback((sessionId: string, content: string) => {
    // Add user message immediately
    setEvents((prev) => [
      ...prev,
      { type: 'PromptSubmitted', sessionId, role: 'user', content },
    ])

    // Simulate agent response after delay
    setTimeout(() => {
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
      setTimeout(() => {
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
    }, 500)
  }, [])

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

  return { events, sendPrompt, respondPermission }
}
