import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { StartupHero } from './StartupHero'

interface BootSplashProps {
  /**
   * When true the splash is fully opaque. When false, it fades out and
   * unmounts itself after the transition completes.
   */
  visible: boolean
  /**
   * Optional minimum visible duration in ms — keeps the splash on screen
   * long enough to feel intentional even when the app loads instantly.
   * Defaults to 700ms.
   */
  minDurationMs?: number
}

export function BootSplash({ visible, minDurationMs = 700 }: BootSplashProps): React.ReactNode {
  const { t } = useTranslation()
  const [mounted, setMounted] = useState(true)
  const [leaving, setLeaving] = useState(false)
  const [mountedAt] = useState(() => Date.now())

  useEffect(() => {
    if (visible || leaving) return
    const elapsed = Date.now() - mountedAt
    const remaining = Math.max(0, minDurationMs - elapsed)

    let removeTimer: number | undefined
    const startLeavingTimer = window.setTimeout(() => {
      setLeaving(true)
      // total fade animation: 80ms delay + 380ms fade = ~460ms
      removeTimer = window.setTimeout(() => setMounted(false), 480)
    }, remaining)

    return () => {
      window.clearTimeout(startLeavingTimer)
      if (removeTimer !== undefined) window.clearTimeout(removeTimer)
    }
  }, [visible, leaving, mountedAt, minDurationMs])

  if (!mounted) return null

  return (
    <div
      className={cn(
        'boot-splash fixed inset-0 z-[100] flex items-center justify-center bg-background',
        leaving && 'boot-splash--leaving'
      )}
      aria-hidden={leaving}
    >
      {/* subtle terminal vignette so the brand sits in something */}
      <div className="absolute inset-0 grain-overlay" />

      <div className="relative flex flex-col items-center gap-6 px-8">
        <StartupHero
          size="lg"
          animated
          tagline={t('startup.tagline')}
          wordmark={t('startup.wordmark')}
          caption={t('startup.bootCaption')}
        />

        {/* loading bar — three blocks marching, terminal style */}
        <div
          className="mt-2 flex items-center gap-1.5 text-[10px] uppercase tracking-[0.28em] text-muted-foreground-faint"
          aria-live="polite"
        >
          <span aria-hidden className="select-none">
            {'>_'}
          </span>
          <span className="boot-splash__dots">{t('startup.booting')}</span>
        </div>
      </div>
    </div>
  )
}
