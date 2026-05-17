/**
 * Helpers for `/`-mentioned skills attached to a task spec.
 *
 * Skills come from local SKILL.md folders (Claude Code / opencode / agents)
 * and are injected ahead of the user's spec before assignment. Claude Code
 * executors receive a short reference directive — their built-in Skill tool
 * loads the actual body. Other providers (opencode/codex/gemini) get the full
 * SKILL.md body inlined as context since they don't have a Skill primitive.
 */

import { selectReferencedFiles, type FileContexts } from './fileMentions'
import type { ProviderKind } from '../types'

export interface SkillContextEntry {
  description: string
  body: string
}

export type SkillContexts = Map<string, SkillContextEntry>

/** Builds a regex that matches `/name` only at start of input or after whitespace,
 * with a non-skill char (or end-of-string) following the name so we don't pick
 * up `/foo` inside `/foobar` or paths like `/foo/bar`. */
function tokenMatcher(name: string): RegExp {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  return new RegExp(`(^|[\\s\\[(])/${escaped}(?![A-Za-z0-9_-])`)
}

/** Returns only skills whose `/name` mention still appears in the spec. The user
 * may have deleted a mention after selecting it; we drop those before sending. */
export function selectReferencedSkills(
  spec: string,
  skillContexts: SkillContexts
): Array<[string, SkillContextEntry]> {
  return Array.from(skillContexts.entries()).filter(([name]) => tokenMatcher(name).test(spec))
}

function formatSkillBlock(
  skills: Array<[string, SkillContextEntry]>,
  executorProvider: ProviderKind
): string {
  if (executorProvider === 'claude') {
    const names = skills.map(([n]) => `/${n}`).join(', ')
    return [
      '--- SKILLS REQUESTED ---',
      `Use these Skills for this task: ${names}.`,
      'For each, invoke your Skill tool with the matching name before continuing.'
    ].join('\n')
  }
  const sections = skills.map(([name, { description, body }]) => {
    const trimmed = body.trim()
    return `## /${name}\n${description}\n\n${trimmed}`
  })
  return ['--- SKILLS LOADED ---', sections.join('\n\n')].join('\n')
}

function formatFileBlock(referenced: Array<[string, string]>): string {
  return [
    '--- REFERENCED FILES ---',
    referenced.map(([path, content]) => `@${path}:\n${content}`).join('\n\n')
  ].join('\n')
}

/**
 * Composes the final spec sent to the agent. Order is SKILLS → FILES → TASK so
 * the executor reads instructions first, context second, and the task last.
 * Returns the spec unchanged if neither contexts apply.
 */
export function composeFinalSpec(
  spec: string,
  fileContexts: FileContexts,
  skillContexts: SkillContexts,
  executorProvider: ProviderKind
): string {
  const referencedFiles = selectReferencedFiles(spec, fileContexts)
  const referencedSkills = selectReferencedSkills(spec, skillContexts)
  if (referencedFiles.length === 0 && referencedSkills.length === 0) {
    return spec
  }

  const sections: string[] = []
  if (referencedSkills.length > 0) {
    sections.push(formatSkillBlock(referencedSkills, executorProvider))
  }
  if (referencedFiles.length > 0) {
    sections.push(formatFileBlock(referencedFiles))
  }
  return sections.join('\n\n') + '\n\n--- TASK ---\n' + spec
}
