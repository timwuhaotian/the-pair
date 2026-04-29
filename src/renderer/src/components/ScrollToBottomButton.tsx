import React, { useEffect, useState, useCallback } from 'react'
import { ArrowDown } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { motion, AnimatePresence } from 'framer-motion'
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

    const handleScroll = (): void => {
      updateVisibility()
    }

    el.addEventListener('scroll', handleScroll, { passive: true })
    return () => el.removeEventListener('scroll', handleScroll)
  }, [scrollRef, updateVisibility])

  const scrollToBottom = (): void => {
    const el = scrollRef.current
    if (!el) return
    el.scrollTo({
      top: el.scrollHeight,
      behavior: 'smooth'
    })
  }

  return (
    <AnimatePresence>
      {showButton && (
        <motion.button
          initial={{ opacity: 0, scale: 0.8, y: 8 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.8, y: 8 }}
          transition={{ duration: 0.2 }}
          onClick={scrollToBottom}
          className={cn(
            'absolute bottom-6 right-6 z-10',
            'flex items-center gap-1.5 rounded-full',
            'border border-border/60 bg-background/90 backdrop-blur-md',
            'px-3 py-1.5 shadow-lg',
            'text-[10px] font-medium text-muted-foreground',
            'hover:bg-background hover:text-foreground transition-colors',
            'cursor-pointer',
            className
          )}
        >
          <ArrowDown size={12} />
          <span>{t('console.newMessages')}</span>
        </motion.button>
      )}
    </AnimatePresence>
  )
}
