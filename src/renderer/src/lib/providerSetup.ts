import type { AvailableModel, ProviderKind, ProviderSetupHint } from '../types'

function isSelectableForPairExecution(model: AvailableModel): boolean {
  return model.available && model.supportsPairExecution
}

export interface ProviderSetupSummary {
  readyModelCount: number
  readyProviderLabels: string[]
  isReady: boolean
}

export function buildProviderSetupSummary(models: AvailableModel[]): ProviderSetupSummary {
  const readyModels = models.filter(isSelectableForPairExecution)
  const readyProviderLabels = Array.from(
    new Set(readyModels.map((model) => model.providerLabel))
  ).sort()

  return {
    readyModelCount: readyModels.length,
    readyProviderLabels,
    isReady: readyModels.length > 0
  }
}

/** Static per-provider metadata for login/install guidance. Mirrors the Rust Provider trait. */
const PROVIDER_LOGIN_COMMANDS: Partial<Record<ProviderKind, string>> = {
  claude: 'claude login',
  codex: 'codex auth',
  opencode: 'opencode auth login'
}

const PROVIDER_INSTALL_URLS: Partial<Record<ProviderKind, string>> = {
  claude: 'https://claude.ai/download',
  codex: 'https://github.com/openai/codex',
  opencode: 'https://opencode.ai'
}

/**
 * Build per-provider setup hints from the full model catalog.
 * Each hint tells the user whether the provider is installed, authenticated,
 * how many models are ready, and what login/install command to show.
 *
 * Providers are sorted: ready first, then auth-missing, then cli-missing.
 */
export function buildProviderSetupHints(models: AvailableModel[]): ProviderSetupHint[] {
  // Group models by provider kind
  const byProvider = new Map<ProviderKind, AvailableModel[]>()

  for (const model of models) {
    const existing = byProvider.get(model.provider)
    if (existing) {
      existing.push(model)
    } else {
      byProvider.set(model.provider, [model])
    }
  }

  const hints: ProviderSetupHint[] = []

  for (const [kind, providerModels] of byProvider) {
    // A provider is "installed" if any model's status is NOT cli-missing
    // (i.e. the CLI binary was detected, even if not authenticated).
    const installed = providerModels.some(
      (m) => m.availabilityStatus !== 'cli-missing'
    )
    // A provider is "authenticated" if it's installed and NOT auth-missing.
    // This avoids showing "Not signed in" when the real issue is runtime-unsupported.
    const authenticated =
      installed && providerModels.some((m) => m.availabilityStatus !== 'auth-missing')
    const readyModels = providerModels.filter(isSelectableForPairExecution)

    const label = providerModels[0]?.providerLabel ?? kind

    // Gemini login/install depends on whether agy or legacy gemini is active.
    // We can't know from models alone, so leave it undefined here — the
    // backend's DetectedProviderProfile.loginCommand is the source of truth
    // for the onboarding screen.
    const loginCommand = PROVIDER_LOGIN_COMMANDS[kind]
    const installUrl = PROVIDER_INSTALL_URLS[kind]

    hints.push({
      kind,
      label,
      installed,
      authenticated,
      readyModelCount: readyModels.length,
      loginCommand,
      installUrl
    })
  }

  // Sort: ready first, then auth-missing (installed but not authed), then cli-missing.
  hints.sort((a, b) => {
    const rank = (h: ProviderSetupHint): number => {
      if (h.readyModelCount > 0) return 0
      if (h.installed) return 1
      return 2
    }
    return rank(a) - rank(b)
  })

  return hints
}
