import React, { useState, useEffect, useLayoutEffect, useRef, useCallback } from 'react'
import { createPortal } from 'react-dom'
import Fuse from 'fuse.js'
import { File, Folder } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { Z, emptyHint, menuItem, popoverSurface } from '../lib/ui'
import { createCompositionTracker, isImeKeyEvent } from './keyboard'
import {
  findFileMentionQuery,
  placeMentionPopup,
  replaceFileMentionToken,
  type CaretLineRect,
  type MentionPopupPosition
} from './mentionText'
import { scrollActiveOptionIntoView } from '../lib/listbox'

interface FileEntry {
  path: string
  type: 'file' | 'directory'
}

interface FileMentionProps {
  textareaRef: React.RefObject<HTMLTextAreaElement | null>
  onChange: (value: string) => void
  directory: string
  pairId?: string
  onFileSelect?: (path: string, content: string) => void
}

export function FileMention({
  textareaRef,
  onChange,
  directory,
  pairId,
  onFileSelect
}: FileMentionProps): React.ReactNode {
  const { t } = useTranslation()
  const [isOpen, setIsOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<FileEntry[]>([])
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [caret, setCaret] = useState<CaretLineRect | null>(null)
  const [position, setPosition] = useState<MentionPopupPosition>({ top: 0, left: 0 })
  const [files, setFiles] = useState<FileEntry[]>([])
  // Which listing `files` came from and whether it failed, so the empty hint can
  // tell "still loading" and "couldn't list" apart from an empty repo.
  const listingKey = `${pairId ?? ''}|${directory}`
  const [listing, setListing] = useState<{ key: string; failed: boolean } | null>(null)
  const popoverRef = useRef<HTMLDivElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  // Pointer hover moves the highlight too, but must not scroll the list under
  // the cursor — only keyboard moves bring the active row into view.
  const pointerNavRef = useRef(false)

  const [fuse, setFuse] = useState<Fuse<FileEntry> | null>(null)
  const filesRef = useRef<FileEntry[]>([])
  const resultsRef = useRef<FileEntry[]>([])
  const selectedIndexRef = useRef(0)
  const isOpenRef = useRef(false)

  useEffect(() => {
    if (!directory) return

    let cancelled = false
    window.api.file
      .listFiles({ pairId, directory })
      .then((fileList) => {
        if (cancelled) return
        setFiles(fileList)
        filesRef.current = fileList
        setListing({ key: listingKey, failed: false })
      })
      .catch((err: unknown) => {
        if (cancelled) return
        console.warn('[FileMention] Failed to list files:', err)
        setFiles([])
        filesRef.current = []
        setListing({ key: listingKey, failed: true })
      })
    return () => {
      cancelled = true
    }
  }, [directory, pairId, listingKey])

  useEffect(() => {
    if (files.length === 0) return

    setFuse(
      new Fuse(files, {
        keys: ['path'],
        threshold: 0.4,
        includeScore: true
      })
    )
  }, [files])

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
    span.textContent = '@'
    mirror.appendChild(span)

    const spanRect = span.getBoundingClientRect()
    document.body.removeChild(mirror)

    return { top: spanRect.top, bottom: spanRect.bottom, left: spanRect.right + 8 }
  }, [textareaRef])

  const insertMention = useCallback(
    async (path: string): Promise<void> => {
      const textarea = textareaRef.current
      if (!textarea) return

      if (onFileSelect) {
        try {
          const content = await window.api.file.readContent({
            pairId: pairId,
            directory: pairId ? undefined : directory,
            filePath: path
          })
          onFileSelect(path, content)
        } catch (err) {
          console.error('Failed to read file content:', err)
        }
      }

      setIsOpen(false)
      isOpenRef.current = false

      // The text may have changed while the file was read; only replace the
      // `@query` token that is still in front of the cursor.
      const replacement = replaceFileMentionToken(textarea.value, textarea.selectionStart, path)
      if (!replacement) return
      onChange(replacement.value)

      setTimeout(() => {
        textarea.focus()
        textarea.setSelectionRange(replacement.cursor, replacement.cursor)
      }, 0)
    },
    [textareaRef, onChange, onFileSelect, pairId, directory]
  )

  useEffect(() => {
    const textarea = textareaRef.current
    if (!textarea) return

    const handleInput = (): void => {
      const text = textarea.value
      const pos = textarea.selectionStart
      const mentionQuery = findFileMentionQuery(text.slice(0, pos))

      if (mentionQuery === null) {
        setIsOpen(false)
        isOpenRef.current = false
        return
      }

      setQuery(mentionQuery)
      setCaret(getCaretRect())
      setIsOpen(true)
      isOpenRef.current = true
      pointerNavRef.current = false
      setSelectedIndex(0)
      selectedIndexRef.current = 0
    }

    const composition = createCompositionTracker()

    const handleKeyDown = (e: KeyboardEvent): void => {
      if (!isOpenRef.current) return
      // Keys used to drive an IME (Enter to confirm, arrows to pick a candidate)
      // belong to the IME, not to the popover.
      if (composition.isComposing() || isImeKeyEvent(e)) return

      const hasModifier = e.metaKey || e.ctrlKey || e.altKey || e.shiftKey

      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        setIsOpen(false)
        isOpenRef.current = false
        return
      }

      if (hasModifier) {
        return
      }

      if (e.key === 'ArrowDown') {
        e.preventDefault()
        e.stopPropagation()
        if (resultsRef.current.length === 0) return
        const newIndex = (selectedIndexRef.current + 1) % resultsRef.current.length
        pointerNavRef.current = false
        setSelectedIndex(newIndex)
        selectedIndexRef.current = newIndex
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        e.stopPropagation()
        if (resultsRef.current.length === 0) return
        const newIndex =
          (selectedIndexRef.current - 1 + resultsRef.current.length) % resultsRef.current.length
        pointerNavRef.current = false
        setSelectedIndex(newIndex)
        selectedIndexRef.current = newIndex
      } else if (e.key === 'Enter' && resultsRef.current.length > 0) {
        e.preventDefault()
        e.stopPropagation()
        void insertMention(resultsRef.current[selectedIndexRef.current].path)
      } else if (e.key === 'Tab' && resultsRef.current.length > 0) {
        e.preventDefault()
        e.stopPropagation()
        void insertMention(resultsRef.current[selectedIndexRef.current].path)
      }
    }

    // Popover items keep focus in the textarea (mousedown is prevented), so a
    // blur means the user left the input — don't leave the popover hanging.
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

    function handleScroll(): void {
      if (isOpenRef.current) {
        setCaret(getCaretRect())
      }
    }
  }, [textareaRef, getCaretRect, insertMention])

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined
    if (!query || !fuse) {
      timer = setTimeout(() => {
        const initialResults = filesRef.current.slice(0, 50)
        setResults(initialResults)
        resultsRef.current = initialResults
      }, 0)
      return () => {
        if (timer) clearTimeout(timer)
      }
    }

    const searchResults = fuse.search(query).slice(0, 50)
    timer = setTimeout(() => {
      const mappedResults = searchResults.map((r) => r.item)
      setResults(mappedResults)
      resultsRef.current = mappedResults
    }, 0)
    return () => {
      if (timer) clearTimeout(timer)
    }
  }, [query, fuse])

  // The popover is content-sized, so pick its side from the rendered height
  // (before paint) rather than the `max-h-72` cap.
  useLayoutEffect(() => {
    const popover = popoverRef.current
    if (!isOpen || !caret || !popover) return
    setPosition(placeMentionPopup(caret, window.innerHeight, popover.offsetHeight))
  }, [isOpen, caret, results])

  useEffect(() => {
    if (isOpen && !pointerNavRef.current) scrollActiveOptionIntoView(listRef.current)
  }, [isOpen, selectedIndex, results])

  if (!isOpen) return null

  const emptyText =
    directory && listing?.key !== listingKey
      ? t('console.mentionLoading')
      : directory && listing?.failed
        ? t('console.mentionLoadFailed')
        : files.length === 0
          ? t('console.mentionNoFiles')
          : t('console.mentionNoMatch')

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
      <div
        ref={listRef}
        role="listbox"
        aria-label={t('console.mentionFiles')}
        className="flex-1 overflow-y-auto p-1 scrollbar-thin"
      >
        {results.length === 0 ? (
          <div className={emptyHint}>{emptyText}</div>
        ) : (
          results.map((file, index) => (
            <div
              key={file.path}
              role="option"
              aria-selected={index === selectedIndex}
              className={menuItem(index === selectedIndex)}
              onMouseEnter={() => {
                pointerNavRef.current = true
                setSelectedIndex(index)
                selectedIndexRef.current = index
              }}
              onMouseDown={(e) => {
                e.preventDefault()
                void insertMention(file.path)
              }}
            >
              {file.type === 'directory' ? (
                <Folder size={12} aria-hidden className="shrink-0 text-muted-foreground" />
              ) : (
                <File size={12} aria-hidden className="shrink-0 text-muted-foreground" />
              )}
              <span className="truncate">{file.path}</span>
            </div>
          ))
        )}
      </div>
      {files.length > results.length && results.length === 50 && (
        <div className="border-t border-border px-2 py-1 text-[10px] text-muted-foreground-faint">
          {t('console.moreFiles', { count: files.length - results.length })}
        </div>
      )}
    </div>,
    document.body
  )
}
