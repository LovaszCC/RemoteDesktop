// Which editor sheet is open (Swift `AppState.editorDraft`, `credentialEditor`, `folderEditor`). The
// sidebar, the toolbar and the menus only ask for an editor here; the views render them.

import type { ConnectionType } from '../types';

/** A new entry with one connection of `type`, or an existing entry (on a connection's tab). */
export type EntryEditorRequest =
  | { mode: 'new'; type: ConnectionType; folderId: string | null }
  | { mode: 'edit'; id: string; connectionId?: string };
export type CredentialEditorRequest = { mode: 'new' } | { mode: 'edit'; id: string };
export type FolderEditorRequest = { mode: 'new'; parentId: string | null } | { mode: 'edit'; id: string };

export function createEditorStore() {
  let entry = $state<EntryEditorRequest | null>(null);
  let credential = $state<CredentialEditorRequest | null>(null);
  let folder = $state<FolderEditorRequest | null>(null);

  return {
    /** The open entry editor, or `null`. */
    get entry() {
      return entry;
    },
    get credential() {
      return credential;
    },
    get folder() {
      return folder;
    },

    /** `{type, folderId}` opens a new entry with one connection of that type; `{id}` edits an entry,
     * on the `connectionId` tab when given (else General). */
    openEntryEditor(
      request:
        | { mode?: 'new'; type: ConnectionType; folderId?: string | null }
        | { id: string; connectionId?: string },
    ) {
      if (!('id' in request)) {
        entry = { mode: 'new', type: request.type, folderId: request.folderId ?? null };
      } else {
        const { id, connectionId } = request;
        entry = connectionId ? { mode: 'edit', id, connectionId } : { mode: 'edit', id };
      }
    },
    /** No id: a new credential. */
    openCredentialEditor(id?: string) {
      credential = id === undefined ? { mode: 'new' } : { mode: 'edit', id };
    },
    /** `{parentId}` creates a folder there; `{id}` renames one. */
    openFolderEditor(request: { parentId?: string | null } | { id: string }) {
      folder =
        'id' in request
          ? { mode: 'edit', id: request.id }
          : { mode: 'new', parentId: request.parentId ?? null };
    },

    closeEntryEditor() {
      entry = null;
    },
    closeCredentialEditor() {
      credential = null;
    },
    closeFolderEditor() {
      folder = null;
    },
    /** Lock: every sheet goes away. */
    closeAll() {
      entry = null;
      credential = null;
      folder = null;
    },
  };
}

export type EditorStore = ReturnType<typeof createEditorStore>;

export const editor = createEditorStore();
