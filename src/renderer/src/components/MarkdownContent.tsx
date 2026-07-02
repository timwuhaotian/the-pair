import { memo } from 'react'
import ReactMarkdown from 'react-markdown'
import type { Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'

/**
 * Hoisted to module scope so the object identity stays stable across renders.
 * None of the render functions capture instance state — they are pure.
 */
const MARKDOWN_COMPONENTS: Components = {
  h1: ({ children }) => (
    <h1 className="mb-1 text-[14px] font-bold uppercase tracking-[0.08em] text-foreground">
      ── {children}
    </h1>
  ),
  h2: ({ children }) => (
    <h2 className="mb-1 mt-2 text-[13px] font-bold uppercase tracking-[0.06em] text-foreground">
      ─ {children}
    </h2>
  ),
  h3: ({ children }) => (
    <h3 className="mb-1 mt-2 text-[12px] font-bold uppercase tracking-[0.04em] text-foreground/90">
      {children}
    </h3>
  ),
  p: ({ children }) => <p className="mb-2 last:mb-0 leading-relaxed">{children}</p>,
  ul: ({ children }) => <ul className="mb-2 ml-1 space-y-0.5">{children}</ul>,
  ol: ({ children }) => <ol className="mb-2 ml-1 space-y-0.5 list-decimal pl-5">{children}</ol>,
  li: ({ children }) => (
    <li className="leading-relaxed pl-4 relative before:absolute before:left-0 before:top-0 before:text-muted-foreground-faint before:content-['·']">
      {children}
    </li>
  ),
  blockquote: ({ children }) => (
    <blockquote className="my-2 border-l-2 border-border pl-3 text-foreground/75">
      {children}
    </blockquote>
  ),
  code: ({ className, children }) => {
    const isBlock = className?.includes('language-')
    if (isBlock) {
      return (
        <code className="block overflow-x-auto border border-border bg-background/40 px-3 py-2 text-[12px] leading-relaxed rounded-sm">
          {children}
        </code>
      )
    }
    return (
      <code className="border border-border bg-foreground/[0.06] px-1 py-px text-[12px] rounded-sm">
        {children}
      </code>
    )
  },
  pre: ({ children }) => <pre className="my-2 overflow-x-auto">{children}</pre>,
  a: ({ href, children }) => (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="role-mentor underline underline-offset-2 hover:text-foreground"
    >
      {children}
    </a>
  ),
  table: ({ children }) => (
    <table className="my-2 w-full border-collapse text-[12px]">{children}</table>
  ),
  th: ({ children }) => (
    <th className="border border-border bg-foreground/[0.05] px-2 py-1 text-left font-bold uppercase tracking-[0.08em] text-[10px]">
      {children}
    </th>
  ),
  td: ({ children }) => (
    <td className="border border-border px-2 py-1 align-baseline">{children}</td>
  ),
  hr: () => <hr className="my-2 tty-divider" />,
  strong: ({ children }) => <strong className="font-bold text-foreground">{children}</strong>,
  em: ({ children }) => <em className="not-italic text-foreground/85">{children}</em>
}

/**
 * Terminal-flavored markdown renderer. No serif body fonts, no decorative
 * radii — everything reads as a typed log entry. Inline + block code keep
 * their own subtle surface treatment for legibility.
 *
 * Wrapped in `memo` with a shallow prop comparison: the only prop is a
 * `content` string, so re-renders only happen when the content actually
 * changes.
 */
export const MarkdownContent = memo(function MarkdownContent({
  content
}: {
  content: string
}): React.ReactNode {
  return (
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={MARKDOWN_COMPONENTS}>
      {content}
    </ReactMarkdown>
  )
})
