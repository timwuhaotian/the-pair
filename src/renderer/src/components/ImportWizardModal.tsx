import React, { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AlertTriangle, RefreshCw } from 'lucide-react'
import { GlassButton } from './ui/GlassButton'
import { GlassModal } from './ui/GlassModal'
import { Chip } from './ui/Chip'
import { cn } from '../lib/utils'
import { errorCallout, warningCallout } from '../lib/ui'
import { candidateRows } from '../store/useModelStore'
import type { CandidateView, ProviderView } from '../types'

interface ImportWizardModalProps {
  isOpen: boolean
  onClose: () => void
  onImported: (providers: ProviderView[]) => void
}

/** Closes the wizard and drops the scan, so the next open starts fresh. */
function useDismiss(onClose: () => void, reset: () => void): () => void {
  return () => {
    reset()
    onClose()
  }
}

/**
 * First-run (and on-demand) import: shows, per source, exactly which endpoints
 * and credentials a scan would copy, and lets the user pick. Nothing is written
 * until Import is pressed.
 */
export function ImportWizardModal({
  isOpen,
  onClose,
  onImported
}: ImportWizardModalProps): React.ReactNode {
  const { t } = useTranslation()
  const [candidates, setCandidates] = useState<CandidateView[] | null>(null)
  const dismiss = useDismiss(onClose, () => setCandidates(null))
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const runScan = useCallback(async () => {
    setBusy(true)
    setError(null)
    try {
      const found = await window.api.models.scan()
      setCandidates(found)
      // Sources with something to import start checked; sources that only carry
      // a note or an error start unchecked so a single click imports the rest.
      setSelected(
        new Set(
          candidateRows(found)
            .filter((candidate) => candidate.accountCount > 0)
            .map((candidate) => candidate.sourceId)
        )
      )
    } catch (cause) {
      setError(String(cause))
      setCandidates([])
    } finally {
      setBusy(false)
    }
  }, [])

  useEffect(() => {
    if (isOpen && candidates === null) void runScan()
  }, [isOpen, candidates, runScan])

  const rows = candidates === null ? [] : candidateRows(candidates)
  const selectedCount = rows.filter((row) => selected.has(row.sourceId)).length

  const commit = async (): Promise<void> => {
    if (selectedCount === 0) return
    setBusy(true)
    setError(null)
    try {
      onImported(
        await window.api.models.commitImport(
          rows.filter((row) => selected.has(row.sourceId)).map((row) => row.sourceId)
        )
      )
      dismiss()
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
      title={t('models.wizard.title')}
      className="max-w-2xl"
      data-testid="import-wizard"
    >
      <div className="space-y-3 px-4 py-4">
        <p className="text-[12px] text-muted-foreground">{t('models.wizard.intro')}</p>

        {error && <p className={cn(errorCallout, 'text-[12px]')}>{error}</p>}

        {candidates === null || (busy && rows.length === 0) ? (
          <p className="py-6 text-center text-[12px] text-muted-foreground">
            {t('models.wizard.scanning')}
          </p>
        ) : rows.length === 0 ? (
          <p className="py-6 text-center text-[12px] text-muted-foreground">
            {t('models.wizard.nothingFound')}
          </p>
        ) : (
          <ul className="max-h-[46vh] space-y-2 overflow-y-auto scrollbar-thin">
            {rows.map((candidate) => {
              const hasAccounts = candidate.accountCount > 0
              const checked = selected.has(candidate.sourceId)
              return (
                <li
                  key={candidate.sourceId}
                  data-testid={`import-row-${candidate.sourceId}`}
                  className={cn(
                    'rounded-md border px-3 py-2',
                    checked ? 'border-foreground/25 bg-foreground/[0.03]' : 'border-border'
                  )}
                >
                  <label className="flex cursor-pointer items-start gap-2.5">
                    <input
                      type="checkbox"
                      className="mt-1 accent-foreground"
                      checked={checked}
                      disabled={!hasAccounts}
                      onChange={(event) =>
                        setSelected((current) => {
                          const next = new Set(current)
                          if (event.target.checked) next.add(candidate.sourceId)
                          else next.delete(candidate.sourceId)
                          return next
                        })
                      }
                    />
                    <span className="min-w-0 flex-1">
                      <span className="flex items-baseline gap-2">
                        <span className="text-[13px] font-medium text-foreground">
                          {candidate.sourceName}
                        </span>
                        {candidate.configPath && (
                          <span className="truncate font-mono text-[10px] text-muted-foreground">
                            {candidate.configPath}
                          </span>
                        )}
                      </span>

                      {candidate.providers.map((provider) => (
                        <span
                          key={`${provider.protocol}-${provider.baseUrl}`}
                          className="mt-1.5 block"
                        >
                          <span className="flex flex-wrap items-center gap-1.5">
                            <span className="font-mono text-[11px] text-foreground/85">
                              {provider.baseUrl}
                            </span>
                            {provider.alreadyImported && (
                              <Chip shape="tag" tone="neutral">
                                {t('models.wizard.alreadyImported')}
                              </Chip>
                            )}
                          </span>
                          {provider.accounts.map((account) => (
                            <span
                              key={`${account.source}-${account.label}`}
                              className="mt-0.5 block truncate font-mono text-[10px] text-muted-foreground"
                            >
                              {account.label} · {account.maskedSecret ?? t('models.wizard.noKey')}
                            </span>
                          ))}
                        </span>
                      ))}

                      {candidate.notes.map((note) => (
                        <span key={note} className={cn(warningCallout, 'mt-1.5 block text-[11px]')}>
                          {note}
                        </span>
                      ))}
                      {candidate.error && (
                        <span
                          className={cn(
                            errorCallout,
                            'mt-1.5 flex items-start gap-1.5 text-[11px]'
                          )}
                        >
                          <AlertTriangle size={12} className="mt-px shrink-0" />
                          {candidate.error}
                        </span>
                      )}
                    </span>
                  </label>
                </li>
              )
            })}
          </ul>
        )}

        <div className="flex items-center justify-between gap-2 border-t border-border pt-3">
          <GlassButton
            variant="ghost"
            size="sm"
            disabled={busy}
            onClick={() => void runScan()}
            data-testid="import-wizard-rescan"
          >
            <RefreshCw size={13} />
            {t('models.wizard.rescan')}
          </GlassButton>
          <div className="flex items-center gap-2">
            <GlassButton variant="ghost" size="sm" onClick={onClose}>
              {t('common.cancel')}
            </GlassButton>
            <GlassButton
              variant="primary"
              size="sm"
              disabled={busy || selectedCount === 0}
              onClick={() => void commit()}
              data-testid="import-wizard-commit"
            >
              {t('models.wizard.import', { count: selectedCount })}
            </GlassButton>
          </div>
        </div>
      </div>
    </GlassModal>
  )
}
