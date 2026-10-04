import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readdirSync, rmSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const APP_ROOT = fileURLToPath(new URL('../', import.meta.url));
export const TAURI_ROOT = join(APP_ROOT, 'src-tauri');
export const TARGET_ROOT = join(TAURI_ROOT, 'target', 'release');
export const BUNDLE_ROOT = join(TARGET_ROOT, 'bundle');
export const OUTPUT_ROOT = join(APP_ROOT, 'packaging', 'out');

export function run(command, args, options = {}) {
  console.log('$ ' + [command, ...args].join(' '));
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? APP_ROOT,
    env: options.env ?? process.env,
    stdio: 'inherit'
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(command + ' exited with status ' + result.status);
  }
}

export function runBun(...args) {
  run('bun', args);
}

export function tauriBuildConfig(extra = {}) {
  const version = process.env.AUGUR_RELEASE_VERSION;
  return version ? { ...extra, version } : extra;
}

export function appendTauriConfig(args, config) {
  if (Object.keys(config).length === 0) return args;
  return [...args, '--config', JSON.stringify(config)];
}

export function ensureOutputRoot() {
  mkdirSync(OUTPUT_ROOT, { recursive: true });
}

export function removePath(path) {
  rmSync(path, { recursive: true, force: true });
}

export function copySingleFile(sourceDirectory, extension, destinationName) {
  const matches = readdirSync(sourceDirectory, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(extension))
    .map((entry) => join(sourceDirectory, entry.name));

  if (matches.length !== 1) {
    throw new Error(
      'Expected exactly one ' +
        extension +
        ' file in ' +
        sourceDirectory +
        ', found ' +
        matches.length
    );
  }

  ensureOutputRoot();
  const destination = join(OUTPUT_ROOT, destinationName);
  removePath(destination);
  copyFileSync(matches[0], destination);
  assertNonEmptyFile(destination);
  return destination;
}

export function findSingleDirectory(sourceDirectory, suffix) {
  const matches = readdirSync(sourceDirectory, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && entry.name.endsWith(suffix))
    .map((entry) => join(sourceDirectory, entry.name));

  if (matches.length !== 1) {
    throw new Error(
      'Expected exactly one ' +
        suffix +
        ' directory in ' +
        sourceDirectory +
        ', found ' +
        matches.length
    );
  }
  return matches[0];
}

export function assertNonEmptyFile(path) {
  if (!statSync(path).isFile() || statSync(path).size === 0) {
    throw new Error('Expected a non-empty file at ' + path);
  }
}

export function assertExecutable(path) {
  assertNonEmptyFile(path);
  if ((statSync(path).mode & 0o111) === 0) {
    throw new Error('Expected an executable file at ' + path);
  }
}

export function rustHostTarget() {
  const result = spawnSync('rustc', ['-vV'], {
    cwd: APP_ROOT,
    encoding: 'utf8'
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error('rustc -vV failed');
  }

  const host = result.stdout.split(/\r?\n/).find((line) => line.startsWith('host: '));
  if (!host) {
    throw new Error('Could not determine the Rust host target');
  }
  return host.slice('host: '.length).trim();
}

export function assertHostPlatform(expectedPlatform, expectedArch) {
  if (process.platform !== expectedPlatform || process.arch !== expectedArch) {
    throw new Error(
      'This packaging script requires ' +
        expectedPlatform +
        '/' +
        expectedArch +
        '; found ' +
        process.platform +
        '/' +
        process.arch
    );
  }
}

export function buildCli() {
  run('cargo', ['build', '--manifest-path', join(TAURI_ROOT, 'Cargo.toml'), '-p', 'augur-core', '--bin', 'agit', '--release']);
  assertCli(join(TARGET_ROOT, 'agit'));
}

export function assertCli(path) {
  assertExecutable(path);
  for (const flag of ['--help', '--version']) {
    const result = spawnSync(path, [flag], { encoding: 'utf8', timeout: 10000 });
    if (result.error || result.status !== 0 || !result.stdout.includes('Augur Git')) {
      throw new Error(`CLI smoke check failed for ${path} ${flag}: ${result.error ?? result.stderr}`);
    }
    if (flag === '--help' && !result.stdout.includes('agit [OPTIONS]')) {
      throw new Error('CLI help does not expose the agit command');
    }
  }
  const invalid = spawnSync(path, ['--not-an-option'], { encoding: 'utf8', timeout: 10000 });
  if (invalid.status !== 2 || !invalid.stderr.includes('agit:')) {
    throw new Error('CLI usage errors must be printed to stderr with exit status 2');
  }
}
