import './tauri-shim'
import './assets/base.css'
import './assets/main.css'

import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { invoke } from '@tauri-apps/api/core'
import App from './App'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>
)

// The Tauri window starts hidden (tauri.conf.json `visible: false`) so the
// user never sees the bare-HTML splash flash before React mounts. We unhide
// from a Rust command (`show_main_window`) instead of the JS window plugin
// because the JS path requires a `core:window:allow-show` capability the
// app doesn't currently grant. The invoke fails harmlessly under
// `npm run dev:renderer` (no Tauri host).
let revealed = false
const reveal = (): void => {
  if (revealed) return
  revealed = true
  void invoke('show_main_window').catch((e) => console.error('[main] failed to show window', e))
}
requestAnimationFrame(() => requestAnimationFrame(reveal))
window.setTimeout(reveal, 800)
