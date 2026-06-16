import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, RotateCcw } from 'lucide-react'
import { usePairStore, type Pair } from '../store/usePairStore'
import { GlassButton } from './ui/GlassButton'

/**
 * Human-in-the-loop approval bar shown when a pair is "Awaiting Human Review" —
 * i.e. the plan gate stopped after the mentor's first plan, before the executor
 * starts. Approve releases the handoff to the executor; reject sends the plan
 * back to the mentor to revise with the (optional) feedback.
 */
export function PlanReviewBar({ pair }: { pair: Pair }): React.ReactNode {
  const { t } = useTranslation()
  const resolvePlanReview = usePairStore((s) => s.resolvePlanReview)
  const [showReject, setShowReject] = useState(false)
  const [feedback, setFeedback] = useState('')
  const [submitting, setSubmitting] = useState(false)

  const run = async (decision: 'approve' | 'reject'): Promise<void> => {
    setSubmitting(true)
    try {
      await resolvePlanReview(
        pair.id,
        decision,
        decision === 'reject' ? feedback.trim() : undefined
      )
    } catch {
      // Store surfaces the error in its `error` field.
    } finally {
      setSubmitting(false)
      setShowReject(false)
      setFeedback('')
    }
  }

  return (
    <div className="pl-[3ch]">
      <div className="rounded-sm border border-state-running/40 bg-state-running/[0.06] p-3 font-mono">
        <div className="mb-1.5 flex items-center gap-1.5 text-[10px] uppercase tracking-[0.16em] state-running">
          <span aria-hidden>▸</span>
          <span className="font-bold">{t('planReview.title')}</span>
        </div>
        <p className="mb-2.5 text-[11px] leading-relaxed text-muted-foreground [overflow-wrap:anywhere]">
          {t('planReview.description')}
        </p>

        {showReject ? (
          <div className="flex flex-col gap-2">
            <textarea
              value={feedback}
              onChange={(e) => setFeedback(e.target.value)}
              placeholder={t('planReview.feedbackPlaceholder')}
              rows={3}
              autoFocus
              disabled={submitting}
              className="w-full resize-none rounded-sm border border-border bg-background px-2 py-1.5 font-mono text-[12px] leading-relaxed text-foreground placeholder:text-muted-foreground-faint focus:border-foreground/60 focus:outline-none"
            />
            <div className="flex items-center gap-2">
              <GlassButton
                variant="primary"
                size="sm"
                icon={<RotateCcw size={11} />}
                onClick={() => void run('reject')}
                disabled={submitting}
              >
                {t('planReview.sendBack')}
              </GlassButton>
              <GlassButton
                variant="ghost"
                size="sm"
                onClick={() => {
                  setShowReject(false)
                  setFeedback('')
                }}
                disabled={submitting}
              >
                {t('common.cancel')}
              </GlassButton>
            </div>
          </div>
        ) : (
          <div className="flex items-center gap-2">
            <GlassButton
              variant="primary"
              size="sm"
              icon={<Check size={11} />}
              onClick={() => void run('approve')}
              disabled={submitting}
            >
              {t('planReview.approve')}
            </GlassButton>
            <GlassButton
              variant="secondary"
              size="sm"
              icon={<RotateCcw size={11} />}
              onClick={() => setShowReject(true)}
              disabled={submitting}
            >
              {t('planReview.reject')}
            </GlassButton>
          </div>
        )}
      </div>
    </div>
  )
}
