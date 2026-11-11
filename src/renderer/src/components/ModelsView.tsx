import React, { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { KeyRound, Plus, RefreshCw, Trash2 } from 'lucide-react'
import { GlassButton } from './ui/GlassButton'
import { GlassModal } from './ui/GlassModal'
import { Chip } from './ui/Chip'
import { cn } from '../lib/utils'
import { emptyHint, fieldInput, fieldLabel, sectionLabel } from '../lib/ui'
import { useModelStore } from '../store/useModelStore'
import type { AccountStatus, AccountView, ModelProtocol, ProviderView } from '../types'
import { ImportWizardModal } from './ImportWizardModal'
import { SignInModal } from './SignInModal'

/** Every wire protocol the registry speaks, in the order the form lists them. */
const PROTOCOLS: ModelProtocol[] = ['openaiChat', 'anthropic', 'responses', 'commandCode']

/** A URL the registry can actually call: a scheme plus a host, nothing less. */
function isUsableBaseUrl(raw: string): boolean {
  try {
    const parsed = new URL(raw)
    return (
      (parsed.protocol === 'http:' || parsed.protocol === 'https:') && parsed.hostname.length > 0
    )
  } catch {
    return false
  }
}

const STATUS_TONES: Record<AccountStatus, 'done' | 'running' | 'error'> = {
  ready: 'done',
  needsKey: 'running',
  authExpired: 'error',
  rateLimited: 'running'
}

export function ModelsView(): React.ReactNode {
  const { t } = useTranslation()
  const { providers, loaded, load, replace } = useModelStore()
  const [wizardOpen, setWizardOpen] = useState(false)
  const [signInOpen, setSignInOpen] = useState(false)
  const [addOpen, setAddOpen] = useState(false)
  const [expanded, setExpanded] = useState<string | null>(null)
  const [keyEditor, setKeyEditor] = useState<{
    provider: ProviderView
    account: AccountView
  } | null>(null)

  useEffect(() => {
    if (!loaded) void load()
  }, [loaded, load])

  const refresh = useCallback(async () => {
    await load()
  }, [load])

  return (
    <div data-testid="models-view" className="flex h-full flex-col overflow-hidden">
      <div className="shrink-0 px-6 pt-5">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h1 className="text-[17px] font-semibold tracking-tight text-foreground">
              {t('models.title')}
            </h1>
            <p className="mt-0.5 text-[13px] text-muted-foreground">{t('models.subtitle')}</p>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <GlassButton
              variant="secondary"
              size="sm"
              onClick={() => void refresh()}
              data-testid="models-refresh"
            >
              <RefreshCw size={13} />
              {t('models.refresh')}
            </GlassButton>
            <GlassButton
              variant="secondary"
              size="sm"
              onClick={() => setSignInOpen(true)}
              data-testid="models-sign-in"
            >
              <KeyRound size={13} />
              {t('models.signIn')}
            </GlassButton>
            <GlassButton
              variant="secondary"
              size="sm"
              onClick={() => setAddOpen(true)}
              data-testid="models-add-provider"
            >
              <Plus size={13} />
              {t('models.addProvider')}
            </GlassButton>
            <GlassButton
              variant="primary"
              size="sm"
              onClick={() => setWizardOpen(true)}
              data-testid="models-scan"
            >
              {t('models.scan')}
            </GlassButton>
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4 scrollbar-thin">
        {providers.length === 0 ? (
          <div className="shell-card px-4 py-8 text-center">
            <p className="text-[13px] text-muted-foreground">{t('models.empty')}</p>
            <p className="mt-1 text-[12px] text-muted-foreground-faint">{t('models.emptyHint')}</p>
          </div>
        ) : (
          <ul className="space-y-2">
            {providers.map((provider) => (
              <li key={provider.id}>
                <ProviderCard
                  provider={provider}
                  expanded={expanded === provider.id}
                  onToggle={() =>
                    setExpanded((current) => (current === provider.id ? null : provider.id))
                  }
                  onReplaceKey={(account) => setKeyEditor({ provider, account })}
                  onChanged={replace}
                />
              </li>
            ))}
          </ul>
        )}
      </div>

      <ImportWizardModal
        isOpen={wizardOpen}
        onClose={() => setWizardOpen(false)}
        onImported={replace}
      />
      <SignInModal isOpen={signInOpen} onClose={() => setSignInOpen(false)} onDone={replace} />
      <AddProviderModal isOpen={addOpen} onClose={() => setAddOpen(false)} onAdded={replace} />
      {keyEditor && (
        <ReplaceKeyModal
          provider={keyEditor.provider}
          account={keyEditor.account}
          onClose={() => setKeyEditor(null)}
          onSaved={replace}
        />
      )}
    </div>
  )
}

interface ProviderCardProps {
  provider: ProviderView
  expanded: boolean
  onToggle: () => void
  onReplaceKey: (account: AccountView) => void
  onChanged: (providers: ProviderView[]) => void
}

function ProviderCard({
  provider,
  expanded,
  onToggle,
  onReplaceKey,
  onChanged
}: ProviderCardProps): React.ReactNode {
  const { t } = useTranslation()
  const ready = provider.readyAccountCount
  const total = provider.accounts.length

  return (
    <section
      data-testid={`provider-card-${provider.id}`}
      className="shell-card shell-card-hover overflow-hidden"
    >
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={expanded}
        className="flex w-full cursor-pointer items-center gap-3 px-4 py-3 text-left"
      >
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-2">
            <span className="truncate text-[13px] font-medium text-foreground">
              {provider.displayName}
            </span>
            <Chip shape="tag" tone="neutral">
              {t(`models.protocol.${provider.protocol}`)}
            </Chip>
            {ready === 0 && (
              <Chip shape="tag" tone="running">
                {t('models.noAccount')}
              </Chip>
            )}
          </span>
          <span className="mt-0.5 block truncate font-mono text-[11px] text-muted-foreground">
            {provider.baseUrl}
          </span>
        </span>
        <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">
          {t('models.accountCount', { ready, total })}
        </span>
        <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">
          {t('models.modelCount', { count: provider.models.length })}
        </span>
      </button>

      {expanded && (
        <div className="border-t border-border px-4 py-3">
          <h3 className={sectionLabel}>{t('models.accounts')}</h3>
          {provider.accounts.length === 0 ? (
            <p className={emptyHint}>{t('models.noAccounts')}</p>
          ) : (
            <ul className="mt-1.5 space-y-1.5">
              {provider.accounts.map((account) => (
                <li
                  key={account.id}
                  className="flex items-center gap-2 rounded-md border border-border px-2.5 py-1.5"
                >
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[12px] text-foreground/90">
                      {account.label}
                    </span>
                    <span className="block truncate font-mono text-[10px] text-muted-foreground">
                      {t(`models.accountKind.${account.kind}`)}
                      {account.maskedSecret ? ` · ${account.maskedSecret}` : ''} · {account.source}
                    </span>
                  </span>
                  <Chip shape="tag" tone={STATUS_TONES[account.status]}>
                    {t(`models.status.${account.status}`)}
                  </Chip>
                  <GlassButton
                    variant="ghost"
                    size="xs"
                    onClick={() => onReplaceKey(account)}
                    title={t('models.replaceKey')}
                    aria-label={t('models.replaceKey')}
                  >
                    <KeyRound size={12} />
                  </GlassButton>
                  <DeleteAccountButton
                    providerId={provider.id}
                    accountId={account.id}
                    onChanged={onChanged}
                  />
                </li>
              ))}
            </ul>
          )}

          {provider.models.length > 0 && (
            <>
              <h3 className={cn(sectionLabel, 'mt-4')}>{t('models.models')}</h3>
              <ul className="mt-1.5 flex flex-wrap gap-1.5">
                {provider.models.map((model) => (
                  <li key={model.id}>
                    <Chip shape="tag" tone="neutral" title={model.id}>
                      {model.displayName}
                    </Chip>
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      )}
    </section>
  )
}

function DeleteAccountButton({
  providerId,
  accountId,
  onChanged
}: {
  providerId: string
  accountId: string
  onChanged: (providers: ProviderView[]) => void
}): React.ReactNode {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const remove = async (): Promise<void> => {
    setBusy(true)
    setError(null)
    try {
      onChanged(await window.api.models.deleteAccount({ providerId, accountId }))
    } catch (cause) {
      setError(String(cause))
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <GlassButton
        variant="ghost"
        size="xs"
        disabled={busy}
        onClick={() => void remove()}
        title={t('models.deleteAccount')}
        aria-label={t('models.deleteAccount')}
        data-testid="account-delete"
      >
        <Trash2 size={12} />
      </GlassButton>
      {error && <span className="text-[11px] state-error">{error}</span>}
    </>
  )
}

function ReplaceKeyModal({
  provider,
  account,
  onClose,
  onSaved
}: {
  provider: ProviderView
  account: AccountView
  onClose: () => void
  onSaved: (providers: ProviderView[]) => void
}): React.ReactNode {
  const { t } = useTranslation()
  const [key, setKey] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const save = async (): Promise<void> => {
    if (key.trim().length === 0) {
      setError(t('models.keyRequired'))
      return
    }
    setBusy(true)
    setError(null)
    try {
      onSaved(
        await window.api.models.setAccountKey({
          providerId: provider.id,
          accountId: account.id,
          apiKey: key.trim()
        })
      )
      onClose()
    } catch (cause) {
      setError(String(cause))
    } finally {
      setBusy(false)
    }
  }

  return (
    <GlassModal
      isOpen
      onClose={onClose}
      title={t('models.replaceKeyTitle', { label: account.label })}
      data-testid="replace-key-modal"
    >
      <div className="space-y-3 px-4 py-4">
        <p className="text-[12px] text-muted-foreground">
          {t('models.replaceKeyHint', { provider: provider.displayName })}
        </p>
        <label className="block">
          <span className={fieldLabel}>{t('models.apiKey')}</span>
          <input
            className={cn(fieldInput, 'mt-1 font-mono')}
            type="password"
            autoComplete="off"
            value={key}
            onChange={(event) => setKey(event.target.value)}
            placeholder={account.maskedSecret ?? ''}
            data-testid="replace-key-input"
          />
        </label>
        {error && <p className="text-[12px] state-error">{error}</p>}
        <div className="flex justify-end gap-2">
          <GlassButton
            variant="ghost"
            size="sm"
            onClick={onClose}
            data-testid="add-provider-cancel"
          >
            {t('common.cancel')}
          </GlassButton>
          <GlassButton
            variant="primary"
            size="sm"
            disabled={busy}
            onClick={() => void save()}
            data-testid="replace-key-save"
          >
            {t('models.save')}
          </GlassButton>
        </div>
      </div>
    </GlassModal>
  )
}

function AddProviderModal({
  isOpen,
  onClose,
  onAdded
}: {
  isOpen: boolean
  onClose: () => void
  onAdded: (providers: ProviderView[]) => void
}): React.ReactNode {
  const { t } = useTranslation()
  const [protocol, setProtocol] = useState<ModelProtocol>('openaiChat')
  const [baseUrl, setBaseUrl] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [label, setLabel] = useState('')
  const [models, setModels] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  // GlassModal unmounts its content but keeps this component mounted, so a
  // cancelled draft would otherwise be waiting on the next open.
  useEffect(() => {
    if (!isOpen) return
    setProtocol('openaiChat')
    setBaseUrl('')
    setApiKey('')
    setLabel('')
    setModels('')
    setError(null)
  }, [isOpen])

  const submit = async (): Promise<void> => {
    const url = baseUrl.trim()
    if (url.length === 0) {
      setError(t('models.baseUrlRequired'))
      return
    }
    if (!isUsableBaseUrl(url)) {
      setError(t('models.baseUrlInvalid'))
      return
    }
    setBusy(true)
    setError(null)
    try {
      onAdded(
        await window.api.models.addProvider({
          protocol,
          baseUrl: url,
          apiKey: apiKey.trim(),
          label: label.trim() || undefined,
          models: models
            .split(/[,\n]/)
            .map((id) => id.trim())
            .filter((id) => id.length > 0)
        })
      )
      onClose()
    } catch (cause) {
      setError(String(cause))
    } finally {
      setBusy(false)
    }
  }

  return (
    <GlassModal
      isOpen={isOpen}
      onClose={onClose}
      title={t('models.addProviderTitle')}
      data-testid="add-provider-modal"
    >
      <div className="space-y-3 px-4 py-4">
        <label className="block">
          <span className={fieldLabel}>{t('models.protocolLabel')}</span>
          <select
            className={cn(fieldInput, 'mt-1')}
            value={protocol}
            onChange={(event) => setProtocol(event.target.value as ModelProtocol)}
            data-testid="add-provider-protocol"
          >
            {PROTOCOLS.map((value) => (
              <option key={value} value={value}>
                {t(`models.protocol.${value}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="block">
          <span className={fieldLabel}>{t('models.baseUrl')}</span>
          <input
            className={cn(fieldInput, 'mt-1 font-mono')}
            value={baseUrl}
            onChange={(event) => setBaseUrl(event.target.value)}
            placeholder={t('models.baseUrlPlaceholder')}
            data-testid="add-provider-base-url"
          />
        </label>
        <label className="block">
          <span className={fieldLabel}>{t('models.apiKey')}</span>
          <input
            className={cn(fieldInput, 'mt-1 font-mono')}
            type="password"
            autoComplete="off"
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
            data-testid="add-provider-key"
          />
        </label>
        <label className="block">
          <span className={fieldLabel}>{t('models.labelOptional')}</span>
          <input
            className={cn(fieldInput, 'mt-1')}
            value={label}
            onChange={(event) => setLabel(event.target.value)}
            placeholder={t('models.labelPlaceholder')}
          />
        </label>
        <label className="block">
          <span className={fieldLabel}>{t('models.modelsOptional')}</span>
          <textarea
            className={cn(fieldInput, 'mt-1 min-h-[64px] resize-y font-mono')}
            value={models}
            onChange={(event) => setModels(event.target.value)}
            placeholder={t('models.modelsPlaceholder')}
            data-testid="add-provider-models"
          />
        </label>
        {error && <p className="text-[12px] state-error">{error}</p>}
        <div className="flex justify-end gap-2">
          <GlassButton
            variant="ghost"
            size="sm"
            onClick={onClose}
            data-testid="add-provider-cancel"
          >
            {t('common.cancel')}
          </GlassButton>
          <GlassButton
            variant="primary"
            size="sm"
            disabled={busy}
            onClick={() => void submit()}
            data-testid="add-provider-save"
          >
            {t('models.add')}
          </GlassButton>
        </div>
      </div>
    </GlassModal>
  )
}
