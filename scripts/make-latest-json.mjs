#!/usr/bin/env node
// Writes the Tauri updater manifest (`latest.json`) for a release from the assets collected by
// scripts/collect-bundles.sh.
//
//   node scripts/make-latest-json.mjs <assets dir> <version> <download base URL> [notes file] > latest.json
//
// The base URL is the release's download folder, e.g.
// https://github.com/LovaszCC/RemoteDesktop/releases/download/v1.2.3. Every updater bundle needs
// its `.sig` (minisign, made by `tauri build` with TAURI_SIGNING_PRIVATE_KEY); a missing one fails.
//
// Platform keys: `{os}-{arch}` (the fallback) and `{os}-{arch}-{installer}`, which the updater tries
// first, so a .deb install updates from the .deb and an AppImage from the AppImage.

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

/** Asset name suffix → the platform keys it serves. */
export const PLATFORMS = [
  { suffix: '_macos_aarch64.app.tar.gz', keys: ['darwin-aarch64', 'darwin-aarch64-app'] },
  { suffix: '_macos_x86_64.app.tar.gz', keys: ['darwin-x86_64', 'darwin-x86_64-app'] },
  { suffix: '_windows_x64-setup.exe', keys: ['windows-x86_64', 'windows-x86_64-nsis'] },
  { suffix: '_linux_amd64.AppImage', keys: ['linux-x86_64', 'linux-x86_64-appimage'] },
  { suffix: '_linux_amd64.deb', keys: ['linux-x86_64-deb'] },
  { suffix: '_linux_x86_64.rpm', keys: ['linux-x86_64-rpm'] },
];

/** The manifest object; throws when a bundle lacks its signature or nothing matches. */
export function makeManifest({ files, readSignature, version, baseUrl, notes = '', pubDate = new Date() }) {
  const platforms = {};
  for (const { suffix, keys } of PLATFORMS) {
    const asset = files.find((f) => f.endsWith(suffix));
    if (!asset) continue;
    if (!files.includes(`${asset}.sig`)) throw new Error(`${asset} has no .sig; was the signing key set?`);
    const entry = {
      signature: readSignature(`${asset}.sig`).trim(),
      url: `${baseUrl.replace(/\/$/, '')}/${encodeURIComponent(asset)}`,
    };
    for (const key of keys) platforms[key] = entry;
  }
  if (Object.keys(platforms).length === 0) throw new Error('no updater bundles found');
  return { version, notes, pub_date: pubDate.toISOString().replace(/\.\d{3}Z$/, 'Z'), platforms };
}

function main([dir, version, baseUrl, notesFile]) {
  if (!dir || !version || !baseUrl) {
    console.error('usage: make-latest-json.mjs <assets dir> <version> <download base URL> [notes file]');
    process.exit(2);
  }
  const manifest = makeManifest({
    files: readdirSync(dir),
    readSignature: (name) => readFileSync(join(dir, name), 'utf8'),
    version,
    baseUrl,
    notes: notesFile && existsSync(notesFile) ? readFileSync(notesFile, 'utf8').trim() : '',
  });
  process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
}

if (import.meta.url === `file://${process.argv[1]}`) main(process.argv.slice(2));
