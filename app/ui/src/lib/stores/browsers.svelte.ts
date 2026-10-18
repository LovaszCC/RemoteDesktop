// The browsers registered for https here (`installed_browsers`), for the connection editor's picker and
// the web detail subtitle. Loaded once on first use.

import { t } from '../i18n/index.svelte';
import type { BrowserDto } from '../types';
import { resolveDeps, type StoreDeps } from './deps';

/** The name of the browser with `id`: the installed one's name, else the id itself (Swift
 * `InstalledBrowsers.name(for:)`). */
export function browserName(id: string, installed: readonly BrowserDto[]): string {
  return installed.find((b) => b.id === id)?.name ?? id;
}

/**
 * The editor's picker: "Default" (`""`), the installed browsers, and a saved browser that isn't installed
 * here marked " (not installed)".
 */
export function browserPickerOptions(
  saved: string | null,
  installed: readonly BrowserDto[],
): { value: string; label: string }[] {
  const options = [
    { value: '', label: t('Default') },
    ...installed.map((browser) => ({ value: browser.id, label: browser.name })),
  ];
  if (saved && !installed.some((b) => b.id === saved)) {
    options.push({ value: saved, label: t('%@ (not installed)', saved) });
  }
  return options;
}

export function createBrowsersStore(deps: StoreDeps = {}) {
  const { api } = resolveDeps(deps);
  let installed = $state<BrowserDto[]>([]);
  let loading: Promise<void> | null = null;

  return {
    get installed(): readonly BrowserDto[] {
      return installed;
    },
    /** The name for `id` (the id when the browser isn't installed). */
    name(id: string): string {
      return browserName(id, installed);
    },
    /** Loads the list once (failures leave it empty and allow a retry). */
    load(): Promise<void> {
      loading ??= api
        .installedBrowsers()
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

export type BrowsersStore = ReturnType<typeof createBrowsersStore>;

export const browsers = createBrowsersStore();
