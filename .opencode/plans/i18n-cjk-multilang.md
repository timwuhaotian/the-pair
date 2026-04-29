# i18n: CJK Multi-language Support Implementation Plan

## Overview

Add Chinese (zh), Japanese (ja), and Korean (ko) language support alongside English (en) to "The Pair" desktop app using i18next + react-i18next.

## Architecture

- **Library**: `i18next` + `react-i18next` (already installed)
- **Locale files**: Single JSON per language at `src/renderer/src/locales/{en,zh,ja,ko}.json`
- **Namespace organization**: common, chrome, dashboard, emptyState, onboarding, modals, pickers, pair, console, errors, updates, history, shortcuts
- **Persistence**: New `useLocaleStore` (zustand + persist) alongside `useThemeStore`
- **Font**: Add CJK fallback chain to `--font-sans` in `main.css`
- **Default**: English, auto-detect browser language on first visit
- **Language Switcher**: Glass-morphism popover in AppChrome next to theme toggle

---

## File Changes

### 1. New File: `src/renderer/src/locales/en.json`

English translation source with all namespaces. Keys organized by domain:

```json
{
  "common": { "cancel": "Cancel", "delete": "Delete", "save": "Save", "retry": "Retry", ... },
  "chrome": { "title": "The Pair", "newTask": "New Task", "newPair": "New Pair", ... },
  "dashboard": { "title": "Pair Containers", "description": "...", "run": "Run {{count}}", ... },
  "emptyState": { "title": "No Pair Containers Yet", "mentorDesc": "...", "handoffDesc": "...", ... },
  "onboarding": { "title": "Setup Wizard", "systemHealth": "System Health", ... },
  "modals": { "createTitle": "Create New Pair", "pairDefaults": "Pair Defaults · {{name}}", ... },
  "pickers": { "mentorLabel": "Mentor", "executorLabel": "Executor", "reasoningDeep": "Deep", ... },
  "pair": { "resumePair": "Resume Pair", "pausePair": "Pause Pair", ... },
  "console": { "all": "All", "runningCommand": "Running Command", ... },
  "errors": { "executionError": "Execution Error", "iterationLimit": "..." },
  "updates": { "install": "Install v{{version}}", ... },
  "history": { "title": "Task History", ... },
  "shortcuts": { "pausePair": "Pause Pair", ... }
}
```

### 2. New File: `src/renderer/src/locales/zh.json`

Simplified Chinese translations:

```json
{
  "common": { "cancel": "取消", "delete": "删除", "save": "保存", "retry": "重试", ... },
  "chrome": { "title": "The Pair", "newTask": "新任务", "newPair": "新建 Pair", ... },
  "dashboard": { "title": "Pair 容器", "description": "每个 Pair 保留其工作区、默认设置和任务历史...", ... },
  "emptyState": { "title": "还没有 Pair 容器", "mentorDesc": "分析任务并审查执行器输出", "handoffDesc": "智能体自动交替控制", ... },
  "onboarding": { "title": "设置向导", "systemHealth": "系统健康", ... },
  ...
}
```

### 3. New File: `src/renderer/src/locales/ja.json`

Japanese translations:

```json
{
  "common": { "cancel": "キャンセル", "delete": "削除", "save": "保存", "retry": "再試行", ... },
  "chrome": { "title": "The Pair", "newTask": "新しいタスク", "newPair": "新しいペア", ... },
  ...
}
```

### 4. New File: `src/renderer/src/locales/ko.json`

Korean translations:

```json
{
  "common": { "cancel": "취소", "delete": "삭제", "save": "저장", "retry": "다시 시도", ... },
  "chrome": { "title": "The Pair", "newTask": "새 작업", "newPair": "새 페어", ... },
  ...
}
```

### 5. New File: `src/renderer/src/i18n.ts`

```typescript
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
```

### 6. New File: `src/renderer/src/store/useLocaleStore.ts`

```typescript
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
```

### 7. New File: `src/renderer/src/components/LanguageSwitcher.tsx`

Glass-morphism dropdown with Framer Motion:

```typescript
import React, { useState, useRef, useEffect } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { Languages, Check } from 'lucide-react'
import { useLocaleStore, SupportedLocale, LOCALE_LABELS } from '../store/useLocaleStore'
import { cn } from '../lib/utils'

const LOCALES: SupportedLocale[] = ['en', 'zh', 'ja', 'ko']

export function LanguageSwitcher(): React.ReactNode {
  const { locale, setLocale } = useLocaleStore()
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [])

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen(!open)}
        className="flex h-10 w-10 items-center justify-center rounded-2xl border border-border bg-muted/40 text-muted-foreground transition-all hover:bg-muted hover:text-foreground cursor-pointer"
        title="Switch language"
        data-testid="chrome-language-toggle"
      >
        <Languages size={16} />
      </button>

      <AnimatePresence>
        {open && (
          <>
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.15 }}
              className="fixed inset-0 z-40"
              onClick={() => setOpen(false)}
            />
            <motion.div
              initial={{ opacity: 0, y: -8, scale: 0.96 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: -8, scale: 0.96 }}
              transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
              className="absolute right-0 top-12 z-50 w-48 rounded-2xl border border-border/60 bg-background/95 p-1.5 shadow-xl backdrop-blur-xl"
            >
              <div className="mb-1 px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.15em] text-muted-foreground">
                Language
              </div>
              {LOCALES.map((l) => {
                const isActive = locale === l
                const meta = LOCALE_LABELS[l]
                return (
                  <button
                    key={l}
                    onClick={() => { setLocale(l); setOpen(false) }}
                    className={cn(
                      'flex w-full items-center gap-2.5 rounded-xl px-2.5 py-2 text-sm transition-colors',
                      isActive
                        ? 'bg-primary/10 text-primary'
                        : 'text-foreground hover:bg-muted/60'
                    )}
                  >
                    <span className="text-base">{meta.flag}</span>
                    <span className="flex-1 text-left font-medium">{meta.native}</span>
                    {isActive && <Check size={14} className="text-primary" />}
                  </button>
                )
              })}
            </motion.div>
          </>
        )}
      </AnimatePresence>
    </div>
  )
}
```

### 8. Edit: `src/renderer/src/assets/main.css`

Add CJK font fallback to `--font-sans`:

```css
/* Change line 39-41 from: */
--font-sans:
  'Geist Sans', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell,
  'Helvetica Neue', sans-serif;

/* To: */
--font-sans:
  'Geist Sans', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell,
  'PingFang SC', 'Hiragino Sans', 'Hiragino Kaku Gothic ProN', 'Meiryo', 'Noto Sans KR',
  'Helvetica Neue', sans-serif;
```

### 9. Edit: `src/renderer/src/App.tsx`

- Add `import './i18n'` at top (side-effect import to initialize)
- The rest uses `useTranslation` hook per-component

### 10. Edit: `src/renderer/src/components/AppChrome.tsx`

- Import `useTranslation` and `LanguageSwitcher`
- Replace hardcoded strings with `t()`:
  - `'The Pair'` → `t('chrome.title')`
  - `'New Task'` → `t('chrome.newTask')`
  - `'New Pair'` → `t('chrome.newPair')`
  - `'Models queued'` → `t('chrome.modelsQueued')`
  - `'Detecting models...'` → `t('chrome.detectingModels')`
  - Model ready text → `t('chrome.modelsReady', { ready, total })`
- Add `<LanguageSwitcher />` between mute toggle and theme toggle

### 11-25. Edit all remaining components

Replace hardcoded strings with `t('namespace.key')` calls. Full mapping:

| Component                   | Keys Used                                                                                 |
| --------------------------- | ----------------------------------------------------------------------------------------- |
| `Dashboard.tsx`             | `dashboard.*`                                                                             |
| `DashboardEmptyState.tsx`   | `emptyState.*`                                                                            |
| `OnboardingWizard.tsx`      | `onboarding.*`, `pickers.workflowPreset`                                                  |
| `CreatePairModal.tsx`       | `modals.createTitle`, `modals.choosePreset`, `modals.creating`, `modals.create`, + common |
| `AssignTaskModal.tsx`       | `modals.restoreTask`, `modals.assignTask`, `modals.workspaceLabel`, etc.                  |
| `PairSettingsModal.tsx`     | `modals.pairDefaults`, `modals.pairDefaultsDesc`, etc.                                    |
| `SessionRecoveryModal.tsx`  | `modals.confirmDelete`, `modals.recoverTitle`, etc.                                       |
| `ModelPicker.tsx`           | `pickers.mentorLabel`, `pickers.executorLabel`, `pickers.recent`, etc.                    |
| `PresetPicker.tsx`          | `pickers.noPresets`, `pickers.presetsError`, `pickers.presetInfo`                         |
| `BranchPicker.tsx`          | `pickers.selectBranch`, `pickers.filterBranches`, `pickers.localBranches`, etc.           |
| `ReasoningEffortPicker.tsx` | `pickers.reasoning*`, `pickers.resetReasoning`                                            |
| `PairDetail.tsx`            | `pair.*`, `console.sessionConsole`, `console.taskHistory`                                 |
| `MessageCard.tsx`           | `console.missionSpecs`, `console.acceptance`, `console.collapse`, `console.expand`        |
| `ActivityIndicator.tsx`     | `console.stalled`, `console.noOutput`, `console.error`, `console.thinking`, etc.          |
| `IterationProgress.tsx`     | `errors.iterations`, `errors.iterationsAdaptive`, `errors.iterationLimit`                 |
| `ErrorDetailPanel.tsx`      | `errors.executionError`, `errors.*`, `console.*`                                          |
| `MessageFilterBar.tsx`      | `console.all` + pickers.mentorLabel/executorLabel                                         |
| `ScrollToBottomButton.tsx`  | `console.newMessages`                                                                     |
| `UpdateControls.tsx`        | `updates.*`                                                                               |
| `UpdateNotification.tsx`    | `updates.available`, `updates.remindLater`, `updates.installingPercent`                   |
| `TaskHistoryPanel.tsx`      | `history.*`                                                                               |
| `IntentChip.tsx`            | `console.runningCommand`, `console.readingFile`, `console.writingCode`, etc.              |
| `ConfirmModal.tsx`          | `common.cancel`, `common.delete`                                                          |

---

## Translation Data (All 4 Languages)

Complete zh.json, ja.json, ko.json content is provided in the companion files:

- `.opencode/plans/i18n-zh.json`
- `.opencode/plans/i18n-ja.json`
- `.opencode/plans/i18n-ko.json`

---

## Execution Order

1. `npm install i18next react-i18next` (done)
2. Create `src/renderer/src/locales/{en,zh,ja,ko}.json`
3. Create `src/renderer/src/i18n.ts`
4. Create `src/renderer/src/store/useLocaleStore.ts`
5. Create `src/renderer/src/components/LanguageSwitcher.tsx`
6. Edit `src/renderer/src/assets/main.css` (CJK font)
7. Edit `src/renderer/src/App.tsx` (i18n init)
8. Edit `src/renderer/src/components/AppChrome.tsx` (add switcher + translate)
9. Edit all 20+ component files (replace hardcoded strings)
10. Run `npm run typecheck && npm run lint`

---

## Testing Checklist

- [ ] Language switcher appears in chrome bar
- [ ] Dropdown opens/closes with animation
- [ ] Switching to zh/ja/ko translates all visible UI text
- [ ] CJK characters render correctly (no tofu/boxes)
- [ ] Language persists after app restart
- [ ] AppChrome strings translated
- [ ] Dashboard + empty state translated
- [ ] Onboarding wizard translated
- [ ] All modals translated
- [ ] All pickers translated
- [ ] Pair detail view translated
- [ ] Console messages translated
- [ ] Error panel translated
- [ ] IntentChip labels translated (was hardcoded Chinese)
- [ ] Typecheck passes
- [ ] Lint passes
