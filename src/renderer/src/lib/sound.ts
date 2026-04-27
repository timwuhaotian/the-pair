type SoundName = 'finish-chime' | 'error-alert' | 'pause-confirm'

let audioElements: Map<SoundName, HTMLAudioElement> | null = null
let muted = false

export function setMuted(value: boolean): void {
  muted = value
}

export function isMuted(): boolean {
  return muted
}

function getAudioElements(): Map<SoundName, HTMLAudioElement> {
  if (!audioElements) {
    audioElements = new Map()
  }
  return audioElements
}

export function preloadSounds(): void {
  if (typeof window === 'undefined') return

  const soundFiles: SoundName[] = ['finish-chime', 'error-alert', 'pause-confirm']

  for (const name of soundFiles) {
    const audio = new Audio()
    audio.preload = 'auto'
    audio.src = `/sounds/${name}.mp3`
    getAudioElements().set(name, audio)
  }
}

function playSound(name: SoundName): void {
  if (muted) return
  if (typeof window === 'undefined') return

  const audio = getAudioElements().get(name)
  if (audio) {
    audio.currentTime = 0
    audio.play().catch(() => {
      const fallbackMap: Record<SoundName, () => void> = {
        'finish-chime': playFinishChimeSynth,
        'error-alert': playErrorAlertSynth,
        'pause-confirm': playPauseConfirmSynth
      }
      fallbackMap[name]?.()
    })
  } else {
    const fallbackMap: Record<SoundName, () => void> = {
      'finish-chime': playFinishChimeSynth,
      'error-alert': playErrorAlertSynth,
      'pause-confirm': playPauseConfirmSynth
    }
    fallbackMap[name]?.()
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
  const resumeAudio = () => {
    if (audioContext?.state === 'suspended') {
      void audioContext.resume()
    }
  }
  document.addEventListener('click', resumeAudio, { once: true })
  document.addEventListener('keydown', resumeAudio, { once: true })
}
