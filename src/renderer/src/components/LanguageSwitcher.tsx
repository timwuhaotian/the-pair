import React, { useState, useRef, useEffect, useCallback } from 'react'
import { createPortal } from 'react-dom'
import { Languages } from 'lucide-react'
import { useLocaleStore, SupportedLocale, LOCALE_LABELS } from '../store/useLocaleStore'
import { cn } from '../lib/utils'

const LOCALES: SupportedLocale[] = ['en', 'zh', 'ja', 'ko']

export function LanguageSwitcher(): React.ReactNode {
  const { locale, setLocale } = useLocaleStore()
  const [open, setOpen] = useState(false)
  const [position, setPosition] = useState({ top: 0, left: 0 })
  const ref = useRef<HTMLDivElement>(null)
  const btnRef = useRef<HTMLButtonElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)

  const updatePosition = useCallback(() => {
    if (!btnRef.current) return
    const rect = btnRef.current.getBoundingClientRect()
    const panelHeight = 220
    const top = rect.bottom + 4
    const left = Math.max(8, rect.right - 180)
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
    const handler = (e: MouseEvent): void => {
      const target = e.target as Node
      if (ref.current?.contains(target)) return
      if (panelRef.current?.contains(target)) return
      setOpen(false)
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [])

  return (
    <div ref={ref} className="relative">
      <button
        ref={btnRef}
        onClick={() => setOpen(!open)}
        className="flex h-7 w-7 items-center justify-center rounded-sm border border-border bg-transparent text-muted-foreground transition-colors hover:bg-foreground/[0.06] hover:border-foreground/40 hover:text-foreground cursor-pointer"
        title="switch language"
        data-testid="chrome-language-toggle"
      >
        <Languages size={13} />
      </button>

      {createPortal(
        open ? (
          <>
            <div className="fixed inset-0 z-[9998]" onClick={() => setOpen(false)} />
            <div
              ref={panelRef}
              className="fixed z-[9999] w-44 rounded-sm border border-border bg-popover p-1 font-mono"
              style={{ top: position.top, left: position.left }}
            >
              <div className="px-2 py-1 text-[10px] uppercase tracking-[0.16em] text-muted-foreground">
                ── language ──
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
                      'flex w-full items-baseline gap-2 px-2 py-1 text-[12px] transition-colors cursor-pointer rounded-sm',
                      isActive
                        ? 'bg-foreground/[0.08] text-foreground'
                        : 'text-foreground/85 hover:bg-foreground/[0.05]'
                    )}
                  >
                    <span
                      aria-hidden
                      className={cn(
                        'w-[1ch] select-none',
                        isActive ? 'role-mentor' : 'text-muted-foreground-faint'
                      )}
                    >
                      {isActive ? '●' : '○'}
                    </span>
                    <span className="text-base leading-none">{meta.flag}</span>
                    <span className="flex-1 text-left">{meta.native}</span>
                  </button>
                )
              })}
            </div>
          </>
        ) : null,
        document.body
      )}
    </div>
  )
}
