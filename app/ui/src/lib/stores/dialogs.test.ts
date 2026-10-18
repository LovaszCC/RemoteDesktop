import { describe, expect, it } from 'vitest';
import { createDialogStore } from './dialogs.svelte';
import { createEditorStore } from './editor.svelte';

describe('dialog store', () => {
  it('resolves confirmations', async () => {
    const dialogs = createDialogStore();
    const yes = dialogs.confirm({ title: 'Sure?', confirmLabel: 'Delete' });
    expect(dialogs.current?.title).toBe('Sure?');
    dialogs.accept();
    expect(await yes).toBe(true);
    expect(dialogs.current).toBeNull();
    const no = dialogs.confirm({ title: 'Sure?', confirmLabel: 'Delete' });
    dialogs.cancel();
    expect(await no).toBe(false);
  });

  it('resolves prompts with the trimmed name, null for blank or cancel', async () => {
    const dialogs = createDialogStore();
    let name = dialogs.prompt({ title: 'Name', label: 'Name', confirmLabel: 'Save' });
    dialogs.accept('  a b ');
    expect(await name).toBe('a b');
    name = dialogs.prompt({ title: 'Name', label: 'Name', confirmLabel: 'Save' });
    dialogs.accept('  ');
    expect(await name).toBeNull();
    name = dialogs.prompt({ title: 'Name', label: 'Name', confirmLabel: 'Save' });
    dialogs.cancel();
    expect(await name).toBeNull();
  });

  it('a new dialog dismisses the open one', async () => {
    const dialogs = createDialogStore();
    const first = dialogs.confirm({ title: 'One', confirmLabel: 'OK' });
    const second = dialogs.prompt({ title: 'Two', label: 'Name', confirmLabel: 'OK' });
    expect(await first).toBe(false);
    dialogs.accept('x');
    expect(await second).toBe('x');
  });
});

describe('editor store', () => {
  it('opens and closes the editor requests', () => {
    const editor = createEditorStore();
    editor.openConnectionEditor({ type: 'ssh' });
    expect(editor.connection).toEqual({ mode: 'new', type: 'ssh', folderId: null });
    editor.openConnectionEditor({ id: 'c1' });
    expect(editor.connection).toEqual({ mode: 'edit', id: 'c1' });
    editor.openCredentialEditor();
    expect(editor.credential).toEqual({ mode: 'new' });
    editor.openFolderEditor({ parentId: 'f1' });
    expect(editor.folder).toEqual({ mode: 'new', parentId: 'f1' });
    editor.closeConnectionEditor();
    expect(editor.connection).toBeNull();
    editor.closeAll();
    expect([editor.credential, editor.folder]).toEqual([null, null]);
  });
});
