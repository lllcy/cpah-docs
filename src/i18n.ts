import english from "./locales/en.json" with { type: "json" };

export type Locale = "en" | "zh-CN";
export type LanguagePreference = "system" | Locale;
export type MessageKey = keyof typeof english;
type Parameters = Record<string, string | number>;
export const LANGUAGE_STORAGE_KEY = "cpah-language";
const listeners = new Set<() => void>();

export function resolveLocale(preference: LanguagePreference, languages: readonly string[] = []): Locale {
  if (preference !== "system") return preference;
  return languages[0]?.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

function readPreference(): LanguagePreference {
  try {
    const saved = typeof localStorage === "undefined" ? null : localStorage.getItem(LANGUAGE_STORAGE_KEY);
    return saved === "en" || saved === "zh-CN" ? saved : "system";
  } catch { return "system"; }
}

function systemLanguages(): readonly string[] {
  if (typeof navigator === "undefined") return [];
  return navigator.languages?.length ? navigator.languages : [navigator.language];
}

let preference = readPreference();
let locale = resolveLocale(preference, systemLanguages());
export const getLocale = () => locale;
export const getLanguagePreference = () => preference;
export function subscribeLanguage(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

function applyLocale() {
  if (typeof document !== "undefined") document.documentElement.lang = locale;
}

export function setLanguagePreference(value: LanguagePreference) {
  preference = value;
  locale = resolveLocale(value, systemLanguages());
  try { if (typeof localStorage !== "undefined") localStorage.setItem(LANGUAGE_STORAGE_KEY, value); }
  catch { /* Language switching still works when storage is unavailable. */ }
  applyLocale();
  listeners.forEach((listener) => listener());
}

// The Chinese source messages are stable catalog keys, similar to gettext msgids.
// Unknown text (including filenames, user categories and provider errors) is preserved.
export function translate(message: string): string {
  if (locale === "zh-CN") return message;
  return Object.hasOwn(english, message) ? english[message as MessageKey] : message;
}

// Health checks append a fixed label to a user-defined profile name.
// Translate only that label, even when the name contains the same separator.
export function translateHealthTitle(title: string): string {
  const separator = title.lastIndexOf(" · ");
  if (separator === -1) return translate(title);
  const label = title.slice(separator + 3);
  if (!["监控目录", "输出目录", "目录关系"].includes(label)) return title;
  return title.slice(0, separator + 3) + translate(label);
}

export function t(key: MessageKey, parameters: Parameters = {}): string {
  return translate(key).replace(/\{(\w+)\}/g, (placeholder, name: string) =>
    Object.hasOwn(parameters, name) ? String(parameters[name]) : placeholder);
}

export function formatNumber(value: number): string {
  return new Intl.NumberFormat(locale).format(value);
}

applyLocale();
if (typeof window !== "undefined") window.addEventListener("languagechange", () => {
  if (preference === "system") setLanguagePreference("system");
});
