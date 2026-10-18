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
    setLocale('hu');
    expect(noDatabaseMessage({ kind: 'noDatabase', code: 'noPath', error: 'No database is open.' })).toBe(
      'Nincs megnyitott adatbázis.',
    );
  });

  it('names shortcut commands', () => {
    expect(commandTitle('newRDP')).toBe('New RDP connection');
    expect(commandTitle('unknown')).toBe('unknown');
  });
});
