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

export function assertRealSidecar(target = rustHostTarget()) {
  const executableSuffix = target.includes('windows') ? '.exe' : '';
  const path = join(TAURI_ROOT, 'binaries', 'augurgit-tauri-' + target + executableSuffix);

  try {
    if (statSync(path).isFile() && statSync(path).size > 4096) {
      return path;
    }
  } catch {
    // The sidecar is missing; report the expected target-specific path below.
  }
  throw new Error('The real Tauri CLI sidecar was not built at ' + path);
}

export function assertPackagedSidecar(root) {
  const pending = [root];
  while (pending.length > 0) {
    const directory = pending.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) {
        pending.push(path);
      } else if (
        entry.isFile() &&
        entry.name.startsWith('augurgit-tauri') &&
        statSync(path).size > 4096
      ) {
        return path;
      }
    }
  }
  throw new Error('No real augurgit-tauri CLI sidecar was found under ' + root);
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
