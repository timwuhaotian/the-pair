import { useState, useEffect, useCallback } from 'react'
import type { PairPreset } from '../types'
import { getPresets } from './presetUtils'

interface UsePresetsResult {
  presets: PairPreset[]
  loading: boolean
  error: string | null
  reload: () => void
}

export function usePresets(): UsePresetsResult {
  const [presets, setPresets] = useState<PairPreset[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const loadPresets = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const isDev = !import.meta.env.PROD
      setPresets(getPresets(isDev))
    } catch {
      setError('Failed to load presets')
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void loadPresets()
  }, [loadPresets])

  return { presets, loading, error, reload: loadPresets }
}
