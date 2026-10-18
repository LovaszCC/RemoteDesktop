// Mounts a detail view or an editor with fake deps (a scripted bridge, fresh stores).

import { flushSync, mount, unmount, type Component } from 'svelte';
import { createApi } from '../api';
import { VIEW_DEPS, type ViewDeps } from '../detail/deps';
import { createForwardStore } from '../forwards/forwards.svelte';
import { createSidebarActions } from '../sidebar/actions.svelte';
import { createAlertStore } from '../stores/alert.svelte';
import { createBrowsersStore } from '../stores/browsers.svelte';
import { createDialogStore } from '../stores/dialogs.svelte';
import { createEditorStore } from '../stores/editor.svelte';
import { createLibraryStore } from '../stores/library.svelte';
import { createSettingsStore } from '../stores/settings.svelte';
import { createToastStore } from '../stores/toast.svelte';
import type { LibraryDto, Platform } from '../types';
import { fakeBridge } from './bridge';
import { sidebarLibrary } from './fixtures';

export interface ViewOptions {
  library?: LibraryDto;
  replies?: Parameters<typeof fakeBridge>[0];
  readOnly?: boolean;
  platform?: Platform;
}

let mounted: ReturnType<typeof mount> | undefined;

/** Unmounts what `renderView` mounted; call from `afterEach`. */
export function cleanupView() {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
}

export function renderView<P extends Record<string, unknown>>(
  component: Component<P>,
  props: P,
  { library = sidebarLibrary(), replies = {}, readOnly = false, platform = 'macos' }: ViewOptions = {},
) {
  const bridge = fakeBridge({ set_credential: 1, copy_text: undefined, installed_browsers: [], ...replies });
  const api = createApi(bridge.invoke);
  const store = createLibraryStore({ ...bridge, searchDelay: 0 });
  store.replace(library);
  const toast = createToastStore();
  const alerts = createAlertStore();
  const editor = createEditorStore();
  const settings = createSettingsStore({ ...bridge, onLanguage: () => {} });
  const actions = createSidebarActions({
    api,
    library: store,
    settings,
    toast,
    alerts,
    editor,
    dialogs: createDialogStore(),
  });
  const forwards = createForwardStore(bridge);
  const deps: ViewDeps = {
    api,
    library: store,
    actions,
    editor,
    settings,
    toast,
    alerts,
    forwards,
    browsers: createBrowsersStore(bridge),
    readOnly,
    platform,
  };
  mounted = mount(component, {
    target: document.body,
    props,
    context: new Map([[VIEW_DEPS, deps]]),
  });
  flushSync();
  return { bridge, deps, store, toast, alerts, editor, forwards };
}

/** Lets pending promises settle, then flushes Svelte. */
export async function settle() {
  for (let i = 0; i < 5; i++) await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
}

export const text = () => document.body.textContent?.replace(/\s+/g, ' ') ?? '';

export function buttonByText(label: string): HTMLButtonElement {
  const button = [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);
  if (!button) throw new Error(`no button "${label}"`);
  return button;
}

export function click(element: Element) {
  element.dispatchEvent(new MouseEvent('click', { bubbles: true }));
  flushSync();
}

export function type(input: HTMLInputElement | HTMLTextAreaElement, value: string) {
  input.value = value;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  flushSync();
}

export function check(input: HTMLInputElement, on: boolean) {
  input.checked = on;
  input.dispatchEvent(new Event('change', { bubbles: true }));
  flushSync();
}
