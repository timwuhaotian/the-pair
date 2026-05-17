import { useState, useEffect, useRef } from 'react'
import { Sparkles, RefreshCw, Loader2 } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { cn } from '../lib/utils'

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
  const [isOpen, setIsOpen] = useState(false)
  const [skills, setSkills] = useState<SkillInfo[]>([])
  const [loading, setLoading] = useState(false)
  const [filter, setFilter] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const buttonRef = useRef<HTMLButtonElement>(null)
  const dropdownRef = useRef<HTMLDivElement>(null)

  const loadSkills = async (): Promise<void> => {
    setLoading(true)
    try {
      const result = await invoke<SkillInfo[]>('discover_skills', {
        projectDir: projectDir || null
      })
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
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [isOpen])

  const filteredSkills = skills.filter(
    (s) =>
      s.name.toLowerCase().includes(filter.toLowerCase()) ||
      s.description.toLowerCase().includes(filter.toLowerCase())
  )

  const handleSelect = (skill: SkillInfo): void => {
    onSelect(skill.name)
    setIsOpen(false)
    setFilter('')
    setSelectedIndex(0)
  }

  const handleKeyDown = (e: React.KeyboardEvent): void => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSelectedIndex((i) => Math.min(i + 1, filteredSkills.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSelectedIndex((i) => Math.max(i - 1, 0))
    } else if (e.key === 'Enter' && filteredSkills[selectedIndex]) {
      e.preventDefault()
      handleSelect(filteredSkills[selectedIndex])
    } else if (e.key === 'Escape') {
      setIsOpen(false)
    }
  }

  return (
    <div className="relative">
      <button
        ref={buttonRef}
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        className="p-1 rounded-sm text-muted-foreground hover:text-foreground hover:bg-foreground/[0.06] transition-colors cursor-pointer"
        title="add skill"
      >
        <Sparkles size={13} />
      </button>

      {isOpen && (
        <div
          ref={dropdownRef}
          className="absolute top-full right-0 mt-1 w-80 max-h-96 overflow-hidden rounded-sm border border-border bg-popover z-50 font-mono text-[12px]"
        >
          <div className="px-2 py-1.5 border-b border-border flex items-center gap-2">
            <span aria-hidden className="text-muted-foreground-faint select-none">
              ·
            </span>
            <input
              type="text"
              value={filter}
              onChange={(e) => {
                setFilter(e.target.value)
                setSelectedIndex(0)
              }}
              onKeyDown={handleKeyDown}
              placeholder="search skills…"
              className="flex-1 bg-transparent text-[12px] outline-none placeholder:text-muted-foreground-faint"
              autoFocus
            />
            <button
              type="button"
              onClick={() => void loadSkills()}
              disabled={loading}
              className="p-0.5 rounded-sm hover:bg-foreground/[0.06] transition-colors disabled:opacity-50 cursor-pointer"
              title="refresh"
            >
              {loading ? <Loader2 size={11} className="animate-spin" /> : <RefreshCw size={11} />}
            </button>
          </div>

          <div className="max-h-80 overflow-y-auto scrollbar-thin">
            {loading && skills.length === 0 ? (
              <div className="p-3 text-center text-[11px] text-muted-foreground">
                <Loader2 size={12} className="animate-spin mx-auto mb-1" />· discovering skills…
              </div>
            ) : filteredSkills.length === 0 ? (
              <div className="p-3 text-center text-[11px] text-muted-foreground-faint">
                — {filter ? 'no skills match' : 'no skills found'} —
              </div>
            ) : (
              filteredSkills.map((skill, index) => (
                <button
                  key={skill.name}
                  type="button"
                  onClick={() => handleSelect(skill)}
                  className={cn(
                    'w-full text-left px-3 py-2 border-b border-border/40 last:border-0 transition-colors cursor-pointer',
                    index === selectedIndex ? 'bg-foreground/[0.06]' : 'hover:bg-foreground/[0.04]'
                  )}
                >
                  <div className="flex items-baseline gap-1.5">
                    <span aria-hidden className="role-mentor select-none">
                      ▸
                    </span>
                    <span className="font-bold text-foreground/90">{skill.name}</span>
                  </div>
                  <div className="text-[10px] text-muted-foreground mt-0.5 line-clamp-2 pl-[2ch]">
                    {skill.description}
                  </div>
                </button>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  )
}
