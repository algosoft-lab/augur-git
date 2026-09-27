/**
 * Translation lookup.
 *
 * The catalogs are compiled into the Rust binary and handed to the webview at
 * startup, so the interface and the native menu can never disagree about a
 * translation. A key missing from the active locale resolves through the
 * English entry that the backend already merged in, and a key missing from
 * both renders as the key itself.
 */

import type { LanguagePreference } from "../bridge/types";

export type Catalog = Record<string, string>;

const PLACEHOLDER = /\{\s*\$([a-zA-Z0-9_]+)\s*\}/g;

/** Build a lookup function for one locale. */
export function createTranslator(
  locale: string,
  catalogs: Record<string, Catalog>,
): (key: string, args?: Record<string, string | number>) => string {
  const active = catalogs[locale] ?? {};
  const fallback = catalogs["en-US"] ?? {};
  return (key, args) => {
    const template = active[key] ?? fallback[key] ?? key;
    if (!args) {
      return template;
    }
    return template.replace(PLACEHOLDER, (match, name: string) => {
      const value = args[name];
      return value === undefined ? match : String(value);
    });
  };
}

export type Translator = ReturnType<typeof createTranslator>;

/** Resolve a persisted preference to a concrete locale id. */
export function resolveLocale(
  preference: LanguagePreference,
  systemLanguage: string,
): string {
  if (preference === "en-US" || preference === "zh-CN") {
    return preference;
  }
  return systemLanguage.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";
}
