import { useState, useEffect, useRef, useLayoutEffect, useCallback, useId } from 'react'
import { createPortal } from 'react-dom'
import { Sparkles, RefreshCw, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { Z, emptyHint, menuItem, popoverSurface } from '../lib/ui'
import { IconButton } from './ui/IconButton'
import { isImeKeyEvent } from './keyboard'
import { scrollActiveOptionIntoView } from '../lib/listbox'

const TABBABLE_SELECTOR =
  'button:not([disabled]):not([tabindex="-1"]), input:not([disabled]):not([tabindex="-1"]), [tabindex]:not([tabindex="-1"])'

/** True when this Tab would carry focus past the menu's first or last control. */
function tabLeavesMenu(e: React.KeyboardEvent, menu: HTMLElement | null): boolean {
  if (e.key !== 'Tab' || !menu) return false
  const nodes = menu.querySelectorAll<HTMLElement>(TABBABLE_SELECTOR)
  if (nodes.length === 0) return true
  return document.activeElement === (e.shiftKey ? nodes[0] : nodes[nodes.length - 1])
}

interface SkillInfo {
  name: string
  description: string
  source: string
}

interface SkillPickerProps {
  projectDir: string
  onSelect: (skillName: string) => void
}

export function SkillPicker({ projectDir, onSelect }: SkillPickerProps): React.ReactNode {
  const { t } = useTranslation()
  const [isOpen, setIsOpen] = useState(false)
  const [skills, setSkills] = useState<SkillInfo[]>([])
  const [loading, setLoading] = useState(false)
  const [filter, setFilter] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const buttonRef = useRef<HTMLButtonElement>(null)
  const dropdownRef = useRef<HTMLDivElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const listId = useId()
  const optionId = (index: number): string => `${listId}-option-${index}`
  const [menuPos, setMenuPos] = useState({ top: 0, left: 0 })

  const loadSkills = async (): Promise<void> => {
    setLoading(true)
    try {
      const result = await window.api.skill.discover(projectDir || undefined)
      setSkills(result)
    } catch (error) {
      console.error('Failed to discover skills:', error)
      setSkills([])
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    if (isOpen && skills.length === 0 && !loading) {
      void loadSkills()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen])

  // The menu lives in a portal so modal scroll containers never clip it: it
  // is anchored to the trigger and flips up when the viewport runs out.
  const placeMenu = useCallback(() => {
    const trigger = buttonRef.current
    const menu = dropdownRef.current
    if (!trigger || !menu) return
    const rect = trigger.getBoundingClientRect()
    const gap = 4
    const menuWidth = 320
    const height = menu.offsetHeight
    const fitsBelow = rect.bottom + gap + height <= window.innerHeight - 8
    const fitsAbove = rect.top - gap - height >= 8
    const openUp = !fitsBelow && (fitsAbove || rect.top > window.innerHeight - rect.bottom)
    setMenuPos({
      top: openUp ? Math.max(8, rect.top - gap - height) : rect.bottom + gap,
      left: Math.max(8, Math.min(rect.right - menuWidth, window.innerWidth - menuWidth - 8))
    })
  }, [])

  useLayoutEffect(() => {
    if (isOpen) placeMenu()
  }, [isOpen, filter, skills.length, placeMenu])

  useEffect(() => {
    if (!isOpen) return
    const handleClickOutside = (e: MouseEvent): void => {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(e.target as Node) &&
        buttonRef.current &&
        !buttonRef.current.contains(e.target as Node)
      ) {
        setIsOpen(false)
      }
    }
    const handleViewport = (): void => placeMenu()
    document.addEventListener('mousedown', handleClickOutside)
    window.addEventListener('resize', handleViewport)
    window.addEventListener('scroll', handleViewport, true)
    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
      window.removeEventListener('resize', handleViewport)
      window.removeEventListener('scroll', handleViewport, true)
    }
  }, [isOpen, placeMenu])

  const filteredSkills = skills.filter(
    (s) =>
      s.name.toLowerCase().includes(filter.toLowerCase()) ||
      s.description.toLowerCase().includes(filter.toLowerCase())
  )

  // Clamped during render: a refresh or a narrower filter never leaves the
  // highlight past the end of the list.
  const activeIndex =
    filteredSkills.length === 0 ? -1 : Math.min(selectedIndex, filteredSkills.length - 1)

  /** Close the menu; focus inside it goes back to the trigger instead of <body>. */
  const closeMenu = (): void => {
    if (dropdownRef.current?.contains(document.activeElement)) buttonRef.current?.focus()
    setIsOpen(false)
  }

  const handleSelect = (skill: SkillInfo): void => {
    onSelect(skill.name)
    closeMenu()
    setFilter('')
    setSelectedIndex(0)
  }

  const moveHighlight = (index: number): void => {
    setSelectedIndex(Math.max(0, Math.min(index, filteredSkills.length - 1)))
    requestAnimationFrame(() => scrollActiveOptionIntoView(listRef.current))
  }

  const handleKeyDown = (e: React.KeyboardEvent): void => {
    if (isImeKeyEvent(e)) {
      // WebKit reports the IME-confirming Enter as Enter: keep it from submitting the form.
      if (e.key === 'Enter') e.preventDefault()
      return
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      moveHighlight(activeIndex + 1)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      moveHighlight(activeIndex - 1)
    } else if (e.key === 'Home') {
      e.preventDefault()
      moveHighlight(0)
    } else if (e.key === 'End') {
      e.preventDefault()
      moveHighlight(filteredSkills.length - 1)
    } else if (e.key === 'Enter') {
      // Enter selects the highlighted skill; with no match it must still not
      // submit the enclosing modal form.
      e.preventDefault()
      e.stopPropagation()
      if (filteredSkills[activeIndex]) handleSelect(filteredSkills[activeIndex])
    }
  }

  // Escape inside the open dropdown closes only the dropdown, never the enclosing modal.
  const handleContainerKeyDown = (e: React.KeyboardEvent<HTMLDivElement>): void => {
    if (!isOpen || isImeKeyEvent(e)) return
    if (tabLeavesMenu(e, dropdownRef.current)) {
      // Tab out of the menu closes it from the trigger; forward Tab then
      // carries on from the trigger, Shift+Tab stops on it.
      if (e.shiftKey) e.preventDefault()
      closeMenu()
      return
    }
    if (e.key !== 'Escape') return
    e.preventDefault()
    e.stopPropagation()
    setIsOpen(false)
    buttonRef.current?.focus()
  }

  return (
    <div className="relative" onKeyDown={handleContainerKeyDown}>
      <IconButton
        ref={buttonRef}
        onClick={() => setIsOpen(!isOpen)}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        aria-label={t('skills.add')}
      >
        <Sparkles size={13} />
      </IconButton>

      {isOpen &&
        createPortal(
          <div
            ref={dropdownRef}
            data-modal-popover
            onKeyDown={handleContainerKeyDown}
            className={cn(
              'fixed w-80 max-h-96 overflow-hidden text-[12px]',
              Z.popover,
              popoverSurface
            )}
            style={{ top: menuPos.top, left: menuPos.left }}
          >
            <div className="px-2 py-1.5 border-b border-border flex items-center gap-2">
              <span aria-hidden className="text-muted-foreground-faint select-none">
                ·
              </span>
              <input
                type="text"
                value={filter}
                aria-label={t('skills.search')}
                role="combobox"
                aria-autocomplete="list"
                aria-expanded={filteredSkills.length > 0}
                aria-controls={listId}
                aria-activedescendant={activeIndex >= 0 ? optionId(activeIndex) : undefined}
                onChange={(e) => {
                  setFilter(e.target.value)
                  setSelectedIndex(0)
                }}
                onKeyDown={handleKeyDown}
                placeholder={t('skills.search')}
                className="flex-1 bg-transparent text-[12px] text-foreground outline-none placeholder:text-muted-foreground-faint"
                autoFocus
              />
              <IconButton
                aria-label={t('skills.refresh')}
                onClick={() => void loadSkills()}
                disabled={loading}
                className="h-6 w-6"
              >
                {loading ? (
                  <Loader2 size={12} className="animate-spin text-muted-foreground" />
                ) : (
                  <RefreshCw size={11} />
                )}
              </IconButton>
            </div>

            <div className="max-h-80 overflow-y-auto scrollbar-thin">
              {loading && skills.length === 0 ? (
                <div className={cn(emptyHint, 'flex items-center justify-center gap-2')}>
                  <Loader2 size={12} className="animate-spin text-muted-foreground" />
                  {t('skills.discovering')}
                </div>
              ) : filteredSkills.length === 0 ? (
                <div className={emptyHint}>
                  — {filter ? t('skills.noMatch') : t('skills.empty')} —
                </div>
              ) : (
                <div
                  ref={listRef}
                  id={listId}
                  role="listbox"
                  aria-label={t('skills.popoverTitle')}
                  className="p-1"
                >
                  {filteredSkills.map((skill, index) => (
                    <button
                      key={skill.name}
                      id={optionId(index)}
                      type="button"
                      role="option"
                      aria-selected={index === activeIndex}
                      // Focus stays in the search field; the highlighted row is
                      // its aria-activedescendant, so rows are not Tab stops.
                      tabIndex={-1}
                      onClick={() => handleSelect(skill)}
                      // Pointer and keyboard share one highlight: hovering moves the active row.
                      onMouseEnter={() => setSelectedIndex(index)}
                      className={cn(
                        menuItem(index === activeIndex),
                        'flex-col items-start gap-0.5 py-1.5'
                      )}
                    >
                      <span className="flex items-baseline gap-1.5">
                        <span aria-hidden className="role-mentor select-none">
                          ▸
                        </span>
                        <span
                          className="min-w-0 flex-1 truncate font-medium text-foreground/90 [overflow-wrap:anywhere]"
                          title={skill.name}
                        >
                          {skill.name}
                        </span>
                      </span>
                      <span className="line-clamp-2 pl-[2ch] text-[10px] text-muted-foreground">
                        {skill.description}
                      </span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>,
          document.body
        )}
    </div>
  )
}
