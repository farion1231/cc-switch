// Lightweight source-text-keyed i18n for the Codex++ manager UI.
//
// The app is authored in Chinese. Every user-facing Chinese literal is wrapped
// with `t("中文")` (plain strings) or `tf("前缀 {0}", [expr])` (interpolated
// strings) by tools/i18n-codemod.mjs. When the active language is English we
// look the source text up in the English dictionary; otherwise we return the
// original Chinese, so Chinese stays the zero-overhead default.
//
// Language is resolved once at module load. Many Chinese literals live in
// module-level constants (route tables, preset labels, …) that evaluate a
// single time at import, so a live in-place swap can't reach them. Switching
// language therefore persists the choice and reloads the webview, which is
// instant for a local Tauri window and guarantees every literal — module-level
// or render-level — re-evaluates under the new language.

import { EN_BACKEND, EN_BACKEND_PATTERNS, EN_PLAIN, EN_TEMPLATE } from "@/i18n-en";

export type Language = "zh" | "en";

const STORAGE_KEY = "codex-plus-lang";

function resolveInitialLanguage(): Language {
  try {
    return window.localStorage.getItem(STORAGE_KEY) === "en" ? "en" : "zh";
  } catch {
    return "zh";
  }
}

// Resolved once per webview load. Do not mutate at runtime — use setLanguage,
// which persists and reloads so module-level literals pick up the change.
const LANG: Language = resolveInitialLanguage();

export function getLanguage(): Language {
  return LANG;
}

/** Translate a plain Chinese literal. Falls back to the source text. */
export function t(zh: string): string {
  if (LANG !== "en") return zh;
  const plain = EN_PLAIN[zh] ?? EN_BACKEND[zh];
  if (plain) return plain;
  for (const [re, replacement] of EN_BACKEND_PATTERNS) {
    if (re.test(zh)) return zh.replace(re, replacement);
  }
  return zh;
}

/**
 * Translate an interpolated literal. `key` carries `{0}`,`{1}`… placeholders in
 * the original (Chinese) order; `args` are the runtime values for each. In
 * Chinese we substitute into the key itself; in English we substitute into the
 * looked-up template (also falling back to the key).
 */
export function tf(key: string, args: Array<string | number>): string {
  const template = LANG === "en" ? EN_TEMPLATE[key] ?? key : key;
  return template.replace(/\{(\d+)\}/g, (match, index) => {
    const value = args[Number(index)];
    return value === undefined || value === null ? match : String(value);
  });
}

/** Persist a new language and reload so every literal re-evaluates under it. */
export function setLanguage(language: Language): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, language);
  } catch {
    // Ignore storage failures; the reload below simply keeps the old value.
  }
  window.location.reload();
}

/** Flip between Chinese and English. */
export function toggleLanguage(): void {
  setLanguage(LANG === "en" ? "zh" : "en");
}
