import { openUrl } from '@tauri-apps/plugin-opener'
import { isTauri } from './tauri-api'

const EXTERNAL_PROTOCOLS = new Set(['http:', 'https:', 'mailto:'])

/**
 * Returns the normalized URL when `href` is an absolute http(s)/mailto link that
 * is safe to hand to the OS, otherwise null. Relative links, anchors and any
 * other scheme (javascript:, file:, data:, custom app schemes) are rejected.
 */
export function toExternalUrl(href: string | undefined | null): string | null {
  if (!href) return null
  let url: URL
  try {
    url = new URL(href.trim())
  } catch {
    return null
  }
  return EXTERNAL_PROTOCOLS.has(url.protocol) ? url.href : null
}

/**
 * Open a link from agent output or app chrome in the system browser. The Tauri
 * webview ignores `target="_blank"` (WKWebView) or opens a bare in-app window
 * (WebView2), so links must go through the opener plugin instead.
 */
export async function openExternalUrl(href: string | undefined | null): Promise<boolean> {
  const url = toExternalUrl(href)
  if (!url) return false
  if (isTauri) {
    await openUrl(url)
  } else if (typeof window !== 'undefined') {
    window.open(url, '_blank', 'noopener,noreferrer')
  }
  return true
}
