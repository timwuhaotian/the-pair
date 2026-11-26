import React, { useState, useRef, useEffect, useCallback, useId } from 'react'
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
  // The item that holds the menu's single Tab stop (roving tabindex).
  const [focusIndex, setFocusIndex] = useState(0)
  const btnRef = useRef<HTMLButtonElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([])
  const menuId = useId()

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

  const openMenu = (): void => {
    // Opening lands on the language in use, the way a native menu does.
    setFocusIndex(Math.max(0, LOCALES.indexOf(locale)))
    setOpen(true)
  }

  const closeMenu = (): void => {
    setOpen(false)
    btnRef.current?.focus()
  }

  // Focus follows the roving index while the menu is open (and on open).
  useEffect(() => {
    if (open) itemRefs.current[focusIndex]?.focus()
  }, [open, focusIndex])

  const handleMenuKeyDown = (e: React.KeyboardEvent<HTMLDivElement>): void => {
    const last = LOCALES.length - 1
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault()
        setFocusIndex((i) => (i >= last ? 0 : i + 1))
        break
      case 'ArrowUp':
        e.preventDefault()
        setFocusIndex((i) => (i <= 0 ? last : i - 1))
        break
      case 'Home':
        e.preventDefault()
        setFocusIndex(0)
        break
      case 'End':
        e.preventDefault()
        setFocusIndex(last)
        break
      case 'Tab':
        // Tab leaves a menu: close it from the toggle, and let the browser
        // carry on from there to the next control.
        setOpen(false)
        btnRef.current?.focus()
        break
    }
  }

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
    <div className="relative flex">
      <IconButton
        ref={btnRef}
        size="md"
        bordered
        onClick={() => (open ? setOpen(false) : openMenu())}
        aria-label={t('chrome.switchLanguage')}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        data-testid="chrome-language-toggle"
      >
        <Languages size={13} />
      </IconButton>

      {open &&
        createPortal(
          <div
            ref={panelRef}
            id={menuId}
            role="menu"
            aria-label={t('chrome.language')}
            onKeyDown={handleMenuKeyDown}
            className={cn(
              'fixed min-w-44 w-max max-w-[calc(100vw-16px)] p-1',
              popoverSurface,
              Z.popover
            )}
            style={{ top: position.top, left: position.left }}
          >
            {/* The menu is already named by this label; the visible copy is decoration. */}
            <div aria-hidden className={cn(sectionLabel, 'px-2 py-1')}>
              {t('chrome.language')}
            </div>
            {LOCALES.map((l, index) => {
              const isActive = locale === l
              const meta = LOCALE_LABELS[l]
              return (
                <button
                  key={l}
                  ref={(node) => {
                    itemRefs.current[index] = node
                  }}
                  type="button"
                  role="menuitemradio"
                  aria-checked={isActive}
                  tabIndex={index === focusIndex ? 0 : -1}
                  onClick={() => {
                    setLocale(l)
                    closeMenu()
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
                  <span aria-hidden className="text-base leading-none">
                    {meta.flag}
                  </span>
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
