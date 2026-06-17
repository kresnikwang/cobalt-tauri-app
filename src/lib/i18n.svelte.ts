// Simple Svelte 5 i18n store for the desktop app.
import en from './i18n/en.json';
import ru from './i18n/ru.json';
import zh from './i18n/zh.json';

const translations: Record<string, Record<string, string>> = { en, ru, zh };

let currentLocale = $state('en');

export function t(key: string, params?: Record<string, string | number>): string {
    const dict = translations[currentLocale];
    let value = dict?.[key] ?? key;

    if (params) {
        for (const [k, v] of Object.entries(params)) {
            value = value.replace(`{{ ${k} }}`, String(v));
        }
    }

    return value;
}

export function setLocale(locale: string) {
    if (translations[locale]) {
        currentLocale = locale;
    }
}

export function getLocale(): string {
    return currentLocale;
}
