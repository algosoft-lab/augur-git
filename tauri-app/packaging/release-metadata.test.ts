import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
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

  it('accepts the workflow signature-file option and writes the updater feed', async () => {
    const directory = await mkdtemp(join(tmpdir(), 'augur-updater-feed-'));
    const signaturePath = join(directory, 'installer.exe.sig');
    const outputPath = join(directory, 'latest.json');
    const scriptPath = join(process.cwd(), 'packaging/render-updater-manifest.mjs');

    try {
      await writeFile(signaturePath, 'signed installer');
      const result = spawnSync(
        process.execPath,
        [
          scriptPath,
          '--version',
          '0.1.1-nightly.42',
          '--url',
          'https://github.com/algosoft-lab/augur-git/releases/download/tauri-nightly-0.1.1-nightly.42/augur-git-tauri-windows-x86_64-nsis-0.1.1-nightly.42.exe',
          '--signature-file',
          signaturePath,
          '--out',
          outputPath
        ],
        { encoding: 'utf8' }
      );

      expect(result.status, result.stderr).toBe(0);
      const feed = JSON.parse(await readFile(outputPath, 'utf8'));
      expect(feed.platforms['windows-x86_64'].signature).toBe('signed installer');
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });

  it('pins the Homebrew cask to a valid checksum and immutable DMG URL', () => {
    const cask = renderCask({
      version: '0.1.1-nightly.42',
      sha256: 'a'.repeat(64)
    });
    expect(cask).toContain('sha256 "' + 'a'.repeat(64) + '"');
    expect(cask).toContain('releases/download/tauri-nightly-0.1.1-nightly.42/');
    expect(cask).toContain('depends_on arch: :arm64');
    expect(cask).toContain(
      'binary "#{appdir}/Augur Git Tauri.app/Contents/MacOS/agit", target: "agit"'
    );
    expect(() => renderCask({ version: '0.1.1-nightly.42', sha256: 'invalid' })).toThrow();
  });
});
