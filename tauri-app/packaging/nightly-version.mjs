import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

export function nextNightlyVersion(baseVersion, runNumber) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/u.exec(baseVersion);
  if (!match) throw new Error(`Expected a stable three-part application version: ${baseVersion}`);
  if (!/^[1-9]\d*$/u.test(String(runNumber))) {
    throw new Error(`Expected a positive workflow run number: ${runNumber}`);
  }
  return `${match[1]}.${match[2]}.${Number(match[3]) + 1}-nightly.${runNumber}`;
}

async function main() {
  const runNumber = process.argv[2] ?? process.env.GITHUB_RUN_NUMBER;
  if (!runNumber) throw new Error('Pass a workflow run number or set GITHUB_RUN_NUMBER');
  const configPath = new URL('../src-tauri/tauri.conf.json', import.meta.url);
  const config = JSON.parse(await readFile(configPath, 'utf8'));
  process.stdout.write(`${nextNightlyVersion(config.version, runNumber)}\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
