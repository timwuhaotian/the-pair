import React, { useState, useRef, useEffect, useCallback } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { Languages } from 'lucide-react'
import { useLocaleStore, SupportedLocale, LOCALE_LABELS } from '../store/useLocaleStore'
import { cn } from '../lib/utils'
import { menuItem, popoverSurface, sectionLabel, Z } from '../lib/ui'
import { IconButton } from './ui/IconButton'
import { shouldCloseOnEscape } from './keyboard'

const LOCALES: SupportedLocale[] = ['en', 'zh', 'ja', 'ko']

export function LanguageSwitcher(): React.ReactNode {
  const { t } = useTranslation()
  const { locale, setLocale } = useLocaleStore()
  const [open, setOpen] = useState(false)
  const [position, setPosition] = useState({ top: 0, left: 0 })
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
    if (!open) return
    updatePosition()
    const handleMouseDown = (e: MouseEvent): void => {
      const target = e.target as Node
      if (btnRef.current?.contains(target)) return
      if (panelRef.current?.contains(target)) return
      setOpen(false)
    }
    const handleKeyDown = (e: KeyboardEvent): void => {
      if (!shouldCloseOnEscape(e)) return
      // Claim the Escape so an enclosing dialog doesn't close with the menu.
      e.preventDefault()
      setOpen(false)
      btnRef.current?.focus()
    }
    window.addEventListener('resize', updatePosition)
    document.addEventListener('mousedown', handleMouseDown)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      window.removeEventListener('resize', updatePosition)
      document.removeEventListener('mousedown', handleMouseDown)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [open, updatePosition])

  return (
    <div className="relative">
      <IconButton
        ref={btnRef}
        size="md"
        bordered
        onClick={() => setOpen(!open)}
        aria-label={t('chrome.switchLanguage')}
        aria-expanded={open}
        data-testid="chrome-language-toggle"
      >
        <Languages size={13} />
      </IconButton>

      {open &&
        createPortal(
          <div
            ref={panelRef}
            role="menu"
            aria-label={t('chrome.language')}
            className={cn('fixed w-44 p-1', popoverSurface, Z.popover)}
            style={{ top: position.top, left: position.left }}
          >
            <div className={cn(sectionLabel, 'px-2 py-1')}>{t('chrome.language')}</div>
            {LOCALES.map((l) => {
              const isActive = locale === l
              const meta = LOCALE_LABELS[l]
              return (
                <button
                  key={l}
                  type="button"
                  role="menuitemradio"
                  aria-checked={isActive}
                  onClick={() => {
                    setLocale(l)
                    setOpen(false)
                  }}
                  className={menuItem(isActive)}
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
                  <span className="flex-1">{meta.native}</span>
                </button>
              )
            })}
          </div>,
          document.body
        )}
    </div>
  )
}
