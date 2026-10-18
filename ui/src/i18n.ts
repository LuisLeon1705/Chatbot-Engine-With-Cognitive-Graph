// UI texts. The dictionary lives in the backend (crates/i18n/dictionary.toml): it is
// loaded once at start-up and again when the language changes.
import { invoke } from "@tauri-apps/api/core";

export interface Bundle {
  lang: string;
  languages: { code: string; name: string }[];
  entries: Record<string, string>;
}

let current: Bundle = { lang: "en", languages: [], entries: {} };

export function setBundle(b: Bundle) {
  current = b;
  document.documentElement.lang = b.lang;
}

export const lang = () => current.lang;
export const languages = () => current.languages;

/** Translated text for `key`; `{name}` placeholders are filled from `args`. */
export function t(key: string, args?: Record<string, string | number>): string {
  const text = current.entries[key] ?? key;
  return args ? text.replace(/\{(\w+)\}/g, (m, k: string) => (k in args ? String(args[k]) : m)) : text;
}

export const loadBundle = () => invoke<Bundle>("i18n_bundle");
export const switchLanguage = (code: string) => invoke<Bundle>("set_language", { code });
