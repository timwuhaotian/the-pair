import React, { Component, ReactNode } from 'react'

interface Props {
  children: ReactNode
  fallback?: ReactNode
}

interface State {
  hasError: boolean
  error: Error | null
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props)
    this.state = { hasError: false, error: null }
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo): void {
    console.error('[ErrorBoundary] Caught error:', error, errorInfo)
  }

  render(): ReactNode {
    if (this.state.hasError) {
      if (this.props.fallback) return this.props.fallback

      return (
        <div className="flex h-full w-full items-center justify-center bg-background p-8 font-mono">
          <div className="max-w-2xl border border-state-error bg-state-error/10 p-5 rounded-sm">
            <h2 className="mb-3 text-[13px] uppercase tracking-[0.14em] state-error font-bold">
              ✗ unhandled exception
            </h2>
            <pre className="overflow-auto scrollbar-thin border border-border bg-background/40 p-3 text-[11px] leading-relaxed text-foreground/85 [overflow-wrap:anywhere]">
              {this.state.error?.toString()}
              {'\n\n'}
              {this.state.error?.stack}
            </pre>
            <button
              onClick={() => window.location.reload()}
              className="mt-3 inline-flex items-center gap-1.5 border border-foreground bg-foreground text-background px-3 py-1.5 text-[12px] font-bold uppercase tracking-[0.12em] rounded-sm cursor-pointer hover:bg-foreground/90 transition-colors"
            >
              ▸ reload application
            </button>
          </div>
        </div>
      )
    }

    return this.props.children
  }
}
