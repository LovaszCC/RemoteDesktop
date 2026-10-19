import { afterEach, describe, expect, it } from 'vitest';
import { commandTitle, errorMessage, noDatabaseMessage } from './errors';
import { setLocale } from './i18n/index.svelte';

afterEach(() => setLocale('en'));

describe('errors', () => {
  it("prefers Rust's message when it is a catalog key", () => {
    const wrongCurrent = {
      code: 'wrongPassword',
      message: 'The current master password is wrong.',
      detail: null,
    };
    expect(errorMessage(wrongCurrent)).toBe('The current master password is wrong.');
    setLocale('hu');
    expect(errorMessage({ code: 'wrongPassword', message: 'Wrong password', detail: null })).not.toBe(
      'Wrong password',
    );
  });

  it('falls back to the code', () => {
    expect(errorMessage({ code: 'readOnly', message: 'something new', detail: null })).toBe(
      'The database is open read-only because another computer is using it.',
    );
  });

  it('fills the detail into placeholders', () => {
    expect(errorMessage({ code: 'cannotReadKey', message: "Can't read the SSH key: %@", detail: '/k' })).toBe(
      "Can't read the SSH key: /k",
    );
    expect(errorMessage({ code: 'inUse', message: 'Already used by: %@', detail: 'lock' })).toBe(
      'Already used by: Lock',
    );
  });

  it('names the missing app and the launch failure', () => {
    expect(errorMessage({ code: 'appNotInstalled', message: '%@ is not installed.', detail: 'WinBox' })).toBe(
      'WinBox is not installed.',
    );
    expect(errorMessage({ code: 'launchFailed', message: 'x', detail: 'boom' })).toBe(
      "Couldn't launch: boom",
    );
    expect(errorMessage({ code: 'invalidUrl', message: 'Invalid URL: %@', detail: 'a b' })).toBe(
      'Invalid URL: a b',
    );
  });

  it('appends technical details', () => {
    expect(errorMessage({ code: 'database', message: 'The database failed.', detail: 'disk I/O' })).toBe(
      'The database failed. (disk I/O)',
    );
    expect(errorMessage('bridge down')).toBe('The command failed. (bridge down)');
  });

  it('translates the entry error codes', () => {
    const message = (code: string) => errorMessage({ code, message: code, detail: null });
    expect(message('noConnections')).toBe('An entry needs at least one connection.');
    expect(message('mergeIntoSelf')).toBe("An entry can't be merged into itself.");
    expect(message('multipleDefaults')).toBe('Only one connection can be the default.');
    setLocale('hu');
    expect(message('noConnections')).toBe('Egy elemnek legalább egy kapcsolat kell.');
    expect(message('mergeIntoSelf')).toBe('Egy elem nem vonható össze önmagával.');
    expect(message('multipleDefaults')).toBe('Csak egy kapcsolat lehet az alapértelmezett.');
  });

  it('keeps an unknown message as is', () => {
    expect(errorMessage({ code: 'weird', message: 'Weird thing', detail: null })).toBe('Weird thing');
  });

  it('translates the error screen', () => {
    expect(
      noDatabaseMessage({
        kind: 'noDatabase',
        code: 'openFailed',
        error: "Can't open the database: locked file",
      }),
    ).toBe("Can't open the database: locked file");
    expect(noDatabaseMessage({ kind: 'noDatabase', code: 'lockCancelled', error: 'x' })).toBe(
      'The database is in use on another computer.',
    );
    const upgrade = {
      kind: 'noDatabase' as const,
      code: 'readOnlyNeedsUpgrade' as const,
      error:
        "This database needs an upgrade, which can't be done while it is open on another computer. Close it there, or update the app there first, then retry.",
    };
    expect(noDatabaseMessage(upgrade)).toBe(upgrade.error);
    setLocale('hu');
    expect(noDatabaseMessage({ kind: 'noDatabase', code: 'noPath', error: 'No database is open.' })).toBe(
      'Nincs megnyitott adatbázis.',
    );
    expect(noDatabaseMessage(upgrade)).toContain('frissíteni kell');
    expect(noDatabaseMessage({ ...upgrade, error: 'x' })).toContain('frissíteni kell');
  });

  it('names shortcut commands', () => {
    expect(commandTitle('newRDP')).toBe('New RDP connection');
    expect(commandTitle('unknown')).toBe('unknown');
  });
});
