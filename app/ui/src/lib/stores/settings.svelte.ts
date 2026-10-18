// Settings and keyboard shortcuts, mirrored from Rust (`settings-changed`, `shortcuts-changed`). The
// language setting drives the i18n locale live.

import { onSettingsChanged, onShortcutsChanged, type Unlisten } from '../api';
import { setLocale } from '../i18n/index.svelte';
import type { KeyShortcut, SettingsDto, SettingsPatch, ShortcutCommand, ShortcutDto } from '../types';
import { resolveDeps, type StoreDeps } from './deps';

export interface SettingsStoreOptions extends StoreDeps {
  /** Called with the language whenever settings arrive; defaults to the i18n `setLocale`. */
  onLanguage?: (language: string) => void;
}

export function createSettingsStore(options: SettingsStoreOptions = {}) {
  const { api, listen } = resolveDeps(options);
  const onLanguage = options.onLanguage ?? setLocale;
  let settings = $state<SettingsDto | null>(null);
  let shortcuts = $state<ShortcutDto[]>([]);
  let unlisten: Unlisten[] = [];

  function applySettings(next: SettingsDto): SettingsDto {
    if (next.language !== settings?.language) onLanguage(next.language);
    settings = next;
    return next;
  }

  function applyShortcuts(rows: ShortcutDto[]): ShortcutDto[] {
    shortcuts = rows;
    return rows;
  }

  return {
    /** `null` until loaded. */
    get settings() {
      return settings;
    },
    get shortcuts() {
      return shortcuts;
    },
    get customizedCount() {
      return shortcuts.filter((s) => s.customized).length;
    },
    /** The effective row of `command`. */
    shortcut(command: ShortcutCommand): ShortcutDto | undefined {
      return shortcuts.find((s) => s.command === command);
    },

    /** Subscribes to both events and loads settings and shortcuts. */
    async start() {
      if (unlisten.length === 0) {
        unlisten = [
          await onSettingsChanged(listen, (next) => void applySettings(next)),
          await onShortcutsChanged(listen, (rows) => void applyShortcuts(rows)),
        ];
      }
      const [loaded, rows] = await Promise.all([api.settingsGet(), api.shortcutsGet()]);
      applySettings(loaded);
      applyShortcuts(rows);
    },
    stop() {
      for (const off of unlisten) off();
      unlisten = [];
    },

    /** Applies a partial change. A language change switches the UI before Rust answers. */
    async update(patch: SettingsPatch) {
      if (patch.language) onLanguage(patch.language);
      return applySettings(await api.settingsSet(patch));
    },
    setLanguage(language: SettingsPatch['language'] & string) {
      return this.update({ language });
    },

    // Shortcuts. Refusals reject as `CommandError` (`missingModifier`, `reserved`, `inUse`, …).
    setShortcut: async (command: ShortcutCommand, shortcut: KeyShortcut | null) =>
      applyShortcuts(await api.shortcutSet(command, shortcut)),
    resetShortcut: async (command: ShortcutCommand) => applyShortcuts(await api.shortcutReset(command)),
    resetAllShortcuts: async () => applyShortcuts(await api.shortcutsResetAll()),
  };
}

export type SettingsStore = ReturnType<typeof createSettingsStore>;

export const settings = createSettingsStore();
