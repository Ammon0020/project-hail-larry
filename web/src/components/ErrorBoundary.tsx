import { Component, type ErrorInfo, type ReactNode } from 'react'
import { Banner } from '@/components/ui/Banner'
import { Button } from '@/components/ui/button'

interface Props {
  children: ReactNode
  /** Label shown in the compact fallback ("<name> error") for debugging. */
  name?: string
  /**
   * When true, render a small inline fallback that fits inside a single panel
   * instead of the full-screen one. Used to wrap individual panels (sidebar,
   * editor, chat) so a render crash in one panel doesn't unmount the others.
   */
  compact?: boolean
}

interface State {
  error: Error | null
}

/**
 * React error boundary. Without this, any throw during render unmounts the
 * entire IDE and leaves a blank page — the worst perceived-error state for a
 * self-hosted editor (looks like the daemon died). Catches render errors
 * anywhere in the tree and shows a recoverable fallback.
 *
 * Two fallback modes:
 * - Full-screen (default): used at the top level, offers a Reload button.
 * - Compact (`compact` prop): a small bordered box that fits inside a panel,
 *   with a Retry button that resets the boundary so children re-render without
 *   a full page reload. Wrap individual panels with `compact` + `name` so a
 *   crash in one panel doesn't take down the whole app.
 *
 * Intentionally minimal: no reporting service, no retry beyond reload/Retry.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('[ErrorBoundary] render crashed:', error, info)
  }

  render() {
    if (!this.state.error) return this.props.children

    // Compact inline fallback — sized to live inside a single panel.
    if (this.props.compact) {
      return (
        <div className="flex h-full items-center justify-center p-3">
          <div className="flex max-w-sm flex-col gap-2 rounded border border-destructive/30 bg-destructive/5 p-3 text-xs">
            <p className="font-semibold text-destructive">{this.props.name ?? 'Panel'} error</p>
            <pre className="max-h-24 overflow-auto whitespace-pre-wrap break-words text-[11px] text-muted-foreground">
              {this.state.error.message}
            </pre>
            <Button onClick={() => this.setState({ error: null })} size="sm" className="self-start">
              Retry
            </Button>
          </div>
        </div>
      )
    }

    // Full-screen fallback (top-level boundary).
    return (
      <div className="flex min-h-screen items-center justify-center p-4">
        <Banner
          variant="error"
          role="alert"
          className="flex max-w-md flex-col gap-3 rounded-md border p-4 text-sm"
        >
          <p className="font-semibold">Something went wrong</p>
          <p className="text-xs">
            The IDE hit an unexpected error while rendering. Reloading usually
            clears it.
          </p>
          <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-words rounded bg-destructive/10 p-2 text-[11px]">
            {this.state.error.message}
          </pre>
          <Button onClick={() => location.reload()} size="sm" className="self-start">
            Reload
          </Button>
        </Banner>
      </div>
    )
  }
}
