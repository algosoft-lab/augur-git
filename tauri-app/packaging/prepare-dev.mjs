import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { TARGET_ROOT, buildCli } from './common.mjs';

// The Windows bundle resources include the release CLI, and Tauri validates
// them during dev builds too. Build it once when missing; packaging always
// rebuilds it through prepare-cli.mjs.
if (process.platform === 'win32' && !existsSync(join(TARGET_ROOT, 'agit.exe'))) {
  buildCli();
}
