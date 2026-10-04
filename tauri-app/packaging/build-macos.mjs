import { cpSync, mkdtempSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  BUNDLE_ROOT,
  assertCli,
  OUTPUT_ROOT,
  assertHostPlatform,
  appendTauriConfig,
  ensureOutputRoot,
  findSingleDirectory,
  removePath,
  run,
  runBun,
  tauriBuildConfig
} from './common.mjs';

assertHostPlatform('darwin', 'arm64');
ensureOutputRoot();
const dmgPath = join(OUTPUT_ROOT, 'augur-git-tauri-macos-arm64.dmg');
removePath(dmgPath);
removePath(join(BUNDLE_ROOT, 'macos'));
const environment = {
  ...process.env,
  APPLE_SIGNING_IDENTITY: process.env.APPLE_SIGNING_IDENTITY || '-'
};
run('bun', appendTauriConfig(['run', 'tauri:build', '--', '--bundles', 'app', '--ci'], tauriBuildConfig()), {
  env: environment
});

const appBundle = findSingleDirectory(join(BUNDLE_ROOT, 'macos'), '.app');
run('codesign', ['--verify', '--deep', '--strict', appBundle]);
assertCli(join(appBundle, 'Contents', 'MacOS', 'agit'));

const stagingRoot = mkdtempSync(join(tmpdir(), 'augur-git-tauri-dmg-'));
try {
  cpSync(appBundle, join(stagingRoot, 'Augur Git Tauri.app'), {
    recursive: true
  });
  symlinkSync('/Applications', join(stagingRoot, 'Applications'));
  run('hdiutil', [
    'create',
    '-volname',
    'Augur Git Tauri',
    '-srcfolder',
    stagingRoot,
    '-format',
    'UDZO',
    '-ov',
    dmgPath
  ]);
  run('hdiutil', ['verify', dmgPath]);
} finally {
  removePath(stagingRoot);
}

console.log('Created ' + dmgPath);
