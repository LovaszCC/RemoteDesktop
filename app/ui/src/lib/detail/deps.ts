// What the detail views and the editors use, from Svelte context so mounted tests can pass fakes; the
// app-wide stores otherwise.

import { getContext } from 'svelte';
import { api, type Api } from '../api';
import { forwards, type ForwardStore } from '../forwards/forwards.svelte';
import { sidebarActions, type SidebarActions } from '../sidebar/actions.svelte';
import { alerts, type AlertStore } from '../stores/alert.svelte';
import { browsers, type BrowsersStore } from '../stores/browsers.svelte';
import { app } from '../stores/app.svelte';
import { editor, type EditorStore } from '../stores/editor.svelte';
import { library, type LibraryStore } from '../stores/library.svelte';
import { settings, type SettingsStore } from '../stores/settings.svelte';
import { toast, type ToastStore } from '../stores/toast.svelte';
import type { Platform } from '../types';

export interface ViewDeps {
  api: Api;
  library: LibraryStore;
  actions: SidebarActions;
  editor: EditorStore;
  settings: SettingsStore;
  toast: ToastStore;
  alerts: AlertStore;
  forwards: ForwardStore;
  browsers: BrowsersStore;
  readonly readOnly: boolean;
  readonly platform: Platform;
}

export const VIEW_DEPS = Symbol('view-deps');

const defaults: ViewDeps = {
  api,
  library,
  actions: sidebarActions,
  editor,
  settings,
  toast,
  alerts,
  forwards,
  browsers,
  get readOnly() {
    return app.readOnly;
  },
  get platform() {
    return app.platform;
  },
};

/** The deps from context, or the app-wide ones. Call during component init. */
export function viewDeps(): ViewDeps {
  return getContext<ViewDeps | undefined>(VIEW_DEPS) ?? defaults;
}
