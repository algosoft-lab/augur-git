import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const repository = 'algosoft-lab/augur-git';
const nightlyVersion = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)-nightly\.([1-9]\d*)$/u;

export function renderUpdaterManifest({ version, url, signature, pubDate }) {
  if (!nightlyVersion.test(version)) throw new Error(`Invalid nightly version: ${version}`);
  const parsedUrl = new URL(url);
  if (
    parsedUrl.protocol !== 'https:' ||
    parsedUrl.hostname !== 'github.com' ||
    parsedUrl.pathname !==
      `/${repository}/releases/download/tauri-nightly-${version}/augur-git-tauri-windows-x86_64-nsis-${version}.exe`
  ) {
    throw new Error(`Update URL must point at the matching immutable Windows installer: ${url}`);
  }
  if (!signature.trim()) throw new Error('Updater signature is required');
  return JSON.stringify(
    {
      version,
      notes: `Augur Git nightly ${version}`,
      pub_date: pubDate ?? new Date().toISOString(),
      platforms: {
        'windows-x86_64': {
          url,
          signature: signature.trim()
        }
      }
    },
    null,
    2
  );
}

async function main() {
  const { values } = parseArgs({
    options: {
      version: { type: 'string' },
      url: { type: 'string' },
      'signature-file': { type: 'string' },
      out: { type: 'string' }
    }
  });
  if (!values.version || !values.url || !values['signature-file'] || !values.out) {
    throw new Error('Pass --version, --url, --signature-file, and --out');
  }
  const signature = await readFile(values['signature-file'], 'utf8');
  await writeFile(
    values.out,
    `${renderUpdaterManifest({ version: values.version, url: values.url, signature })}\n`,
    'utf8'
  );
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
