import React, { useEffect, useState, useCallback } from 'react'
import { ArrowDown } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'

interface ScrollToBottomButtonProps {
  scrollRef: React.RefObject<HTMLDivElement | null>
  dependency: unknown
  threshold?: number
  className?: string
}

export function ScrollToBottomButton({
  scrollRef,
  dependency,
  threshold = 160,
  className
}: ScrollToBottomButtonProps): React.ReactNode {
  const { t } = useTranslation()
  const [showButton, setShowButton] = useState(false)

  const updateVisibility = useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    const distanceToBottom = el.scrollHeight - el.scrollTop - el.clientHeight
    setShowButton(distanceToBottom >= threshold)
  }, [scrollRef, threshold])

  useEffect(() => {
    updateVisibility()
  }, [dependency, updateVisibility])

  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    const handleScroll = (): void => updateVisibility()
    el.addEventListener('scroll', handleScroll, { passive: true })
    return () => el.removeEventListener('scroll', handleScroll)
  }, [scrollRef, updateVisibility])

  const scrollToBottom = (): void => {
    const el = scrollRef.current
    if (!el) return
    el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
  }

  if (!showButton) return null

  return (
    <button
      onClick={scrollToBottom}
      className={cn(
        'absolute bottom-4 right-4 z-10 inline-flex items-baseline gap-1 px-2 py-1 border border-border bg-background rounded-sm font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground hover:bg-foreground/[0.05] hover:text-foreground hover:border-foreground/40 transition-colors cursor-pointer',
        className
      )}
    >
      <ArrowDown size={10} className="translate-y-px" />
      <span>· {t('console.newMessages')}</span>
    </button>
  )
}
