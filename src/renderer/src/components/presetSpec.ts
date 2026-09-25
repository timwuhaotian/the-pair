/**
 * Helpers for putting a preset's mentor template around the task typed in the
 * Create Pair / Assign Task modals, and for taking it off again. The modals
 * track which preset's template currently wraps the textarea (instead of
 * sniffing for a marker string), so a spec is never wrapped twice.
 */
import { buildSpecFromPreset } from '../lib/presetUtils'
import type { PairPreset } from '../types'

const TASK_SLOT = '{task}'

interface TemplateParts {
  prefix: string
  suffix: string
}

function templateParts(template: string): TemplateParts | null {
  const index = template.indexOf(TASK_SLOT)
  if (index === -1) return null
  return { prefix: template.slice(0, index), suffix: template.slice(index + TASK_SLOT.length) }
}

function middleOf(spec: string, parts: TemplateParts): string | null {
  const text = spec.trim()
  const prefix = parts.prefix.trimStart()
  const suffix = parts.suffix.trimEnd()
  if (text.length < prefix.length + suffix.length) return null
  if (!text.startsWith(prefix) || !text.endsWith(suffix)) return null
  return text.slice(prefix.length, text.length - suffix.length).trim()
}

/** Whether the preset's template has a `{task}` slot the user must fill in. */
export function presetNeedsTask(preset: PairPreset): boolean {
  return preset.mentorPromptTemplate.includes(TASK_SLOT)
}

/** Wraps `task` in the preset's mentor template (empty task → template placeholder). */
export function wrapInPresetTemplate(preset: PairPreset, task: string): string {
  return buildSpecFromPreset(preset, task)
}

/**
 * Returns the task text inside a spec produced by {@link wrapInPresetTemplate}
 * — `''` when only the template's placeholder is there — or `null` when the
 * spec no longer has the template's shape (the user edited the template text).
 */
export function extractPresetTask(preset: PairPreset, spec: string): string | null {
  const parts = templateParts(preset.mentorPromptTemplate)
  if (!parts) return spec.trim() === preset.mentorPromptTemplate.trim() ? '' : null
  const task = middleOf(spec, parts)
  if (task === null) return null
  const placeholder = middleOf(buildSpecFromPreset(preset, ''), parts)
  return placeholder !== null && task === placeholder ? '' : task
}

/** Removes the preset's template, keeping the user's task. Leaves an edited template untouched. */
export function removePresetTemplate(preset: PairPreset, spec: string): string {
  return extractPresetTask(preset, spec) ?? spec
}

/**
 * Spec after switching the textarea from `previous` (the preset whose template
 * currently wraps it, if any) to `next`, carrying the typed task across.
 */
export function switchPresetTemplate(
  previous: PairPreset | null,
  next: PairPreset,
  spec: string
): string {
  const task = previous ? (extractPresetTask(previous, spec) ?? '') : spec.trim()
  return wrapInPresetTemplate(next, task)
}

/** True when a `{task}` template is applied but the user hasn't typed a task into it yet. */
export function isPresetTaskMissing(preset: PairPreset | null, spec: string): boolean {
  if (!preset || !presetNeedsTask(preset)) return false
  return extractPresetTask(preset, spec) === ''
}

/**
 * Whether any recognizable part of the preset's template is still in `spec`: the
 * first line before `{task}` or the last line after it. Lenient on purpose, so a
 * template the user edited still counts as present and is not wrapped again.
 */
function templateStillPresent(preset: PairPreset, spec: string): boolean {
  const parts = templateParts(preset.mentorPromptTemplate)
  if (!parts) return true
  const firstLine = parts.prefix
    .split('\n')
    .map((line) => line.trim())
    .find((line) => line.length > 0)
  const lastLine = parts.suffix
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .pop()
  return [firstLine, lastLine].some((line) => line !== undefined && spec.includes(line))
}

/**
 * Final spec to submit. Wraps when the selected preset's template is not applied
 * to the textarea — including when the user replaced the applied template's text
 * wholesale (select-all + type), which would otherwise drop the preset silently.
 */
export function finalizePresetSpec(
  selected: PairPreset | null,
  applied: PairPreset | null,
  spec: string
): string {
  if (!selected) return spec
  if (applied?.id === selected.id && templateStillPresent(selected, spec)) return spec
  return wrapInPresetTemplate(selected, spec)
}
