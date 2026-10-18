// The external terminals installed here (`installed_terminals`), for Settings and "Open in …". Loaded once
// at start; Rust picks the stored one if set, else the first installed.

import { t } from '../i18n/index.svelte';
import type { TerminalChoiceDto } from '../types';
import { resolveDeps, type StoreDeps } from './deps';

/** Names of the ids Rust knows, for a saved terminal that isn't installed here. */
const KNOWN_NAMES: Record<string, string> = {
  ghostty: 'Ghostty',
  iterm2: 'iTerm2',
  terminal: 'Terminal',
  wt: 'Windows Terminal',
  conhost: 'Console Host',
};

/**
 * The Settings picker: the system default (`""`), the installed terminals, and a saved choice that isn't
 * installed (any more) marked " (not installed)".
 */
export function externalTerminalOptions(
  current: string | null,
  installed: readonly TerminalChoiceDto[],
): { value: string; label: string }[] {
  const options = [
    { value: '', label: t('System default') },
    ...installed.map((choice) => ({ value: choice.id, label: choice.name })),
  ];
  if (current && !installed.some((c) => c.id === current)) {
    options.push({ value: current, label: t('%@ (not installed)', KNOWN_NAMES[current] ?? current) });
  }
  return options;
}

/** The name of the terminal "Open in …" uses: the saved one, else the first installed, else `fallback`. */
export function externalTerminalName(
  current: string | null,
  installed: readonly TerminalChoiceDto[],
  fallback: string,
): string {
  const saved = installed.find((c) => c.id === current)?.name ?? (current ? KNOWN_NAMES[current] : undefined);
  return saved ?? installed[0]?.name ?? fallback;
}

export function createTerminalsStore(deps: StoreDeps = {}) {
  const { api } = resolveDeps(deps);
  let installed = $state<TerminalChoiceDto[]>([]);
  let loading: Promise<void> | null = null;

  return {
    get installed(): readonly TerminalChoiceDto[] {
      return installed;
    },
    /** Loads the list once (failures leave it empty). */
    load(): Promise<void> {
      loading ??= api
        .installedTerminals()
        .then((list) => {
          installed = list;
        })
        .catch(() => {
          loading = null;
        });
      return loading;
    },
  };
}

export type TerminalsStore = ReturnType<typeof createTerminalsStore>;

export const terminals = createTerminalsStore();
