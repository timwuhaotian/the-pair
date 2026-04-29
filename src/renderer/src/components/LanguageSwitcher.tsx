import React, { useState, useRef, useEffect, useCallback } from 'react'
import { createPortal } from 'react-dom'
import { motion, AnimatePresence } from 'framer-motion'
import { Languages, Check } from 'lucide-react'
import { useLocaleStore, SupportedLocale, LOCALE_LABELS } from '../store/useLocaleStore'
import { cn } from '../lib/utils'

const LOCALES: SupportedLocale[] = ['en', 'zh', 'ja', 'ko']

export function LanguageSwitcher(): React.ReactNode {
  const { locale, setLocale } = useLocaleStore()
  const [open, setOpen] = useState(false)
  const [position, setPosition] = useState({ top: 0, left: 0 })
  const ref = useRef<HTMLDivElement>(null)
  const btnRef = useRef<HTMLButtonElement>(null)

  const updatePosition = useCallback(() => {
    if (!btnRef.current) return
    const rect = btnRef.current.getBoundingClientRect()
    const panelHeight = 260
    const top = rect.bottom + 4
    const left = Math.max(8, rect.right - 192)
    if (top + panelHeight > window.innerHeight) {
      setPosition({ top: rect.top - panelHeight - 4, left })
    } else {
      setPosition({ top, left })
    }
  }, [])

  useEffect(() => {
    if (open) {
      updatePosition()
      window.addEventListener('resize', updatePosition)
      return () => window.removeEventListener('resize', updatePosition)
    }
  }, [open, updatePosition])

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [])

  return (
    <div ref={ref} className="relative">
      <button
        ref={btnRef}
        onClick={() => setOpen(!open)}
        className="flex h-10 w-10 items-center justify-center rounded-2xl border border-border bg-muted/40 text-muted-foreground transition-all hover:bg-muted hover:text-foreground cursor-pointer"
        title="Switch language"
        data-testid="chrome-language-toggle"
      >
        <Languages size={16} />
      </button>

      {createPortal(
        <AnimatePresence>
          {open && (
            <>
              <motion.div
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.15 }}
                className="fixed inset-0 z-[9998]"
                onClick={() => setOpen(false)}
              />
              <motion.div
                initial={{ opacity: 0, y: -8, scale: 0.96 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: -8, scale: 0.96 }}
                transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
                className="fixed z-[9999] w-48 rounded-2xl border border-border/60 bg-background/95 p-1.5 shadow-xl backdrop-blur-xl"
                style={{ top: position.top, left: position.left }}
              >
                <div className="mb-1 px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.15em] text-muted-foreground">
                  Language
                </div>
                {LOCALES.map((l) => {
                  const isActive = locale === l
                  const meta = LOCALE_LABELS[l]
                  return (
                    <button
                      key={l}
                      onClick={() => {
                        setLocale(l)
                        setOpen(false)
                      }}
                      className={cn(
                        'flex w-full items-center gap-2.5 rounded-xl px-2.5 py-2 text-sm transition-colors',
                        isActive
                          ? 'bg-primary/10 text-primary'
                          : 'text-foreground hover:bg-muted/60'
                      )}
                    >
                      <span className="text-base">{meta.flag}</span>
                      <span className="flex-1 text-left font-medium">{meta.native}</span>
                      {isActive && <Check size={14} className="text-primary" />}
                    </button>
                  )
                })}
              </motion.div>
            </>
          )}
        </AnimatePresence>,
        document.body
      )}
    </div>
  )
}
