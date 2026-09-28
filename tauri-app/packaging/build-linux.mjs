import { cpSync, mkdirSync, mkdtempSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  BUNDLE_ROOT,
  OUTPUT_ROOT,
  TARGET_ROOT,
  assertExecutable,
  assertHostPlatform,
  assertNonEmptyFile,
  copySingleFile,
  ensureOutputRoot,
  removePath,
  run,
  runBun
} from './common.mjs';

assertHostPlatform('linux', 'x64');
const appBinary = join(TARGET_ROOT, 'augur-git-tauri');
const rawArchive = join(OUTPUT_ROOT, 'augur-git-tauri-linux-x86_64.tar.gz');
const appImageOutput = join(OUTPUT_ROOT, 'augur-git-tauri-linux-x86_64.AppImage');
const debOutput = join(OUTPUT_ROOT, 'augur-git-tauri-linux-x86_64.deb');
const appImageDirectory = join(BUNDLE_ROOT, 'appimage');
const debDirectory = join(BUNDLE_ROOT, 'deb');

ensureOutputRoot();
removePath(appImageDirectory);
removePath(debDirectory);
removePath(rawArchive);
removePath(appImageOutput);
removePath(debOutput);

runBun('run', 'tauri:build', '--', '--no-bundle', '--ci');
assertExecutable(appBinary);

const stagingRoot = mkdtempSync(join(tmpdir(), 'augur-git-tauri-raw-'));
const archiveDirectory = join(stagingRoot, 'augur-git-tauri-linux-x86_64');
try {
  mkdirSync(archiveDirectory, { recursive: true });
  cpSync(appBinary, join(archiveDirectory, 'augur-git-tauri'));
  writeFileSync(
    join(archiveDirectory, 'README.txt'),
    [
      'Augur Git Tauri Linux x86-64 raw binaries',
      '',
      'Run ./augur-git-tauri to launch the desktop application.',
      '',
      'The desktop application uses system GTK 3 and WebKitGTK 4.1 libraries.',
      'Install the runtime libraries required by your Linux distribution.',
      ''
    ].join('\n'),
    'utf8'
  );
  run('tar', ['-czf', rawArchive, '-C', stagingRoot, 'augur-git-tauri-linux-x86_64']);
} finally {
  removePath(stagingRoot);
}
assertNonEmptyFile(rawArchive);
const archiveCheckRoot = mkdtempSync(join(tmpdir(), 'augur-git-tauri-archive-check-'));
try {
  run('tar', ['-xzf', rawArchive, '-C', archiveCheckRoot]);
  const rawPackage = join(archiveCheckRoot, 'augur-git-tauri-linux-x86_64');
  const rawApp = join(rawPackage, 'augur-git-tauri');
  assertExecutable(rawApp);
} finally {
  removePath(archiveCheckRoot);
}

run('bun', ['run', 'tauri', 'bundle', '--bundles', 'deb,appimage', '--ci'], {
  env: {
    ...process.env,
    APPIMAGE_EXTRACT_AND_RUN: '1'
  }
});
const appImage = copySingleFile(
  appImageDirectory,
  '.AppImage',
  'augur-git-tauri-linux-x86_64.AppImage'
);
const deb = copySingleFile(debDirectory, '.deb', 'augur-git-tauri-linux-x86_64.deb');
assertNonEmptyFile(rawArchive);

const packageCheckRoot = mkdtempSync(join(tmpdir(), 'augur-git-tauri-package-check-'));
try {
  const debRoot = join(packageCheckRoot, 'deb');
  const debArchiveRoot = join(packageCheckRoot, 'deb-archive');
  const appImageWorkingRoot = join(packageCheckRoot, 'appimage');
  const appImageRoot = join(appImageWorkingRoot, 'squashfs-root');
  mkdirSync(debRoot, { recursive: true });
  mkdirSync(debArchiveRoot, { recursive: true });
  mkdirSync(appImageWorkingRoot, { recursive: true });
  run('ar', ['x', deb], { cwd: debArchiveRoot });
  const dataArchiveName = readdirSync(debArchiveRoot).find((name) => name.startsWith('data.tar'));
  if (!dataArchiveName) {
    throw new Error('The Debian package does not contain a data archive');
  }
  run('tar', ['-xf', join(debArchiveRoot, dataArchiveName), '-C', debRoot]);
  run(appImage, ['--appimage-extract'], {
    cwd: appImageWorkingRoot,
    env: {
      ...process.env,
      APPIMAGE_EXTRACT_AND_RUN: '1'
    }
  });
  assertExecutable(join(debRoot, 'usr', 'bin', 'augur-git-tauri'));
  assertExecutable(join(appImageRoot, 'usr', 'bin', 'augur-git-tauri'));
} finally {
  removePath(packageCheckRoot);
}

console.log('Created Linux packages in ' + OUTPUT_ROOT);
