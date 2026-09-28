// Simple Svelte 5 i18n store for the desktop app.
import en from './i18n/en.json';
import ru from './i18n/ru.json';
import zh from './i18n/zh.json';

const translations: Record<string, Record<string, string>> = { en, ru, zh };

export const availableLocales = [
  { code: 'en', label: 'English' },
  { code: 'zh', label: '中文' },
  { code: 'ru', label: 'Русский' }
] as const;

let currentLocale = $state('en');

/** Keeps <html lang> in sync so screen readers and Intl pick the right language. */
function syncDocumentLang() {
  if (typeof document !== 'undefined') {
    document.documentElement.lang = currentLocale;
  }
}

export function t(key: string, params?: Record<string, string | number>): string {
    const dict = translations[currentLocale];
    let value = dict?.[key] ?? translations.en?.[key] ?? key;

    if (params) {
        for (const [k, v] of Object.entries(params)) {
            value = value
                .replaceAll(`{${k}}`, String(v))
                .replaceAll(`{{ ${k} }}`, String(v));
        }
    }

    return value;
}

export function setLocale(locale: string) {
    if (translations[locale]) {
        currentLocale = locale;
        syncDocumentLang();
        try { localStorage.setItem('cobalt.locale', locale); } catch { /* private mode */ }
    }
}

export function getLocale(): string {
    return currentLocale;
}

/** Restores a previously chosen locale. Call once from onMount. */
export function initLocale() {
    let stored = '';
    try { stored = localStorage.getItem('cobalt.locale') ?? ''; } catch { /* private mode */ }
    if (stored && translations[stored]) currentLocale = stored;
    syncDocumentLang();
}
