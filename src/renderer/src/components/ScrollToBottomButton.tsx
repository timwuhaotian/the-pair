import React, { useEffect, useState, useCallback } from 'react'
import { ArrowDown } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { GlassButton } from './ui/GlassButton'

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
    <GlassButton
      variant="secondary"
      size="xs"
      icon={<ArrowDown size={11} />}
      onClick={scrollToBottom}
      className={cn(
        'absolute bottom-4 right-4 z-10 bg-background font-mono hover:bg-muted',
        className
      )}
    >
      {t('console.newMessages')}
    </GlassButton>
  )
}
