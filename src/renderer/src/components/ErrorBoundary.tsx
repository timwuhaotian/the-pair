import React, { Component, ReactNode } from 'react'
import { GlassButton } from './ui/GlassButton'

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
        <div className="flex h-full w-full items-center justify-center bg-background p-8">
          <div className="shell-card max-w-2xl border-state-error/50 bg-state-error/10 p-5">
            <h2 className="mb-3 text-[14px] font-semibold tracking-tight state-error">
              Unhandled exception
            </h2>
            <pre className="overflow-auto scrollbar-thin rounded-md border border-border bg-background/40 p-3 text-[11px] leading-relaxed text-foreground/85 [overflow-wrap:anywhere]">
              {this.state.error?.toString()}
              {'\n\n'}
              {this.state.error?.stack}
            </pre>
            <div className="mt-3 flex justify-end">
              <GlassButton variant="primary" size="sm" onClick={() => window.location.reload()}>
                Reload application
              </GlassButton>
            </div>
          </div>
        </div>
      )
    }

    return this.props.children
  }
}
