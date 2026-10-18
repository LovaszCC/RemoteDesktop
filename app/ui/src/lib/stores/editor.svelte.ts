// Which editor sheet is open (Swift `AppState.editorDraft`, `credentialEditor`, `folderEditor`). The
// sidebar, the toolbar and the menus only ask for an editor here; the views render them. Phase 2 step 5
// renders the folder name dialog; the connection and credential editors come in step 6.

import type { ConnectionType } from '../types';

export type ConnectionEditorRequest =
  { mode: 'new'; type: ConnectionType; folderId: string | null } | { mode: 'edit'; id: string };
export type CredentialEditorRequest = { mode: 'new' } | { mode: 'edit'; id: string };
export type FolderEditorRequest = { mode: 'new'; parentId: string | null } | { mode: 'edit'; id: string };

export function createEditorStore() {
  let connection = $state<ConnectionEditorRequest | null>(null);
  let credential = $state<CredentialEditorRequest | null>(null);
  let folder = $state<FolderEditorRequest | null>(null);

  return {
    /** The open connection editor, or `null`. */
    get connection() {
      return connection;
    },
    get credential() {
      return credential;
    },
    get folder() {
      return folder;
    },

    /** `{type, folderId}` opens a new connection of that type; `{id}` edits one. */
    openConnectionEditor(request: { type: ConnectionType; folderId?: string | null } | { id: string }) {
      connection =
        'id' in request
          ? { mode: 'edit', id: request.id }
          : { mode: 'new', type: request.type, folderId: request.folderId ?? null };
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

    closeConnectionEditor() {
      connection = null;
    },
    closeCredentialEditor() {
      credential = null;
    },
    closeFolderEditor() {
      folder = null;
    },
    /** Lock: every sheet goes away. */
    closeAll() {
      connection = null;
      credential = null;
      folder = null;
    },
  };
}

export type EditorStore = ReturnType<typeof createEditorStore>;

export const editor = createEditorStore();
