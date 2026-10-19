import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import Fuse from 'fuse.js'
import { Sparkles, RefreshCw, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { Z, emptyHint, menuItem, popoverSurface } from '../lib/ui'
import { scrollActiveOptionIntoView } from '../lib/listbox'
import { GlassButton } from './ui/GlassButton'
import { createCompositionTracker, isImeKeyEvent } from './keyboard'
import {
  placeMentionPopup,
  shouldAcceptSkillOnEnter,
  type CaretLineRect,
  type MentionPopupPosition
} from './mentionText'

interface SkillEntry {
  name: string
  description: string
  source: string
}

interface SkillMentionProps {
  textareaRef: React.RefObject<HTMLTextAreaElement | null>
  onChange: (value: string) => void
  projectDir?: string
  onSkillSelect: (name: string, description: string, body: string) => void
}

/** Trigger when the current cursor sits at the end of a `/token` that begins
 * at start-of-input or right after whitespace. The token may be empty (just
 * a fresh `/`) and contain only kebab-friendly chars. */
const TRIGGER_RE = /(?:^|\s)\/([A-Za-z0-9_-]*)$/

export function SkillMention({
  textareaRef,
  onChange,
  projectDir,
  onSkillSelect
}: SkillMentionProps): React.ReactNode {
  const { t } = useTranslation()
  const [isOpen, setIsOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [skills, setSkills] = useState<SkillEntry[]>([])
  const [results, setResults] = useState<SkillEntry[]>([])
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [caret, setCaret] = useState<CaretLineRect | null>(null)
  const [position, setPosition] = useState<MentionPopupPosition>({ top: 0, left: 0 })
  const [isRefreshing, setIsRefreshing] = useState(false)
  const popoverRef = useRef<HTMLDivElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  // Pointer hover moves the highlight too, but must not scroll the list under
  // the cursor — only keyboard moves bring the active row into view.
  const pointerNavRef = useRef(false)
  const mentionTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => {
    return () => {
      if (mentionTimerRef.current) clearTimeout(mentionTimerRef.current)
    }
  }, [])

  const [fuse, setFuse] = useState<Fuse<SkillEntry> | null>(null)
  const skillsRef = useRef<SkillEntry[]>([])
  const resultsRef = useRef<SkillEntry[]>([])
  const selectedIndexRef = useRef(0)
  const isOpenRef = useRef(false)
  const queryRef = useRef('')
  // True once the user moved the highlight with the arrow keys.
  const navigatedRef = useRef(false)

  useEffect(() => {
    let cancelled = false
    window.api.skill
      .discover(projectDir)
      .then((list) => {
        if (cancelled) return
        setSkills(list)
        skillsRef.current = list
      })
      .catch((err: unknown) => {
        if (!cancelled) console.warn('[SkillMention] Failed to discover skills:', err)
      })
    return () => {
      cancelled = true
    }
  }, [projectDir])

  useEffect(() => {
    if (skills.length === 0) {
      setFuse(null)
      return
    }
    setFuse(
      new Fuse(skills, {
        keys: ['name', 'description'],
        threshold: 0.4,
        includeScore: true
      })
    )
  }, [skills])

  const refreshSkills = useCallback(async (): Promise<void> => {
    setIsRefreshing(true)
    try {
      const list = await window.api.skill.refresh(projectDir)
      setSkills(list)
      skillsRef.current = list
    } catch (err) {
      console.error('Failed to refresh skills:', err)
    } finally {
      setIsRefreshing(false)
    }
  }, [projectDir])

  const getCaretRect = useCallback((): CaretLineRect | null => {
    const textarea = textareaRef.current
    if (!textarea) return null

    const rect = textarea.getBoundingClientRect()
    const text = textarea.value
    const pos = textarea.selectionStart
    const textBeforeCursor = text.slice(0, pos)

    const mirror = document.createElement('div')
    const computed = window.getComputedStyle(textarea)

    mirror.style.cssText = `
      position: fixed;
      top: ${rect.top - textarea.scrollTop}px;
      left: ${rect.left - textarea.scrollLeft}px;
      visibility: hidden;
      white-space: pre-wrap;
      word-wrap: break-word;
      font: ${computed.font};
      padding: ${computed.padding};
      border: ${computed.border};
      width: ${computed.width};
      line-height: ${computed.lineHeight};
    `

    mirror.textContent = textBeforeCursor
    document.body.appendChild(mirror)

    const span = document.createElement('span')
    span.textContent = '/'
    mirror.appendChild(span)

    const spanRect = span.getBoundingClientRect()
    document.body.removeChild(mirror)

    return { top: spanRect.top, bottom: spanRect.bottom, left: spanRect.right + 8 }
  }, [textareaRef])

  const insertMention = useCallback(
    async (skill: SkillEntry): Promise<void> => {
      const textarea = textareaRef.current
      if (!textarea) return

      try {
        const content = await window.api.skill.readContent(skill.name, projectDir)
        onSkillSelect(skill.name, skill.description, content)
      } catch (err) {
        console.error('Failed to read skill content:', err)
        // Skip inserting — without body we can't safely attach context.
        setIsOpen(false)
        isOpenRef.current = false
        return
      }

      const text = textarea.value
      const pos = textarea.selectionStart
      const textBeforeCursor = text.slice(0, pos)
      const match = textBeforeCursor.match(TRIGGER_RE)
      if (!match) {
        setIsOpen(false)
        isOpenRef.current = false
        return
      }
      const slashIndex = textBeforeCursor.length - match[0].length + match[0].indexOf('/')
      const textBefore = text.slice(0, slashIndex)
      const textAfter = text.slice(pos)
      const newValue = `${textBefore}/${skill.name}${textAfter}`
      onChange(newValue)
      setIsOpen(false)
      isOpenRef.current = false

      mentionTimerRef.current = setTimeout(() => {
        const newPos = slashIndex + skill.name.length + 1
        textarea.focus()
        textarea.setSelectionRange(newPos, newPos)
      }, 0)
    },
    [textareaRef, onChange, onSkillSelect, projectDir]
  )

  useEffect(() => {
    const textarea = textareaRef.current
    if (!textarea) return

    const handleInput = (): void => {
      const text = textarea.value
      const pos = textarea.selectionStart
      const textBeforeCursor = text.slice(0, pos)
      const match = textBeforeCursor.match(TRIGGER_RE)

      if (!match) {
        setIsOpen(false)
        isOpenRef.current = false
        return
      }

      setQuery(match[1])
      queryRef.current = match[1]
      navigatedRef.current = false
      pointerNavRef.current = false
      setCaret(getCaretRect())
      setIsOpen(true)
      isOpenRef.current = true
      setSelectedIndex(0)
      selectedIndexRef.current = 0
    }

    const composition = createCompositionTracker()

    const handleKeyDown = (e: KeyboardEvent): void => {
      if (!isOpenRef.current) return
      // Keys used to drive an IME belong to the IME, not to the popover.
      if (composition.isComposing() || isImeKeyEvent(e)) return

      const hasModifier = e.metaKey || e.ctrlKey || e.altKey || e.shiftKey

      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        setIsOpen(false)
        isOpenRef.current = false
        return
      }

      if (hasModifier) return

      if (e.key === 'ArrowDown') {
        e.preventDefault()
        e.stopPropagation()
        if (resultsRef.current.length === 0) return
        const newIndex = (selectedIndexRef.current + 1) % resultsRef.current.length
        pointerNavRef.current = false
        setSelectedIndex(newIndex)
        selectedIndexRef.current = newIndex
        navigatedRef.current = true
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        e.stopPropagation()
        if (resultsRef.current.length === 0) return
        const newIndex =
          (selectedIndexRef.current - 1 + resultsRef.current.length) % resultsRef.current.length
        pointerNavRef.current = false
        setSelectedIndex(newIndex)
        selectedIndexRef.current = newIndex
        navigatedRef.current = true
      } else if (e.key === 'Tab' && resultsRef.current.length > 0) {
        e.preventDefault()
        e.stopPropagation()
        void insertMention(resultsRef.current[selectedIndexRef.current])
      } else if (e.key === 'Enter' && resultsRef.current.length > 0) {
        const skill = resultsRef.current[selectedIndexRef.current]
        if (!shouldAcceptSkillOnEnter(queryRef.current, skill.name, navigatedRef.current)) {
          // Not an unambiguous pick (e.g. a path like ` /tmp`): let Enter keep
          // its normal meaning instead of swapping the text for a fuzzy match.
          setIsOpen(false)
          isOpenRef.current = false
          return
        }
        e.preventDefault()
        e.stopPropagation()
        void insertMention(skill)
      }
    }

    const handleScroll = (): void => {
      if (isOpenRef.current) {
        setCaret(getCaretRect())
      }
    }

    // Popover interactions keep focus in the textarea (mousedown is prevented),
    // so a blur means the user left the input — close the popover.
    const handleBlur = (): void => {
      setIsOpen(false)
      isOpenRef.current = false
    }

    textarea.addEventListener('input', handleInput)
    textarea.addEventListener('keydown', handleKeyDown)
    textarea.addEventListener('scroll', handleScroll)
    textarea.addEventListener('blur', handleBlur)
    textarea.addEventListener('compositionstart', composition.onCompositionStart)
    textarea.addEventListener('compositionend', composition.onCompositionEnd)

    return () => {
      textarea.removeEventListener('input', handleInput)
      textarea.removeEventListener('keydown', handleKeyDown)
      textarea.removeEventListener('scroll', handleScroll)
      textarea.removeEventListener('blur', handleBlur)
      textarea.removeEventListener('compositionstart', composition.onCompositionStart)
      textarea.removeEventListener('compositionend', composition.onCompositionEnd)
      composition.dispose()
    }
  }, [textareaRef, getCaretRect, insertMention])

  useEffect(() => {
    if (skillsRef.current.length === 0) {
      setResults([])
      resultsRef.current = []
      return
    }
    if (!query || !fuse) {
      const initial = skillsRef.current.slice(0, 50)
      setResults(initial)
      resultsRef.current = initial
      return
    }
    const mapped = fuse
      .search(query)
      .slice(0, 50)
      .map((r) => r.item)
    setResults(mapped)
    resultsRef.current = mapped
  }, [query, fuse])

  // The popover is content-sized, so pick its side from the rendered height
  // (before paint) rather than the `max-h-72` cap.
  useLayoutEffect(() => {
    const popover = popoverRef.current
    if (!isOpen || !caret || !popover) return
    setPosition(placeMentionPopup(caret, window.innerHeight, popover.offsetHeight))
  }, [isOpen, caret, results, skills])

  // Declared after every other hook so the early return below stays legal.
  useEffect(() => {
    if (isOpen && !pointerNavRef.current) scrollActiveOptionIntoView(listRef.current)
  }, [isOpen, selectedIndex, results])

  if (!isOpen) return null

  // mousedown is prevented on the popover so focus stays in the textarea: its
  // blur handler would otherwise close the popover before an item click lands.
  return createPortal(
    <div
      ref={popoverRef}
      className={cn(
        'fixed flex max-h-72 w-96 flex-col overflow-hidden font-mono',
        Z.popover,
        popoverSurface
      )}
      style={position}
      onMouseDown={(e) => e.preventDefault()}
    >
      <div className="flex items-center justify-between border-b border-border py-1 pl-2 pr-1 text-[10px] uppercase tracking-[0.14em] text-muted-foreground">
        <span className="flex items-center gap-1.5">
          <Sparkles size={11} aria-hidden />
          {t('skills.popoverTitle')}
        </span>
        <GlassButton
          variant="ghost"
          size="xs"
          onClick={() => void refreshSkills()}
          disabled={isRefreshing}
          icon={
            isRefreshing ? <Loader2 size={11} className="animate-spin" /> : <RefreshCw size={11} />
          }
          title={t('skills.refresh')}
        >
          {t('skills.refresh')}
        </GlassButton>
      </div>
      <div
        ref={listRef}
        role="listbox"
        aria-label={t('skills.popoverTitle')}
        className="flex-1 overflow-y-auto p-1 scrollbar-thin"
      >
        {results.length === 0 ? (
          <div className={emptyHint}>
            {skills.length === 0 ? t('skills.empty') : t('skills.noMatch')}
          </div>
        ) : (
          results.map((skill, index) => (
            <div
              key={skill.name}
              role="option"
              aria-selected={index === selectedIndex}
              className={cn(menuItem(index === selectedIndex), 'flex-col items-stretch gap-0.5')}
              onMouseEnter={() => {
                pointerNavRef.current = true
                setSelectedIndex(index)
                selectedIndexRef.current = index
              }}
              onMouseDown={(e) => {
                e.preventDefault()
                void insertMention(skill)
              }}
            >
              <div className="flex items-baseline gap-1.5">
                <span className="select-none text-muted-foreground">/</span>
                <span className="truncate font-bold">{skill.name}</span>
              </div>
              <div className="line-clamp-2 pl-[2ch] text-[10px] text-muted-foreground">
                {skill.description}
              </div>
            </div>
          ))
        )}
      </div>
      {skills.length > results.length && results.length === 50 && (
        <div className="border-t border-border px-2 py-1 text-[10px] text-muted-foreground-faint">
          · {skills.length - results.length} {t('skills.moreSkills')}
        </div>
      )}
    </div>,
    document.body
  )
}
