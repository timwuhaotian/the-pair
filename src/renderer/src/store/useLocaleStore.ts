import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import i18n from '../i18n'

export type SupportedLocale = 'en' | 'zh' | 'ja' | 'ko'

export const LOCALE_LABELS: Record<SupportedLocale, { native: string; flag: string }> = {
  en: { native: 'English', flag: '🇺🇸' },
  zh: { native: '简体中文', flag: '🇨🇳' },
  ja: { native: '日本語', flag: '🇯🇵' },
  ko: { native: '한국어', flag: '🇰🇷' }
}

interface LocaleStore {
  locale: SupportedLocale
  setLocale: (locale: SupportedLocale) => void
}

export const useLocaleStore = create<LocaleStore>()(
  persist(
    (set) => ({
      locale: 'en',
      setLocale: (locale) => {
        void i18n.changeLanguage(locale)
        set({ locale })
      }
    }),
    { name: 'locale-storage' }
  )
)
