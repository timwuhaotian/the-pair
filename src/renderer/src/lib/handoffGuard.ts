export interface HandoffEventLike {
  pairStatus?: string | null
  backendStatus?: string | null
}

export function shouldIgnoreHandoffEvent({ pairStatus, backendStatus }: HandoffEventLike): boolean {
  return (
    backendStatus === 'Finished' ||
    pairStatus === 'Finished' ||
    backendStatus === 'Paused' ||
    pairStatus === 'Paused' ||
    backendStatus === 'Error' ||
    pairStatus === 'Error'
  )
}
