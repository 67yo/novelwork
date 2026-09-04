import { computed, watchEffect } from "vue";
import { useLocalStorage, usePreferredDark } from "@vueuse/core";

export const UI_THEME_PREFS = ["system", "light", "dark"] as const;
export type UiThemePref = (typeof UI_THEME_PREFS)[number];

const STORAGE_KEY = "novework.uiTheme";

export function parseUiThemePref(raw: string | null | undefined): UiThemePref {
  return UI_THEME_PREFS.includes(raw as UiThemePref) ? (raw as UiThemePref) : "system";
}

export function resolveUiDark(pref: UiThemePref, systemDark: boolean): boolean {
  return pref === "dark" || (pref === "system" && systemDark);
}

export function applyUiTheme(dark: boolean) {
  const root = document.documentElement;
  root.classList.toggle("dark", dark);
  root.style.colorScheme = dark ? "dark" : "light";
}

export function bootUiTheme() {
  let pref: UiThemePref = "system";
  try {
    pref = parseUiThemePref(localStorage.getItem(STORAGE_KEY));
  } catch {
    /* ignore */
  }
  const systemDark =
    typeof matchMedia === "function" && matchMedia("(prefers-color-scheme: dark)").matches;
  applyUiTheme(resolveUiDark(pref, systemDark));
}

export function useUiTheme() {
  const pref = useLocalStorage<UiThemePref>(STORAGE_KEY, "system");
  const systemDark = usePreferredDark();
  const isDark = computed(() =>
    resolveUiDark(parseUiThemePref(pref.value), systemDark.value),
  );
  watchEffect(() => {
    applyUiTheme(isDark.value);
  });
  function setPref(next: UiThemePref) {
    pref.value = parseUiThemePref(next);
  }
  return { pref, isDark, setPref };
}
