/**
 * Turning one destination path into the parent and the new folder name the
 * clone command needs.
 *
 * The clone backend takes the two halves separately so it can create the folder
 * itself and refuse a target that already exists. The interface asks for the
 * full path once, which is the only thing a person has in mind, and splits it
 * here. Windows separators, drive letters, and UNC shares are recognised so the
 * same code serves every platform.
 */

export type CloneDestinationError = 'empty' | 'relative' | 'home' | 'no-name';

export type CloneDestinationResult =
  | { ok: true; parent: string; directoryName: string }
  | { ok: false; reason: CloneDestinationError };

const DRIVE = /^[a-zA-Z]:\\?$/;
const ABSOLUTE = /^(\/|\\\\|[a-zA-Z]:[\\/])/;

export function splitCloneDestination(input: string): CloneDestinationResult {
  const trimmed = input.trim();
  if (!trimmed) return { ok: false, reason: 'empty' };
  // A shell would expand this, Git would not, and a literal `~` directory is
  // never what the person meant.
  if (trimmed.startsWith('~')) return { ok: false, reason: 'home' };

  const path = trimmed.replace(/[\\/]+$/, '');
  const index = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  if (index < 0) return { ok: false, reason: 'relative' };

  const directoryName = path.slice(index + 1);
  if (!directoryName || directoryName === '.' || directoryName === '..') {
    return { ok: false, reason: 'no-name' };
  }

  const separator = path[index] ?? '/';
  let parent = path.slice(0, index);
  if (!parent) {
    // `/repo` keeps the root as its parent.
    parent = separator;
  } else if (DRIVE.test(parent)) {
    // `C:\repo` needs the separator back to name the drive root.
    parent = `${parent.replace(/\\$/, '')}${separator}`;
  }
  if (!ABSOLUTE.test(parent)) return { ok: false, reason: 'relative' };

  return { ok: true, parent, directoryName };
}

/** Join a folder chosen in the directory picker with the suggested name. */
export function joinCloneDestination(parent: string, directoryName: string): string {
  if (!parent) return directoryName;
  const separator = parent.includes('\\') && !parent.includes('/') ? '\\' : '/';
  const trimmed = parent.replace(/[\\/]+$/, '');
  // A drive root keeps its separator, or `C:` alone would name the drive.
  if (!trimmed) return `${parent}${directoryName}`;
  return `${trimmed}${separator}${directoryName}`;
}

/** The folder name a remote URL suggests, without its `.git` suffix. */
export function suggestedCloneName(remote: string): string {
  const tail =
    remote
      .trim()
      .replace(/[\\/]+$/, '')
      .split(/[/:\\]/)
      .pop() ?? '';
  return tail.replace(/\.git$/i, '');
}
