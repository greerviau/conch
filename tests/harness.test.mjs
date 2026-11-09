import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
test('desktop uses a local UI and microphone usage description', () => {
 const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json'));
 assert.equal(config.identifier, 'app.conch.desktop');
 assert.match(readFileSync('src-tauri/Info.plist','utf8'), /NSMicrophoneUsageDescription/);
});
