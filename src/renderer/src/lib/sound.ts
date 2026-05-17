type SoundName = 'finish-chime' | 'error-alert' | 'pause-confirm'

const SOUND_SOURCES: Record<SoundName, string> = {
  'finish-chime': '/sounds/finish-chime.mp3',
  'error-alert': '/sounds/error-alert.mp3',
  'pause-confirm': '/sounds/pause-confirm.mp3'
}

const MUTED_STORAGE_KEY = 'thePair.soundMuted'

let audioTemplates: Map<SoundName, HTMLAudioElement> | null = null
let muted = readPersistedMuted()

function hasLocalStorage(): boolean {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined'
}

function readPersistedMuted(): boolean {
  if (!hasLocalStorage()) return false
  try {
    return window.localStorage.getItem(MUTED_STORAGE_KEY) === 'true'
  } catch {
    return false
  }
}

function writePersistedMuted(value: boolean): void {
  if (!hasLocalStorage()) return
  try {
    window.localStorage.setItem(MUTED_STORAGE_KEY, String(value))
  } catch {
    // ignore quota / privacy errors
  }
}

export function setMuted(value: boolean): void {
  muted = value
  writePersistedMuted(value)
}

export function isMuted(): boolean {
  return muted
}

function getAudioTemplates(): Map<SoundName, HTMLAudioElement> {
  if (!audioTemplates) {
    audioTemplates = new Map()
  }
  return audioTemplates
}

export function preloadSounds(): void {
  if (typeof window === 'undefined') return

  for (const name of Object.keys(SOUND_SOURCES) as SoundName[]) {
    const audio = new Audio()
    audio.preload = 'auto'
    audio.src = SOUND_SOURCES[name]
    getAudioTemplates().set(name, audio)
  }
}

function runFallbackSynth(name: SoundName): void {
  const fallbackMap: Record<SoundName, () => void> = {
    'finish-chime': playFinishChimeSynth,
    'error-alert': playErrorAlertSynth,
    'pause-confirm': playPauseConfirmSynth
  }
  fallbackMap[name]?.()
}

function playSound(name: SoundName): void {
  if (muted) return
  if (typeof window === 'undefined') return

  const template = getAudioTemplates().get(name)
  if (template) {
    // Clone so simultaneous fires (multiple pairs finishing at once) do not
    // interrupt each other by rewinding the shared element.
    const clone = template.cloneNode(true) as HTMLAudioElement
    const playPromise = clone.play()
    if (playPromise && typeof playPromise.catch === 'function') {
      playPromise.catch(() => runFallbackSynth(name))
    }
  } else {
    runFallbackSynth(name)
  }
}

export function playFinishChime(): void {
  playSound('finish-chime')
}

export function playErrorAlert(): void {
  playSound('error-alert')
}

export function playPauseConfirm(): void {
  playSound('pause-confirm')
}

// --- Web Audio API Fallback Synthesis ---

let audioContext: AudioContext | null = null
let audioApiAvailable = true

function getAudioContext(): AudioContext | null {
  if (!audioApiAvailable) return null
  if (!audioContext) {
    try {
      audioContext = new AudioContext()
    } catch {
      audioApiAvailable = false
      return null
    }
  }
  return audioContext
}

function playFinishChimeSynth(): void {
  try {
    const ctx = getAudioContext()
    if (!ctx) return
    if (ctx.state === 'suspended') {
      ctx.resume()
    }
    const masterGain = ctx.createGain()
    masterGain.gain.value = 0
    masterGain.connect(ctx.destination)
    const notes = [523.25, 659.25]
    notes.forEach((freq, i) => {
      const osc = ctx.createOscillator()
      const noteGain = ctx.createGain()
      osc.type = 'sine'
      osc.frequency.value = freq
      noteGain.gain.setValueAtTime(0, ctx.currentTime + i * 0.18)
      noteGain.gain.linearRampToValueAtTime(0.12, ctx.currentTime + i * 0.18 + 0.02)
      noteGain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + i * 0.18 + 0.35)
      osc.connect(noteGain)
      noteGain.connect(masterGain)
      osc.start(ctx.currentTime + i * 0.18)
      osc.stop(ctx.currentTime + i * 0.18 + 0.4)
    })
    masterGain.gain.value = 1
  } catch {
    audioApiAvailable = false
  }
}

function playErrorAlertSynth(): void {
  try {
    const ctx = getAudioContext()
    if (!ctx) return
    const osc = ctx.createOscillator()
    const gain = ctx.createGain()
    osc.type = 'square'
    osc.frequency.value = 220
    gain.gain.setValueAtTime(0.08, ctx.currentTime)
    gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.3)
    osc.connect(gain)
    gain.connect(ctx.destination)
    osc.start(ctx.currentTime)
    osc.stop(ctx.currentTime + 0.35)
  } catch {
    audioApiAvailable = false
  }
}

function playPauseConfirmSynth(): void {
  try {
    const ctx = getAudioContext()
    if (!ctx) return
    const osc = ctx.createOscillator()
    const gain = ctx.createGain()
    osc.type = 'sine'
    osc.frequency.value = 440
    gain.gain.setValueAtTime(0.06, ctx.currentTime)
    gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.15)
    osc.connect(gain)
    gain.connect(ctx.destination)
    osc.start(ctx.currentTime)
    osc.stop(ctx.currentTime + 0.2)
  } catch {
    audioApiAvailable = false
  }
}

// Resume audio context on first user interaction
if (typeof document !== 'undefined') {
  const resumeAudio = (): void => {
    if (audioContext?.state === 'suspended') {
      void audioContext.resume()
    }
  }
  document.addEventListener('click', resumeAudio, { once: true })
  document.addEventListener('keydown', resumeAudio, { once: true })
}
