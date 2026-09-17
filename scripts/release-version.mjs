import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const read = path => readFileSync(resolve(root, path), 'utf8');
const manifest = JSON.parse(read('package.json'));
const npmLock = JSON.parse(read('package-lock.json'));
const config = JSON.parse(read('src-tauri/tauri.conf.json'));
const cargo = read('src-tauri/Cargo.toml');
const cargoLock = read('src-tauri/Cargo.lock');
const cargoVersion = /(\[package\][\s\S]*?\nversion = ")([^"]+)(")/;
const cargoLockVersion = /(\[\[package\]\]\r?\nname = "hq-git"\r?\nversion = ")([^"]+)(")/;
const versions = [manifest.version, npmLock.version, npmLock.packages[''].version, config.version,
  cargo.match(cargoVersion)?.[2], cargoLock.match(cargoLockVersion)?.[2]];
const [argument, tag] = process.argv.slice(2);
const stableVersion = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

if (argument === '--check') {
  if (!stableVersion.test(manifest.version) || versions.some(value => value !== manifest.version))
    throw new Error('Version mismatch. Run npm run release:version -- X.Y.Z first.');
  if (tag && tag !== `v${manifest.version}`) throw new Error(`Tag must be v${manifest.version}.`);
  if (!config.plugins?.updater?.pubkey || !config.bundle.createUpdaterArtifacts)
    throw new Error('Updater signing configuration is missing.');
  process.stdout.write(`Release configuration OK: v${manifest.version}\n`);
} else {
  if (!argument || !stableVersion.test(argument) || tag)
    throw new Error('Usage: npm run release:version -- X.Y.Z (stable releases only), or --check [vX.Y.Z]');
  if (versions.some(value => value === undefined)) throw new Error('Cannot locate all version fields.');
  const compare = (a, b) => {
    const left = a.split('.').map(Number), right = b.split('.').map(Number);
    for (let i = 0; i < left.length; i++) if (left[i] !== right[i]) return left[i] - right[i];
    return 0;
  };
  if (compare(argument, manifest.version) <= 0) throw new Error('New version must be greater than the current version.');
  manifest.version = npmLock.version = npmLock.packages[''].version = config.version = argument;
  for (const [path, data] of [['package.json', manifest], ['package-lock.json', npmLock], ['src-tauri/tauri.conf.json', config]])
    writeFileSync(resolve(root, path), JSON.stringify(data, null, 2) + '\n');
  writeFileSync(resolve(root, 'src-tauri/Cargo.toml'), cargo.replace(cargoVersion, (_, before, _old, after) => before + argument + after));
  writeFileSync(resolve(root, 'src-tauri/Cargo.lock'), cargoLock.replace(cargoLockVersion, (_, before, _old, after) => before + argument + after));
  process.stdout.write(`Version set to ${argument}. Review and commit all five version files before tagging.\n`);
}
