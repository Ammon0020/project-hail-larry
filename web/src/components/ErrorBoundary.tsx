import { Component, type ErrorInfo, type ReactNode } from 'react'
import { Banner } from '@/components/ui/Banner'
import { Button } from '@/components/ui/button'

interface Props {
  children: ReactNode
}

interface State {
  error: Error | null
}

/**
 * Top-level React error boundary. Without this, any throw during render
 * unmounts the entire IDE and leaves a blank page — the worst perceived-error
 * state for a self-hosted editor (looks like the daemon died). Catches render
 * errors anywhere in the tree and shows a recoverable fallback with a Reload
 * button instead. Intentionally minimal: no reporting service, no retry beyond
 * a full page reload.
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
    if (this.state.error) {
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
    return this.props.children
  }
}
