// node --test scripts/make-latest-json.test.mjs
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { makeManifest } from './make-latest-json.mjs';

const BASE = 'https://github.com/LovaszCC/RemoteDesktop/releases/download/v1.2.3';
const ALL = [
  'RemoteDesktop_1.2.3_macos_aarch64.dmg',
  'RemoteDesktop_1.2.3_macos_aarch64.app.tar.gz',
  'RemoteDesktop_1.2.3_macos_aarch64.app.tar.gz.sig',
  'RemoteDesktop_1.2.3_macos_x86_64.dmg',
  'RemoteDesktop_1.2.3_macos_x86_64.app.tar.gz',
  'RemoteDesktop_1.2.3_macos_x86_64.app.tar.gz.sig',
  'RemoteDesktop_1.2.3_windows_x64-setup.exe',
  'RemoteDesktop_1.2.3_windows_x64-setup.exe.sig',
  'RemoteDesktop_1.2.3_linux_amd64.AppImage',
  'RemoteDesktop_1.2.3_linux_amd64.AppImage.sig',
  'RemoteDesktop_1.2.3_linux_amd64.deb',
  'RemoteDesktop_1.2.3_linux_amd64.deb.sig',
  'RemoteDesktop_1.2.3_linux_x86_64.rpm',
  'RemoteDesktop_1.2.3_linux_x86_64.rpm.sig',
];

const manifest = (files) =>
  makeManifest({
    files,
    readSignature: (name) => `sig of ${name}\n`,
    version: '1.2.3',
    baseUrl: `${BASE}/`,
    notes: 'Notes',
    pubDate: new Date('2026-10-17T10:00:00.123Z'),
  });

test('lists every platform with its signature and download URL', () => {
  const m = manifest(ALL);
  assert.equal(m.version, '1.2.3');
  assert.equal(m.notes, 'Notes');
  assert.equal(m.pub_date, '2026-10-17T10:00:00Z');
  assert.deepEqual(Object.keys(m.platforms).sort(), [
    'darwin-aarch64',
    'darwin-aarch64-app',
    'darwin-x86_64',
    'darwin-x86_64-app',
    'linux-x86_64',
    'linux-x86_64-appimage',
    'linux-x86_64-deb',
    'linux-x86_64-rpm',
    'windows-x86_64',
    'windows-x86_64-nsis',
  ]);
  assert.deepEqual(m.platforms['windows-x86_64'], {
    signature: 'sig of RemoteDesktop_1.2.3_windows_x64-setup.exe.sig',
    url: `${BASE}/RemoteDesktop_1.2.3_windows_x64-setup.exe`,
  });
  assert.equal(m.platforms['linux-x86_64'].url, `${BASE}/RemoteDesktop_1.2.3_linux_amd64.AppImage`);
  assert.equal(m.platforms['darwin-x86_64-app'].url, `${BASE}/RemoteDesktop_1.2.3_macos_x86_64.app.tar.gz`);
});

test('a bundle without its signature fails', () => {
  const files = ALL.filter((f) => f !== 'RemoteDesktop_1.2.3_linux_amd64.deb.sig');
  assert.throws(() => manifest(files), /deb has no \.sig/);
});

test('no updater bundles fails', () => {
  assert.throws(() => manifest(['RemoteDesktop_1.2.3_macos_aarch64.dmg']), /no updater bundles/);
});
