import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { platform, tmpdir } from 'node:os';
import { join } from 'node:path';

const scratch = () => mkdtempSync(join(tmpdir(), 'conch-install-'));
const install = (env) =>
 spawnSync('sh', ['scripts/install.sh'], { encoding: 'utf8', env: { PATH: process.env.PATH, HOME: scratch(), ...env } });

// A signed stand-in for Conch.app packed like the updater tarball.
const releaseWithApp = ({ tamper = false } = {}) => {
 const base = scratch();
 const app = join(scratch(), 'Conch.app');
 mkdirSync(join(app, 'Contents', 'MacOS'), { recursive: true });
 writeFileSync(join(app, 'Contents', 'Info.plist'), `<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>app.conch.desktop</string><key>CFBundleExecutable</key><string>conch</string></dict></plist>`);
 execFileSync('cp', ['/usr/bin/true', join(app, 'Contents', 'MacOS', 'conch')]);
 execFileSync('codesign', ['--force', '--sign', '-', app], { stdio: 'ignore' });
 if (tamper) writeFileSync(join(app, 'Contents', 'MacOS', 'conch'), 'not the signed binary');
 execFileSync('tar', ['-czf', join(base, 'Conch_aarch64.app.tar.gz'), '-C', join(app, '..'), 'Conch.app']);
 return `file://${base}`;
};

test('macOS install puts the signed app in the chosen folder without the quarantine flag', { skip: platform() !== 'darwin' }, () => {
 const dir = scratch();
 const out = install({ CONCH_OS: 'Darwin', CONCH_ARCH: 'arm64', CONCH_DOWNLOAD_BASE: releaseWithApp(), CONCH_INSTALL_DIR: dir });
 assert.equal(out.status, 0, out.stderr);
 assert.ok(existsSync(join(dir, 'Conch.app', 'Contents', 'MacOS', 'conch')));
 const attrs = spawnSync('xattr', [join(dir, 'Conch.app')], { encoding: 'utf8' }).stdout;
 assert.doesNotMatch(attrs, /com\.apple\.quarantine/);
 assert.match(out.stdout, /Installed/);
});

test('macOS install replaces an older copy', { skip: platform() !== 'darwin' }, () => {
 const dir = scratch();
 mkdirSync(join(dir, 'Conch.app', 'Contents'), { recursive: true });
 writeFileSync(join(dir, 'Conch.app', 'old-file'), 'old');
 const out = install({ CONCH_OS: 'Darwin', CONCH_ARCH: 'arm64', CONCH_DOWNLOAD_BASE: releaseWithApp(), CONCH_INSTALL_DIR: dir });
 assert.equal(out.status, 0, out.stderr);
 assert.ok(!existsSync(join(dir, 'Conch.app', 'old-file')));
 assert.ok(existsSync(join(dir, 'Conch.app', 'Contents', 'MacOS', 'conch')));
});

test('macOS install refuses an app whose signature no longer matches and leaves the old copy', { skip: platform() !== 'darwin' }, () => {
 const dir = scratch();
 mkdirSync(join(dir, 'Conch.app'), { recursive: true });
 writeFileSync(join(dir, 'Conch.app', 'old-file'), 'old');
 const out = install({ CONCH_OS: 'Darwin', CONCH_ARCH: 'arm64', CONCH_DOWNLOAD_BASE: releaseWithApp({ tamper: true }), CONCH_INSTALL_DIR: dir });
 assert.notEqual(out.status, 0);
 assert.match(out.stderr, /signature/i);
 assert.ok(existsSync(join(dir, 'Conch.app', 'old-file')));
});

test('macOS install refuses when the download is missing', { skip: platform() !== 'darwin' }, () => {
 const out = install({ CONCH_OS: 'Darwin', CONCH_ARCH: 'arm64', CONCH_DOWNLOAD_BASE: `file://${scratch()}`, CONCH_INSTALL_DIR: scratch() });
 assert.notEqual(out.status, 0);
 assert.match(out.stderr, /download/i);
});

test('an Intel Mac is told the macOS build is Apple Silicon only', () => {
 const out = install({ CONCH_OS: 'Darwin', CONCH_ARCH: 'x86_64' });
 assert.notEqual(out.status, 0);
 assert.match(out.stderr, /Apple Silicon/);
});

test('an unsupported system is pointed to the releases page', () => {
 const out = install({ CONCH_OS: 'FreeBSD', CONCH_ARCH: 'amd64' });
 assert.notEqual(out.status, 0);
 assert.match(out.stderr, /github\.com\/greerviau\/conch\/releases/);
});

test('Linux install downloads the latest AppImage and makes it executable', () => {
 const base = scratch();
 writeFileSync(join(base, 'Conch_9.9.9_amd64.AppImage'), '#!/bin/sh\necho conch\n');
 writeFileSync(join(base, 'Conch_9.9.9_amd64.deb'), 'deb');
 const api = join(base, 'latest.json');
 writeFileSync(api, JSON.stringify({ assets: [
  { name: 'Conch_9.9.9_amd64.deb', browser_download_url: `file://${join(base, 'Conch_9.9.9_amd64.deb')}` },
  { name: 'Conch_9.9.9_amd64.AppImage', browser_download_url: `file://${join(base, 'Conch_9.9.9_amd64.AppImage')}` },
 ] }, null, 2));
 const bin = scratch();
 const out = install({ CONCH_OS: 'Linux', CONCH_ARCH: 'x86_64', CONCH_API_URL: `file://${api}`, CONCH_BIN_DIR: bin });
 assert.equal(out.status, 0, out.stderr);
 const target = join(bin, 'Conch.AppImage');
 assert.match(readFileSync(target, 'utf8'), /echo conch/);
 assert.ok(statSync(target).mode & 0o100);
 assert.match(out.stdout, new RegExp(target.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
});

test('Linux install reports a release with no AppImage', () => {
 const base = scratch();
 const api = join(base, 'latest.json');
 writeFileSync(api, JSON.stringify({ assets: [{ name: 'Conch_9.9.9_amd64.deb', browser_download_url: 'file:///x.deb' }] }, null, 2));
 const out = install({ CONCH_OS: 'Linux', CONCH_ARCH: 'x86_64', CONCH_API_URL: `file://${api}`, CONCH_BIN_DIR: scratch() });
 assert.notEqual(out.status, 0);
 assert.match(out.stderr, /AppImage/);
});
