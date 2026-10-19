// node --test scripts/release-version.test.mjs
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { test } from 'node:test';
import { nextPatch, releaseVersion } from './release-version.mjs';

test('a version without a release yet is released as is', () => {
  assert.deepEqual(releaseVersion({ cargoVersion: '1.6.7', releaseExists: false, runNumber: 57 }), {
    mode: 'release',
    version: '1.6.7',
  });
});

test('after the release, builds are betas of the next patch', () => {
  assert.deepEqual(releaseVersion({ cargoVersion: '1.6.6\n', releaseExists: true, runNumber: 57 }), {
    mode: 'beta',
    version: '1.6.7-beta.57',
  });
  assert.equal(nextPatch('1.9.9'), '1.9.10');
});

test('rejects versions and run numbers it cannot order', () => {
  assert.throws(() => releaseVersion({ cargoVersion: '1.6', releaseExists: true, runNumber: 1 }));
  assert.throws(() => releaseVersion({ cargoVersion: '1.6.7-beta.1', releaseExists: false, runNumber: 1 }));
  assert.throws(() => releaseVersion({ cargoVersion: '1.6.6', releaseExists: true, runNumber: 'x' }));
});

test('the CLI prints GITHUB_OUTPUT lines', () => {
  const out = execFileSync('node', ['scripts/release-version.mjs', '1.6.6', 'true', '12'], { encoding: 'utf8' });
  assert.equal(out, 'mode=beta\nversion=1.6.7-beta.12\n');
});
