import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import en from './locales/en.json'
import zh from './locales/zh.json'
import ja from './locales/ja.json'
import ko from './locales/ko.json'

const browserLocale = navigator.language.split('-')[0]
const defaultLocale = ['en', 'zh', 'ja', 'ko'].includes(browserLocale) ? browserLocale : 'en'

void i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    zh: { translation: zh },
    ja: { translation: ja },
    ko: { translation: ko }
  },
  lng: defaultLocale,
  fallbackLng: 'en',
  interpolation: { escapeValue: false }
})

export { defaultLocale }
export default i18n
