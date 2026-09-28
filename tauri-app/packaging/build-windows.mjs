import { join } from 'node:path';
import {
  BUNDLE_ROOT,
  OUTPUT_ROOT,
  assertHostPlatform,
  assertRealSidecar,
  copySingleFile,
  ensureOutputRoot,
  removePath,
  runBun
} from './common.mjs';

assertHostPlatform('win32', 'x64');
ensureOutputRoot();
removePath(join(OUTPUT_ROOT, 'augur-git-tauri-windows-x86_64-nsis.exe'));
removePath(join(BUNDLE_ROOT, 'nsis'));
runBun('run', 'tauri:build', '--', '--bundles', 'nsis', '--ci');
assertRealSidecar();

const installer = copySingleFile(
  join(BUNDLE_ROOT, 'nsis'),
  '.exe',
  'augur-git-tauri-windows-x86_64-nsis.exe'
);
console.log('Created ' + installer);
