import { useSyncExternalStore } from "react";
import { getLanguagePreference, getLocale, subscribeLanguage } from "./i18n";

export function useLanguage() {
  const preference = useSyncExternalStore(subscribeLanguage, getLanguagePreference);
  const locale = useSyncExternalStore(subscribeLanguage, getLocale);
  return { preference, locale };
}
