/**
 * jsdom environment for component unit tests, imported before any component
 * module so React and the Tauri API shims see a DOM.
 *
 * Every spec that renders a component imports this module first (static
 * import, so its side effects run before the component modules evaluate).
 */
import { JSDOM } from 'jsdom'

const dom = new JSDOM('<!doctype html><html><body></body></html>', {
  url: 'https://pair.test/',
  pretendToBeVisual: true
})

const globals = {
  window: dom.window,
  document: dom.window.document,
  navigator: dom.window.navigator,
  location: dom.window.location,
  HTMLElement: dom.window.HTMLElement,
  HTMLInputElement: dom.window.HTMLInputElement,
  HTMLTextAreaElement: dom.window.HTMLTextAreaElement,
  HTMLButtonElement: dom.window.HTMLButtonElement,
  Element: dom.window.Element,
  Node: dom.window.Node,
  getComputedStyle: dom.window.getComputedStyle.bind(dom.window),
  requestAnimationFrame: dom.window.requestAnimationFrame.bind(dom.window),
  cancelAnimationFrame: dom.window.cancelAnimationFrame.bind(dom.window),
  MutationObserver: dom.window.MutationObserver,
  localStorage: dom.window.localStorage,
  matchMedia: (query: string) => ({
    matches: false,
    media: query,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
    onchange: null
  })
} as unknown as Record<string, unknown>

for (const [key, value] of Object.entries(globals)) {
  Object.defineProperty(globalThis, key, {
    configurable: true,
    writable: true,
    value
  })
}

// React needs this flag to treat state updates inside tests as act() wrapped.
;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true

// Vite `define` constants the shim/renderer read at module scope.
;(globalThis as unknown as { __APP_VERSION__: string }).__APP_VERSION__ = '0.0.0-test'
