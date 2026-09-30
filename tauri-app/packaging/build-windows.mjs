import { join } from 'node:path';
import {
  BUNDLE_ROOT,
  OUTPUT_ROOT,
  assertHostPlatform,
  appendTauriConfig,
  copySingleFile,
  ensureOutputRoot,
  removePath,
  runBun,
  tauriBuildConfig
} from './common.mjs';

assertHostPlatform('win32', 'x64');
ensureOutputRoot();
removePath(join(OUTPUT_ROOT, 'augur-git-tauri-windows-x86_64-nsis.exe'));
removePath(join(OUTPUT_ROOT, 'augur-git-tauri-windows-x86_64-nsis.exe.sig'));
removePath(join(BUNDLE_ROOT, 'nsis'));
if (process.env.AUGUR_RELEASE_VERSION && !process.env.TAURI_SIGNING_PRIVATE_KEY) {
  throw new Error('TAURI_SIGNING_PRIVATE_KEY is required for nightly updater builds');
}
const config = tauriBuildConfig(
  process.env.TAURI_SIGNING_PRIVATE_KEY ? { bundle: { createUpdaterArtifacts: true } } : {}
);
runBun(
  ...appendTauriConfig(['run', 'tauri:build', '--', '--bundles', 'nsis', '--ci'], config)
);

const installer = copySingleFile(
  join(BUNDLE_ROOT, 'nsis'),
  '.exe',
  'augur-git-tauri-windows-x86_64-nsis.exe'
);
console.log('Created ' + installer);
if (process.env.TAURI_SIGNING_PRIVATE_KEY) {
  const signature = copySingleFile(
    join(BUNDLE_ROOT, 'nsis'),
    '.exe.sig',
    'augur-git-tauri-windows-x86_64-nsis.exe.sig'
  );
  console.log('Created ' + signature);
}
