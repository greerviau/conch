// Fails unless a release tag such as v0.3.0 matches the version in every manifest.
// Usage: node scripts/check-version.mjs <tag> [root]
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const [tag, root = '.'] = process.argv.slice(2);
if (!tag || !/^v\d+\.\d+\.\d+$/.test(tag)) {
  console.error(`Release tag "${tag ?? ''}" must look like v1.2.3`);
  process.exit(1);
}
const expected = tag.slice(1);
const cargo = readFileSync(join(root, 'src-tauri/Cargo.toml'), 'utf8');
const found = {
  'package.json': JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version,
  'src-tauri/Cargo.toml': cargo.match(/^\[package\][^[]*?^version\s*=\s*"([^"]+)"/ms)?.[1],
  'src-tauri/tauri.conf.json': JSON.parse(
    readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8'),
  ).version,
};
const wrong = Object.entries(found).filter(([, version]) => version !== expected);
for (const [file, version] of wrong) {
  console.error(`${file} has version ${version ?? 'none'}; tag ${tag} needs ${expected}`);
}
process.exit(wrong.length ? 1 : 0);
