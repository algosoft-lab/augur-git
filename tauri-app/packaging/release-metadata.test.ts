import { describe, expect, it } from 'vitest';

import { nextNightlyVersion } from './nightly-version.mjs';
import { renderCask } from './render-homebrew-cask.mjs';
import { renderUpdaterManifest } from './render-updater-manifest.mjs';

describe('nightly release metadata', () => {
  it('increments the patch version and uses the workflow run as a monotonic prerelease', () => {
    expect(nextNightlyVersion('0.1.0', 42)).toBe('0.1.1-nightly.42');
    expect(() => nextNightlyVersion('0.1.0-beta.1', 42)).toThrow();
    expect(() => nextNightlyVersion('0.1.0', 0)).toThrow();
  });

  it('renders only a signed Windows feed pointing at the matching immutable release', () => {
    const manifest = JSON.parse(
      renderUpdaterManifest({
        version: '0.1.1-nightly.42',
        url: 'https://github.com/algosoft-lab/augur-git/releases/download/tauri-nightly-0.1.1-nightly.42/augur-git-tauri-windows-x86_64-nsis-0.1.1-nightly.42.exe',
        signature: 'trusted signature\n',
        pubDate: '2026-09-30T00:00:00Z'
      })
    );
    expect(manifest.platforms['windows-x86_64'].signature).toBe('trusted signature');
    expect(manifest.platforms['windows-x86_64'].url).toContain('tauri-nightly-0.1.1-nightly.42');
    expect(() =>
      renderUpdaterManifest({
        version: '0.1.1-nightly.42',
        url: 'https://example.com/update.exe',
        signature: 'signature'
      })
    ).toThrow();
    expect(() =>
      renderUpdaterManifest({
        version: '0.1.1-nightly.42',
        url: 'https://github.com/algosoft-lab/augur-git/releases/download/tauri-nightly-0.1.1-nightly.42/other.exe',
        signature: 'signature'
      })
    ).toThrow();
  });

  it('pins the Homebrew cask to a valid checksum and immutable DMG URL', () => {
    const cask = renderCask({
      version: '0.1.1-nightly.42',
      sha256: 'a'.repeat(64)
    });
    expect(cask).toContain('sha256 "' + 'a'.repeat(64) + '"');
    expect(cask).toContain('releases/download/tauri-nightly-0.1.1-nightly.42/');
    expect(cask).toContain('depends_on arch: :arm64');
    expect(() => renderCask({ version: '0.1.1-nightly.42', sha256: 'invalid' })).toThrow();
  });
});
