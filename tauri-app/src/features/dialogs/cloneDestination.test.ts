import { describe, expect, it } from 'vitest';

import {
  joinCloneDestination,
  splitCloneDestination,
  suggestedCloneName
} from './cloneDestination';

describe('splitCloneDestination', () => {
  it('splits a POSIX path into the parent and the new folder', () => {
    expect(splitCloneDestination('/home/dev/checkouts/api')).toEqual({
      ok: true,
      parent: '/home/dev/checkouts',
      directoryName: 'api'
    });
  });

  it('keeps the root as the parent of a top-level folder', () => {
    expect(splitCloneDestination('/api')).toEqual({ ok: true, parent: '/', directoryName: 'api' });
  });

  it('ignores trailing separators', () => {
    expect(splitCloneDestination('/home/dev/api/')).toEqual({
      ok: true,
      parent: '/home/dev',
      directoryName: 'api'
    });
  });

  it('reads a trailing separator as a folder that ends the path', () => {
    expect(splitCloneDestination('/home/dev/')).toEqual({
      ok: true,
      parent: '/home',
      directoryName: 'dev'
    });
  });

  it('splits Windows paths and keeps the drive root intact', () => {
    expect(splitCloneDestination('C:\\work\\api')).toEqual({
      ok: true,
      parent: 'C:\\work',
      directoryName: 'api'
    });
    expect(splitCloneDestination('C:\\api')).toEqual({
      ok: true,
      parent: 'C:\\',
      directoryName: 'api'
    });
  });

  it('accepts UNC shares', () => {
    expect(splitCloneDestination('\\\\server\\share\\api')).toEqual({
      ok: true,
      parent: '\\\\server\\share',
      directoryName: 'api'
    });
  });

  it('rejects a bare name, a home shortcut, and unusable segments', () => {
    expect(splitCloneDestination('api')).toEqual({ ok: false, reason: 'relative' });
    expect(splitCloneDestination('~/api')).toEqual({ ok: false, reason: 'home' });
    expect(splitCloneDestination('/home/dev/.')).toEqual({ ok: false, reason: 'no-name' });
    expect(splitCloneDestination('   ')).toEqual({ ok: false, reason: 'empty' });
  });
});

describe('joinCloneDestination', () => {
  it('appends the name to the chosen folder', () => {
    expect(joinCloneDestination('/home/dev/checkouts', 'api')).toBe('/home/dev/checkouts/api');
    expect(joinCloneDestination('/home/dev/checkouts/', 'api')).toBe('/home/dev/checkouts/api');
  });

  it('uses the separator the folder already uses', () => {
    expect(joinCloneDestination('C:\\work', 'api')).toBe('C:\\work\\api');
    expect(joinCloneDestination('C:\\', 'api')).toBe('C:\\api');
  });
});

describe('suggestedCloneName', () => {
  it('drops credentials, the .git suffix, and trailing separators', () => {
    expect(suggestedCloneName('https://gitlab.example.com/team/api.git')).toBe('api');
    expect(suggestedCloneName('git@gitlab.example.com:team/api.git')).toBe('api');
    expect(suggestedCloneName('https://example.com/team/api/')).toBe('api');
  });
});
