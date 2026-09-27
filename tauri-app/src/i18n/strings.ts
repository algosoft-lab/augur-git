/**
 * Translation helper.
 *
 * The catalogs are already merged with their English fallbacks by the backend,
 * so a lookup either finds a string or returns the key. Returning the key makes
 * a missing translation obvious during development instead of rendering an
 * empty control.
 */

import type { Translator } from "../i18n";

export function t(translate: Translator, key: string): string {
  return translate(key);
}

export function ta(
  translate: Translator,
  key: string,
  args: Record<string, string | number>,
): string {
  return translate(key, args);
}
