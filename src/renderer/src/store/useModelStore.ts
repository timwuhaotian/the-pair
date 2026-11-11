import { create } from 'zustand'
import type { CandidateView, ProviderView } from '../types'
import { usePairStore } from './usePairStore'

interface ModelStore {
  providers: ProviderView[]
  loaded: boolean
  load: () => Promise<void>
  /** Applies the registry a command just returned. */
  replace: (providers: ProviderView[]) => void
}

/// The pair store keeps its own copy for the pickers. Publishing every change
/// here too means a credential added in the Models section is visible to a
/// Create Pair modal that was already open, without an app restart.
function publish(providers: ProviderView[]): void {
  usePairStore.setState({ providers })
}

/** `null` when the read failed: the caller then keeps the list it already has. */
async function fetchProviders(): Promise<ProviderView[] | null> {
  const api = typeof window === 'undefined' ? undefined : window.api
  if (!api) return null
  try {
    const providers = await api.models.list()
    // A missing registry file is the normal first-run state, not an error.
    return Array.isArray(providers) ? providers : []
  } catch {
    return null
  }
}

export const useModelStore = create<ModelStore>((set) => ({
  providers: [],
  loaded: false,
  load: async () => {
    const providers = await fetchProviders()
    // A failed read leaves the list alone: overwriting it with an empty one would
    // empty every model picker over a single transient IPC failure, and `loaded`
    // would then stop anything from ever asking again.
    if (!providers) return
    publish(providers)
    set({ providers, loaded: true })
  },
  replace: (providers: ProviderView[]) => {
    publish(providers)
    set({ providers, loaded: true })
  }
}))

// The shell's Models badge counts endpoints whose credential is missing, so the
// registry is read once at startup — before anyone opens the Models section —
// rather than the first time that view mounts.
void useModelStore.getState().load()

/** Rows for the import wizard: one per source that has something to say. */
export function candidateRows(candidates: CandidateView[]): CandidateView[] {
  return candidates.filter(
    (candidate) =>
      candidate.providers.length > 0 || candidate.error !== undefined || candidate.notes.length > 0
  )
}
