import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const repository = 'algosoft-lab/augur-git';
const nightlyVersion = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)-nightly\.([1-9]\d*)$/u;

export function renderCask({ version, sha256 }) {
  if (!nightlyVersion.test(version)) throw new Error(`Invalid nightly version: ${version}`);
  if (!/^[a-f0-9]{64}$/u.test(sha256)) throw new Error('Cask checksum must be a lowercase SHA-256 digest');
  const dmgName = `augur-git-tauri-macos-arm64-${version}.dmg`;
  const releaseTag = `tauri-nightly-${version}`;
  return `cask "augur-git" do
  version "${version}"
  sha256 "${sha256}"

  url "https://github.com/${repository}/releases/download/${releaseTag}/${dmgName}"
  name "Augur Git"
  desc "Review-first desktop Git client"
  homepage "https://github.com/${repository}"

  depends_on arch: :arm64

  app "Augur Git Tauri.app"
  binary "#{appdir}/Augur Git Tauri.app/Contents/MacOS/agit", target: "agit"

  caveats <<~EOS
    Nightly builds use an ad-hoc signature and are not notarized. macOS may ask you to approve the app
    in System Settings -> Privacy & Security after installation or upgrade.
  EOS
end
`;
}

export async function renderCaskFromDmg(dmgPath, version) {
  const expected = `augur-git-tauri-macos-arm64-${version}.dmg`;
  if (basename(dmgPath) !== expected) throw new Error(`Expected ${expected}, found ${basename(dmgPath)}`);
  const sha256 = createHash('sha256').update(await readFile(dmgPath)).digest('hex');
  return renderCask({ version, sha256 });
}

async function main() {
  const { values } = parseArgs({
    options: {
      dmg: { type: 'string' },
      version: { type: 'string' },
      out: { type: 'string' }
    }
  });
  if (!values.dmg || !values.version || !values.out) {
    throw new Error('Pass --dmg, --version, and --out');
  }
  await writeFile(values.out, await renderCaskFromDmg(values.dmg, values.version), 'utf8');
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
