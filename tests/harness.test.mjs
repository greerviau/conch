import { test } from 'node:test';
import { platform } from 'node:os';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { execFileSync, spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
test('desktop uses a local UI and microphone usage description', () => {
 const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json'));
 assert.equal(config.identifier, 'app.conch.desktop');
 assert.match(readFileSync('src-tauri/Info.plist','utf8'), /NSMicrophoneUsageDescription/);
});
test('releases publish updater artifacts that installed apps verify against the committed public key', () => {
 const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json'));
 assert.equal(config.bundle.createUpdaterArtifacts, true);
 assert.deepEqual(config.plugins.updater.endpoints, [
  'https://github.com/greerviau/conch/releases/latest/download/latest.json',
 ]);
 const key = Buffer.from(config.plugins.updater.pubkey, 'base64').toString();
 assert.match(key, /^untrusted comment: minisign public key/);
});
test('the release version check accepts a tag that matches every manifest and rejects one that does not', () => {
 const root = mkdtempSync(join(tmpdir(), 'conch-version-'));
 mkdirSync(join(root, 'src-tauri'));
 writeFileSync(join(root, 'package.json'), JSON.stringify({ version: '0.3.0' }));
 writeFileSync(join(root, 'src-tauri', 'Cargo.toml'), '[package]\nname = "conch"\nversion = "0.3.0"\n\n[dependencies]\nserde = { version = "1" }\n');
 writeFileSync(join(root, 'src-tauri', 'tauri.conf.json'), JSON.stringify({ version: '0.3.0' }));
 const run = (tag) => spawnSync('node', ['scripts/check-version.mjs', tag, root], { encoding: 'utf8' });
 assert.equal(run('v0.3.0').status, 0);
 const wrong = run('v0.3.1');
 assert.notEqual(wrong.status, 0);
 assert.match(wrong.stderr, /v0\.3\.1/);
 writeFileSync(join(root, 'src-tauri', 'tauri.conf.json'), JSON.stringify({ version: '0.2.9' }));
 const drift = run('v0.3.0');
 assert.notEqual(drift.status, 0);
 assert.match(drift.stderr, /tauri\.conf\.json/);
 assert.notEqual(run('0.3.0').status, 0);
});
const fixtureApp = (identity) => {
 const app = join(mkdtempSync(join(tmpdir(), 'conch-signing-')), 'Conch.app');
 mkdirSync(join(app, 'Contents', 'MacOS'), { recursive: true });
 writeFileSync(join(app, 'Contents', 'Info.plist'), `<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>app.conch.desktop</string><key>CFBundleExecutable</key><string>conch</string></dict></plist>`);
 execFileSync('cp', ['/usr/bin/true', join(app, 'Contents', 'MacOS', 'conch')]);
 execFileSync('codesign', ['--force', '--sign', identity, app], { stdio: 'ignore' });
 return app;
};
test('the signing check rejects an ad-hoc signed app because its Accessibility grant would not survive an update', { skip: platform() !== 'darwin' }, () => {
 const check = spawnSync('scripts/verify-signing.sh', [fixtureApp('-'), '0123456789ABCDEF0123456789ABCDEF01234567'], { encoding: 'utf8' });
 assert.notEqual(check.status, 0);
 assert.match(check.stderr, /Accessibility grant/);
});
test('the signing check accepts an app signed with the stable local identity', { skip: platform() !== 'darwin' }, (t) => {
 const found = spawnSync('scripts/setup-local-signing.sh', { encoding: 'utf8' });
 if (found.status !== 0) return t.skip('no local signing identity');
 const identity = found.stdout.trim();
 const check = spawnSync('scripts/verify-signing.sh', [fixtureApp(identity), identity], { encoding: 'utf8' });
 assert.equal(check.status, 0, check.stderr);
 assert.match(check.stdout, /stable requirement/);
});
test('the repository is MIT licensed under the copyright holder named in every manifest and the license file', () => {
 const license = readFileSync('LICENSE', 'utf8');
 assert.match(license, /^MIT License\n\nCopyright \(c\) 2026 Greer\n/);
 assert.match(license, /Permission is hereby granted, free of charge/);
 assert.equal(JSON.parse(readFileSync('package.json')).license, 'MIT');
 assert.match(readFileSync('src-tauri/Cargo.toml', 'utf8').split(/^\[dependencies\]/m)[0], /^license = "MIT"$/m);
});
