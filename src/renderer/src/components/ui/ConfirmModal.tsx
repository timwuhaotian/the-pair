import React, { useId } from 'react'
import { AlertTriangle } from 'lucide-react'
import { cn } from '../../lib/utils'
import { GlassButton } from './GlassButton'
import { GlassModal } from './GlassModal'
import { ModalFooter } from './ModalFooter'

interface ConfirmModalProps {
  isOpen: boolean
  title: string
  message: string
  confirmLabel?: string
  cancelLabel?: string
  variant?: 'destructive' | 'warning'
  onConfirm: () => void
  onCancel: () => void
}

/** Yes/no confirmation on the shared modal shell (focus trap, Esc, backdrop). */
export function ConfirmModal({
  isOpen,
  title,
  message,
  confirmLabel = 'Confirm',
  cancelLabel = 'Cancel',
  variant = 'destructive',
  onConfirm,
  onCancel
}: ConfirmModalProps): React.ReactNode {
  const toneClass = variant === 'destructive' ? 'state-error' : 'state-running'
  // The message is what the user is confirming: screen readers announce it
  // with the dialog's name, not only when they wander into the body.
  const messageId = useId()

  return (
    <GlassModal
      isOpen={isOpen}
      onClose={onCancel}
      title={title}
      describedBy={messageId}
      className="max-w-md"
    >
      <div className="flex items-baseline gap-2">
        <AlertTriangle size={12} className={cn('shrink-0 translate-y-px', toneClass)} />
        <p id={messageId} className="text-[13px] leading-relaxed text-foreground/90">
          {message}
        </p>
      </div>
      <ModalFooter>
        <GlassButton variant="secondary" onClick={onCancel} data-testid="confirm-modal-cancel">
          {cancelLabel}
        </GlassButton>
        <GlassButton
          variant={variant === 'destructive' ? 'destructive' : 'primary'}
          onClick={onConfirm}
          data-testid="confirm-modal-confirm"
        >
          {confirmLabel}
        </GlassButton>
      </ModalFooter>
    </GlassModal>
  )
}
