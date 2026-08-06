import { computed, ref } from "vue";
import {
  dictionaries,
  type LocaleCode,
  type LocalePreference,
  type MessageKey,
  LOCALE_OPTIONS,
} from "./messages";

export { LOCALE_OPTIONS, type LocaleCode, type LocalePreference, type MessageKey };

const preference = ref<LocalePreference>("system");

export function detectSystemLocale(): LocaleCode {
  const lang = (typeof navigator !== "undefined" ? navigator.language : "en") || "en";
  const lower = lang.toLowerCase();
  if (lower.startsWith("zh")) {
    if (lower.includes("tw") || lower.includes("hk") || lower.includes("hant") || lower.includes("mo")) {
      return "zh-TW";
    }
    return "zh-CN";
  }
  if (lower.startsWith("ja")) return "ja";
  if (lower.startsWith("de")) return "de";
  if (lower.startsWith("fr")) return "fr";
  if (lower.startsWith("en")) return "en";
  return "en";
}

export const resolvedLocale = computed<LocaleCode>(() =>
  preference.value === "system" ? detectSystemLocale() : preference.value,
);

export function getLocalePreference(): LocalePreference {
  return preference.value;
}

export function setLocalePreference(next: LocalePreference) {
  preference.value = next;
}

export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const dict = dictionaries[resolvedLocale.value] ?? dictionaries.en;
  let s = dict[key] ?? dictionaries.en[key] ?? String(key);
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      s = s.split(`{${k}}`).join(String(v));
    }
  }
  return s;
}

/** Reactive helper for templates: use `tt('nav.novels')` so locale switches re-render. */
export function useI18n() {
  return {
    locale: resolvedLocale,
    preference,
    t: (key: MessageKey, params?: Record<string, string | number>) => {
      // touch resolvedLocale so computed dependents update
      void resolvedLocale.value;
      return t(key, params);
    },
  };
}
