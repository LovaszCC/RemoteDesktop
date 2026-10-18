// Which command's shortcut recorder is listening (Swift `ShortcutsSettingsView.recording`). Shared so the
// global dispatcher stays quiet while a shortcut is being recorded.

import type { ShortcutCommand } from '../types';

export function createRecorderStore() {
  let recording = $state<ShortcutCommand | null>(null);
  let errors = $state<Partial<Record<ShortcutCommand, string>>>({});

  return {
    get recording() {
      return recording;
    },
    get active() {
      return recording !== null;
    },
    error(command: ShortcutCommand): string | null {
      return errors[command] ?? null;
    },
    /** Starts (or with the same command again, stops) recording; clears that row's error. */
    toggle(command: ShortcutCommand) {
      recording = recording === command ? null : command;
      errors = { ...errors, [command]: undefined };
    },
    stop() {
      recording = null;
    },
    setError(command: ShortcutCommand, message: string | null) {
      errors = { ...errors, [command]: message ?? undefined };
    },
    clearErrors() {
      errors = {};
    },
  };
}

export type RecorderStore = ReturnType<typeof createRecorderStore>;

export const recorder = createRecorderStore();
