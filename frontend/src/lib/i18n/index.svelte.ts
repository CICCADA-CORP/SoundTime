import en from "./translations/en";
import fr from "./translations/fr";
import es from "./translations/es";
import zh from "./translations/zh";
import ru from "./translations/ru";

export type TranslationKey = keyof typeof en;
export type Locale = "en" | "fr" | "es" | "zh" | "ru";

const translations: Record<Locale, Record<string, string>> = { en, fr, es, zh, ru };

export const localeNames: Record<Locale, string> = {
  en: "English",
  fr: "Français",
  es: "Español",
  zh: "中文",
  ru: "Русский",
};

export const supportedLocales: Locale[] = ["en", "fr", "es", "zh", "ru"];

const STORAGE_KEY = "soundtime_lang";

/** Detect the best locale from the browser */
function detectLocale(): Locale {
  if (typeof window === "undefined") return "en";
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored && supportedLocales.includes(stored as Locale)) return stored as Locale;
  const langs = navigator.languages ?? [navigator.language];
  for (const lang of langs) {
    const code = lang.split("-")[0].toLowerCase();
    if (supportedLocales.includes(code as Locale)) return code as Locale;
  }
  return "en";
}

// Reactive locale state using Svelte 5 runes via module-level $state
let currentLocale = $state<Locale>(detectLocale());

export function getLocale(): Locale {
  return currentLocale;
}

export function setLocale(locale: Locale) {
  currentLocale = locale;
  if (typeof window !== "undefined") {
    localStorage.setItem(STORAGE_KEY, locale);
    document.documentElement.lang = locale;
  }
}

/** Reactive translation function — use as `$t('key')` in .svelte files */
export function t(key: TranslationKey, params?: Record<string, string | number>): string {
  const dict = translations[currentLocale] ?? translations.en;
  let text = dict[key] ?? translations.en[key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      text = text.replace(`{${k}}`, String(v));
    }
  }
  return text;
}

// Initialize lang attribute
if (typeof window !== "undefined") {
  document.documentElement.lang = getLocale();
}
