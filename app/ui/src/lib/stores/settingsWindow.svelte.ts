// Whether the Settings sheet is open and which tab it shows ("General" or "Keyboard Shortcuts").

export type SettingsTab = 'general' | 'shortcuts' | 'vpn';

export function createSettingsWindowStore() {
  let open = $state(false);
  let tab = $state<SettingsTab>('general');

  return {
    get isOpen() {
      return open;
    },
    get tab() {
      return tab;
    },
    set tab(next: SettingsTab) {
      tab = next;
    },
    /** Opens Settings, on `which` tab when given. */
    open(which?: SettingsTab) {
      if (which) tab = which;
      open = true;
    },
    close() {
      open = false;
    },
  };
}

export type SettingsWindowStore = ReturnType<typeof createSettingsWindowStore>;

export const settingsWindow = createSettingsWindowStore();
