import React from 'react'
import { cn } from '../../lib/utils'
import { fieldLabel, hoverTint } from '../../lib/ui'

interface ToggleRowProps {
  checked: boolean
  onToggle: () => void
  label: string
  hint: string
  testId: string
}

/** A checkbox row: label plus a one-line explainer. */
export function ToggleRow({
  checked,
  onToggle,
  label,
  hint,
  testId
}: ToggleRowProps): React.ReactNode {
  return (
    <button
      type="button"
      onClick={onToggle}
      aria-pressed={checked}
      data-testid={testId}
      className={cn(
        '-mx-1.5 flex cursor-pointer items-start gap-2 rounded-sm px-1.5 py-1 text-left transition-colors',
        hoverTint
      )}
    >
      <span
        aria-hidden
        className={cn(
          'mt-px select-none font-mono text-[12px]',
          checked ? 'state-running' : 'text-muted-foreground-faint'
        )}
      >
        {checked ? '[x]' : '[ ]'}
      </span>
      <span className="flex flex-col gap-0.5">
        <span className={fieldLabel}>{label}</span>
        <span className="text-[11px] normal-case leading-snug text-muted-foreground-faint">
          {hint}
        </span>
      </span>
    </button>
  )
}
