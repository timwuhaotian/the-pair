import { create } from 'zustand'
import type { Update } from '@tauri-apps/plugin-updater'
import { extractErrorMessage } from '../lib/utils'

type UpdatePhase = 'idle' | 'checking' | 'available' | 'installing' | 'up-to-date' | 'error'

interface UpdateState {
  phase: UpdatePhase
  version: string | null
  progress: number | null
  message: string | null
  releaseBody: string | null
  update: Update | null
  showModal: boolean
  showToast: boolean
  toastMessage: string | null
  toastType: 'success' | 'error' | 'info' | null
}

interface UpdateActions {
  setPhase: (phase: UpdatePhase) => void
  setVersion: (version: string | null) => void
  setProgress: (progress: number | null) => void
  setMessage: (message: string | null) => void
  setReleaseBody: (body: string | null) => void
  setUpdate: (update: Update | null) => void
  setShowModal: (show: boolean) => void
  displayToast: (message: string, type: 'success' | 'error' | 'info') => void
  clearToast: () => void
  reset: () => void
  installUpdate: () => Promise<void>
}

const initialState: UpdateState = {
  phase: 'idle',
  version: null,
  progress: null,
  message: null,
  releaseBody: null,
  update: null,
  showModal: false,
  showToast: false,
  toastMessage: null,
  toastType: null
}

export const useUpdateStore = create<UpdateState & UpdateActions>((set) => ({
  ...initialState,

  setPhase: (phase) => set({ phase }),
  setVersion: (version) => set({ version }),
  setProgress: (progress) => set({ progress }),
  setMessage: (message) => set({ message }),
  setReleaseBody: (body) => set({ releaseBody: body }),
  setUpdate: (update) => set({ update }),
  setShowModal: (show) => set({ showModal: show }),

  displayToast: (message: string, type: 'success' | 'error' | 'info') =>
    set({
      showToast: true,
      toastMessage: message,
      toastType: type
    }),

  clearToast: () =>
    set({
      showToast: false,
      toastMessage: null,
      toastType: null
    }),

  reset: () => set(initialState),

  installUpdate: async () => {
    const currentUpdate = useUpdateStore.getState().update
    if (!currentUpdate) return

    set({ phase: 'installing', progress: 0 })

    let totalBytes: number | null = null
    let downloadedBytes = 0

    try {
      await currentUpdate.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          totalBytes = event.data.contentLength ?? null
          downloadedBytes = 0
          set({ progress: 0 })
          return
        }

        if (event.event === 'Progress') {
          downloadedBytes += event.data.chunkLength
          if (!totalBytes) {
            set({ progress: null })
            return
          }
          const nextValue = (downloadedBytes / totalBytes) * 100
          set({ progress: Math.min(99, Math.round(nextValue)) })
          return
        }

        set({ progress: 100 })
      })

      await currentUpdate.close().catch(() => {})
      await window.api.app.restart()
    } catch (error) {
      const message = extractErrorMessage(error, 'Update installation failed')
      set({ message, phase: 'error', showModal: false })
    }
  }
}))
