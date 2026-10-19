#!/usr/bin/env node
// Decides what a build on `beta` is (scripts for .github/workflows/release.yml):
//
//   node scripts/release-version.mjs <Cargo.toml version> <release exists: true|false> <run number>
//   → mode=release|beta
//     version=<the version to build>
//
// The Cargo version is the last release, or the next one once a release PR has bumped it:
//   - no GitHub release `v<version>` yet (draft or published) → a release build of exactly that version;
//   - otherwise → a beta of the next patch: 1.6.6 → 1.6.7-beta.<run>, which sorts after every earlier beta
//     and before 1.6.7 itself.

/** `1.6.6` → `1.6.7`; throws on anything but `MAJOR.MINOR.PATCH`. */
export function nextPatch(version) {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version.trim());
  if (!match) throw new Error(`not a MAJOR.MINOR.PATCH version: ${version}`);
  return `${match[1]}.${match[2]}.${Number(match[3]) + 1}`;
}

/** The mode and version of a build of `cargoVersion` on beta. */
export function releaseVersion({ cargoVersion, releaseExists, runNumber }) {
  const version = cargoVersion.trim();
  nextPatch(version); // validates
  if (!releaseExists) return { mode: 'release', version };
  if (!/^\d+$/.test(String(runNumber))) throw new Error(`not a run number: ${runNumber}`);
  return { mode: 'beta', version: `${nextPatch(version)}-beta.${runNumber}` };
}

function main([cargoVersion, releaseExists, runNumber]) {
  if (!cargoVersion || !['true', 'false'].includes(releaseExists) || !runNumber) {
    console.error('usage: release-version.mjs <Cargo.toml version> <true|false> <run number>');
    process.exit(2);
  }
  const { mode, version } = releaseVersion({
    cargoVersion,
    releaseExists: releaseExists === 'true',
    runNumber,
  });
  process.stdout.write(`mode=${mode}\nversion=${version}\n`);
}

if (import.meta.url === `file://${process.argv[1]}`) main(process.argv.slice(2));
