/**
 * Branch-name validation.
 *
 * The rules mirror `git check-ref-format` closely enough to catch what a user
 * can type into a dialog, and they run on every keystroke so the confirm button
 * reflects the input as it is edited.
 */

export type NameError = 'empty' | 'invalid' | 'exists';

const INVALID_CHARACTERS = new Set([' ', '~', '^', ':', '?', '*', '[', '\\']);

export function validateBranchName(
  name: string,
  existing: string[],
  allow?: string
): NameError | null {
  if (name.length === 0) {
    return 'empty';
  }
  if (
    name.startsWith('-') ||
    name.startsWith('.') ||
    name.startsWith('/') ||
    name.endsWith('/') ||
    name.endsWith('.') ||
    name.endsWith('.lock') ||
    name.includes('..') ||
    name.includes('//') ||
    name.includes('@{') ||
    [...name].some((character) => INVALID_CHARACTERS.has(character) || character < ' ')
  ) {
    return 'invalid';
  }
  if (allow !== name && existing.includes(name)) {
    return 'exists';
  }
  return null;
}
