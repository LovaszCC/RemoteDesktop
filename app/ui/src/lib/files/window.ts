// Which window this page runs in. File browser windows are labelled `files-<connection id>`.

/** The label prefix of file browser windows (Rust `FILES_LABEL_PREFIX`). */
export const FILES_LABEL_PREFIX = 'files-';

interface TauriInternals {
  metadata?: { currentWindow?: { label?: string }; currentWebview?: { label?: string } };
}

/** The current window's label (`main` outside Tauri). */
export function windowLabel(
  internals: unknown = (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__,
): string {
  const metadata = (internals as TauriInternals | undefined)?.metadata;
  return metadata?.currentWebview?.label ?? metadata?.currentWindow?.label ?? 'main';
}
