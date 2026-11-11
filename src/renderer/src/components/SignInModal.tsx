import React, { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ExternalLink } from 'lucide-react'
import { GlassButton } from './ui/GlassButton'
import { GlassModal } from './ui/GlassModal'
import { Chip } from './ui/Chip'
import { cn } from '../lib/utils'
import { errorCallout, fieldInput, fieldLabel } from '../lib/ui'
import type { LoginServiceView, OAuthStatusView, ProviderView } from '../types'

const POLL_INTERVAL_MS = 800

interface SignInModalProps {
  isOpen: boolean
  onClose: () => void
  onDone: (providers: ProviderView[]) => void
}

/**
 * Signing in to subscription-style providers. Browser services open the
 * system browser and wait for the redirect back to the app's local callback;
 * key services only need a pasted key.
 */
export function SignInModal({ isOpen, onClose, onDone }: SignInModalProps): React.ReactNode {
  const { t } = useTranslation()
  const [services, setServices] = useState<LoginServiceView[] | null>(null)
  const [active, setActive] = useState<string | null>(null)
  const [key, setKey] = useState('')
  const [session, setSession] = useState<string | null>(null)
  const [status, setStatus] = useState<OAuthStatusView | null>(null)
  const [error, setError] = useState<string | null>(null)
  const poll = useRef<number | null>(null)

  // The whole flow belongs to one modal session: GlassModal unmounts its
  // content but keeps this component mounted, so a flow left failed, cancelled
  // or still waiting would otherwise greet the user on the next open with a
  // stale error and the service buttons disabled. Reset during render, keyed
  // like AssignTaskModal's `session` — an effect calling setState cascades.
  const flowKey = isOpen ? 'open' : 'closed'
  const [openedWith, setOpenedWith] = useState(flowKey)
  if (openedWith !== flowKey) {
    setOpenedWith(flowKey)
    setActive(null)
    setKey('')
    setSession(null)
    setStatus(null)
    setError(null)
  }

  useEffect(() => {
    if (!isOpen) return
    let alive = true
    void window.api.models
      .loginServices()
      .then((found) => {
        if (alive) setServices(found)
      })
      .catch((cause: unknown) => {
        if (alive) setError(String(cause))
      })
    return () => {
      alive = false
    }
  }, [isOpen])

  const stopPolling = useCallback(() => {
    if (poll.current !== null) {
      window.clearInterval(poll.current)
      poll.current = null
    }
  }, [])

  useEffect(() => stopPolling, [stopPolling])

  // Closing drops the flow, so nothing may keep polling for it: the backend
  // prunes a session once it goes terminal, and the UI has already moved on.
  useEffect(() => {
    if (isOpen) return
    stopPolling()
  }, [isOpen, stopPolling])

  /** One status tick: terminal states end the flow, anything else keeps it. */
  const handleStatus = useCallback(
    (next: OAuthStatusView): void => {
      setStatus(next)
      if (next.state === 'done') {
        stopPolling()
        if (next.providers) onDone(next.providers)
      } else if (next.state === 'failed' || next.state === 'cancelled') {
        stopPolling()
        if (next.message) setError(next.message)
      } else if (next.state === 'unknown') {
        // The backend prunes a session once it goes terminal, so this is "the
        // flow is gone", not "still working" — keep polling and it never ends.
        stopPolling()
        setSession(null)
        setActive(null)
        setError(t('models.sessionGone'))
      }
    },
    [onDone, stopPolling, t]
  )

  const watch = useCallback(
    (sessionId: string) => {
      stopPolling()
      poll.current = window.setInterval(() => {
        void window.api.models
          .oauthStatus(sessionId)
          .then(handleStatus)
          .catch(() => {
            // A dropped poll is retried on the next tick; only a terminal
            // state closes the session.
          })
      }, POLL_INTERVAL_MS)
    },
    [handleStatus, stopPolling]
  )

  const start = async (service: LoginServiceView): Promise<void> => {
    setError(null)
    setStatus(null)
    setActive(service.id)
    try {
      const sessionId = await window.api.models.launchOAuth(
        service.id,
        service.needsKey ? key.trim() : undefined
      )
      setSession(sessionId)
      watch(sessionId)
      handleStatus(await window.api.models.oauthStatus(sessionId))
    } catch (cause) {
      setError(String(cause))
      setActive(null)
    }
  }

  const cancel = async (): Promise<void> => {
    if (session) await window.api.models.cancelOAuth(session).catch(() => undefined)
    stopPolling()
    setSession(null)
    setStatus(null)
    setActive(null)
  }

  const waiting = status?.state === 'waiting'
  const activeService = services?.find((service) => service.id === active) ?? null
  // The key field serves every service that takes one, so a key-only flow can
  // be pasted before it is launched; a browser-only flow does not want it.
  const showKeyField =
    !waiting && (activeService ? activeService.needsKey : (services ?? []).some((s) => s.needsKey))
  const waitingSeconds = status ? Math.round(status.waitingMs / 1000) : 0

  return (
    <GlassModal
      isOpen={isOpen}
      onClose={onClose}
      title={t('models.signInTitle')}
      dismissible={!waiting}
      data-testid="sign-in-modal"
    >
      <div className="space-y-3 px-4 py-4">
        <p className="text-[12px] text-muted-foreground">{t('models.signInIntro')}</p>
        {error && <p className={cn(errorCallout, 'text-[12px]')}>{error}</p>}

        <ul className="space-y-2">
          {(services ?? []).map((service) => (
            <li
              key={service.id}
              className="flex items-center gap-3 rounded-md border border-border px-3 py-2"
            >
              <span className="min-w-0 flex-1">
                <span className="block text-[13px] font-medium text-foreground">
                  {service.displayName}
                </span>
                <span className="block text-[11px] text-muted-foreground">
                  {service.needsBrowser && service.needsKey
                    ? t('models.opensBrowserOrKey')
                    : service.needsBrowser
                      ? t('models.opensBrowser')
                      : t('models.pasteKey')}
                </span>
              </span>
              {service.needsBrowser && (
                <ExternalLink size={13} className="shrink-0 text-muted-foreground" />
              )}
              <GlassButton
                variant={active === service.id ? 'secondary' : 'primary'}
                size="sm"
                disabled={active !== null && active !== service.id}
                onClick={() => void start(service)}
                data-testid={`sign-in-${service.id}`}
              >
                {active === service.id && waiting ? t('models.waiting') : t('models.signInAction')}
              </GlassButton>
            </li>
          ))}
        </ul>

        {showKeyField && (
          <label className="block">
            <span className={fieldLabel}>{t('models.apiKey')}</span>
            <input
              className={cn(fieldInput, 'mt-1 font-mono')}
              type="password"
              autoComplete="off"
              value={key}
              onChange={(event) => setKey(event.target.value)}
              data-testid="sign-in-key"
            />
          </label>
        )}

        {waiting && (
          <div className="flex items-center gap-2 rounded-md border border-border px-3 py-2">
            <Chip shape="tag" tone="running">
              {t('models.waiting')}
            </Chip>
            <span className="min-w-0 flex-1 text-[12px] text-muted-foreground">
              {t('models.waitingHint')}
              {waitingSeconds > 0
                ? ` ${t('models.waitingSeconds', { seconds: waitingSeconds })}`
                : ''}
            </span>
            <GlassButton
              variant="ghost"
              size="xs"
              onClick={() => void cancel()}
              data-testid="sign-in-cancel"
            >
              {t('models.cancelSignIn')}
            </GlassButton>
          </div>
        )}

        {status?.state === 'done' && (
          <p className="text-[12px] state-done">{t('models.signInDone')}</p>
        )}

        <div className="flex justify-end border-t border-border pt-3">
          <GlassButton variant="ghost" size="sm" onClick={onClose}>
            {t('common.close')}
          </GlassButton>
        </div>
      </div>
    </GlassModal>
  )
}
