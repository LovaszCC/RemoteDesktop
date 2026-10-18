import { afterEach, describe, expect, it } from 'vitest';
import { setLocale } from '../i18n/index.svelte';
import { LOCAL_TERMINAL, isLocalTerminal, localTerminalName } from './local';

afterEach(() => setLocale('en'));

describe('local terminal', () => {
  it('is told apart from library connections by its id', () => {
    expect(isLocalTerminal(LOCAL_TERMINAL.id)).toBe(true);
    expect(isLocalTerminal('c1')).toBe(false);
    expect(LOCAL_TERMINAL).toMatchObject({ type: 'ssh', color: null });
  });

  it('numbers the tab titles with the lowest free number', () => {
    expect(localTerminalName([])).toBe('Local Terminal');
    expect(localTerminalName(['Local Terminal'])).toBe('Local Terminal 2');
    expect(localTerminalName(['Local Terminal 2'])).toBe('Local Terminal');
    expect(localTerminalName(['Local Terminal', 'Local Terminal 2', 'web01'])).toBe('Local Terminal 3');
    setLocale('hu');
    expect(localTerminalName(['Helyi terminál'])).toBe('Helyi terminál 2');
  });
});
