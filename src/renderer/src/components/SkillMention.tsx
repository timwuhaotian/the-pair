import React, { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import Fuse from 'fuse.js'
import { Sparkles, RefreshCw, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

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
  const [position, setPosition] = useState({ top: 0, left: 0 })
  const [isRefreshing, setIsRefreshing] = useState(false)
  const popoverRef = useRef<HTMLDivElement>(null)

  const [fuse, setFuse] = useState<Fuse<SkillEntry> | null>(null)
  const skillsRef = useRef<SkillEntry[]>([])
  const resultsRef = useRef<SkillEntry[]>([])
  const selectedIndexRef = useRef(0)
  const isOpenRef = useRef(false)

  useEffect(() => {
    let cancelled = false
    window.api.skill.discover(projectDir).then((list) => {
      if (cancelled) return
      setSkills(list)
      skillsRef.current = list
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

  const getCursorPosition = useCallback((): { top: number; left: number } | null => {
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

    return {
      top: spanRect.top - 4,
      left: spanRect.right + 8
    }
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

      setTimeout(() => {
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
      setPosition(getCursorPosition() ?? { top: 0, left: 0 })
      setIsOpen(true)
      isOpenRef.current = true
      setSelectedIndex(0)
      selectedIndexRef.current = 0
    }

    const handleKeyDown = (e: KeyboardEvent): void => {
      if (!isOpenRef.current) return

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
        setSelectedIndex(newIndex)
        selectedIndexRef.current = newIndex
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        e.stopPropagation()
        if (resultsRef.current.length === 0) return
        const newIndex =
          (selectedIndexRef.current - 1 + resultsRef.current.length) % resultsRef.current.length
        setSelectedIndex(newIndex)
        selectedIndexRef.current = newIndex
      } else if ((e.key === 'Enter' || e.key === 'Tab') && resultsRef.current.length > 0) {
        e.preventDefault()
        e.stopPropagation()
        void insertMention(resultsRef.current[selectedIndexRef.current])
      }
    }

    const handleScroll = (): void => {
      if (isOpenRef.current) {
        setPosition(getCursorPosition() ?? { top: 0, left: 0 })
      }
    }

    textarea.addEventListener('input', handleInput)
    textarea.addEventListener('keydown', handleKeyDown)
    textarea.addEventListener('scroll', handleScroll)

    return () => {
      textarea.removeEventListener('input', handleInput)
      textarea.removeEventListener('keydown', handleKeyDown)
      textarea.removeEventListener('scroll', handleScroll)
    }
  }, [textareaRef, getCursorPosition, insertMention])

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

  if (!isOpen) return null

  return createPortal(
    <div
      ref={popoverRef}
      className="fixed z-[9999] flex max-h-72 w-96 flex-col overflow-hidden rounded-sm border border-border bg-popover font-mono"
      style={{ top: position.top, left: position.left }}
    >
      <div className="flex items-center justify-between border-b border-border px-2 py-1 text-[10px] uppercase tracking-[0.14em] text-muted-foreground">
        <span className="flex items-baseline gap-1.5">
          <Sparkles size={10} className="role-mentor translate-y-px" />
          {t('skills.popoverTitle')}
        </span>
        <button
          type="button"
          onMouseDown={(e) => {
            // Prevent the textarea from losing focus before refresh fires.
            e.preventDefault()
            void refreshSkills()
          }}
          disabled={isRefreshing}
          className="flex items-center gap-1 rounded-sm px-1 py-0.5 text-[10px] hover:bg-foreground/[0.06] disabled:opacity-50"
          title={t('skills.refresh')}
        >
          {isRefreshing ? <Loader2 size={10} className="animate-spin" /> : <RefreshCw size={10} />}
          <span>{t('skills.refresh')}</span>
        </button>
      </div>
      <div className="flex-1 overflow-y-auto scrollbar-thin">
        {results.length === 0 ? (
          <div className="px-3 py-4 text-center text-[11px] text-muted-foreground-faint">
            — {skills.length === 0 ? t('skills.empty') : t('skills.noMatch')} —
          </div>
        ) : (
          results.map((skill, index) => (
            <div
              key={skill.name}
              className={`flex cursor-pointer flex-col gap-0.5 px-2 py-1.5 text-[11px] ${
                index === selectedIndex ? 'bg-foreground/[0.08]' : 'hover:bg-foreground/[0.04]'
              }`}
              onMouseDown={(e) => {
                e.preventDefault()
                void insertMention(skill)
              }}
            >
              <div className="flex items-baseline gap-1.5">
                <span className="role-mentor select-none">/</span>
                <span className="truncate font-bold text-foreground/90">{skill.name}</span>
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
