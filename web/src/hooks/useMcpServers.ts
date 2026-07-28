import { useEffect, useRef, useState } from 'react'
import {
  getMcpConfig,
  getMcpStatus,
  patchMcpServer,
  type McpServerStatus,
} from '@/lib/api'

/**
 * Manages MCP server configuration, inline toggles, health refreshes, and the
 * restart-required state shown after an active configuration changes.
 */
export function useMcpServers() {
  const [mcpServers, setMcpServers] = useState<{ name: string; enabled: boolean }[]>([])
  const [mcpTogglingServer, setMcpTogglingServer] = useState<string | null>(null)
  const [mcpConfigChanged, setMcpConfigChanged] = useState(false)
  const [mcpHealth, setMcpHealth] = useState<Record<string, McpServerStatus>>({})
  const [mcpStatusLoading, setMcpStatusLoading] = useState(false)
  const [mcpError, setMcpError] = useState<string | null>(null)
  const mountedRef = useRef(true)
  const mcpStatusReqRef = useRef(0)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
    }
  }, [])

  async function loadMcpServers() {
    try {
      const text = await getMcpConfig()
      const parsed = JSON.parse(text) as {
        mcpServers?: Record<string, { enabled?: boolean }>
      }
      const entries = Object.entries(parsed.mcpServers || {})
      setMcpServers(
        entries.map(([name, config]) => ({
          name,
          enabled: config.enabled !== false,
        })),
      )
      setMcpError(null)
    } catch (error) {
      console.error('Failed to load MCP config:', error)
      setMcpError('Couldn’t load MCP server config from the daemon.')
    }
  }

  /**
   * Refreshes health status without allowing stale requests or an unmounted
   * component to overwrite newer state.
   */
  async function loadMcpStatus() {
    const requestId = ++mcpStatusReqRef.current
    setMcpStatusLoading(true)
    try {
      const statuses = await getMcpStatus()
      if (requestId !== mcpStatusReqRef.current || !mountedRef.current) return
      const nextHealth: Record<string, McpServerStatus> = {}
      for (const status of statuses) nextHealth[status.name] = status
      setMcpHealth(nextHealth)
      setMcpError(null)
    } catch (error) {
      if (requestId !== mcpStatusReqRef.current || !mountedRef.current) return
      console.error('Failed to load MCP status:', error)
      // Preserve last-known health; clearing it would mask the failure as "unknown".
      setMcpError('Couldn’t load MCP server health.')
    } finally {
      if (requestId === mcpStatusReqRef.current && mountedRef.current) {
        setMcpStatusLoading(false)
      }
    }
  }

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    loadMcpServers().then(() => loadMcpStatus())
  }, [])

  const handleToggleMcpServer = async (name: string, enabled: boolean) => {
    setMcpTogglingServer(name)
    try {
      await patchMcpServer(name, enabled)
      await loadMcpServers()
      await loadMcpStatus()
      setMcpConfigChanged(true)
    } catch (error) {
      console.error('Failed to toggle MCP server:', error)
      setMcpError(`Couldn’t ${enabled ? 'enable' : 'disable'} MCP server “${name}”.`)
    } finally {
      setMcpTogglingServer(null)
    }
  }

  return {
    mcpServers,
    mcpHealth,
    mcpStatusLoading,
    mcpTogglingServer,
    mcpConfigChanged,
    setMcpConfigChanged,
    mcpError,
    setMcpError,
    loadMcpStatus,
    handleToggleMcpServer,
  }
}
