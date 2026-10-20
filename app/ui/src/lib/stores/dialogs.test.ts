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

  it('resolves a confirmation with its checkbox, null when cancelled', async () => {
    const dialogs = createDialogStore();
    let answer = dialogs.confirmWithOption({ title: 'Sure?', confirmLabel: 'Delete', option: 'Also' });
    expect(dialogs.current?.kind === 'confirm' && dialogs.current.option).toBe('Also');
    dialogs.accept(true);
    expect(await answer).toEqual({ checked: true });
    answer = dialogs.confirmWithOption({ title: 'Sure?', confirmLabel: 'Delete' });
    dialogs.accept();
    expect(await answer).toEqual({ checked: false });
    answer = dialogs.confirmWithOption({ title: 'Sure?', confirmLabel: 'Delete' });
    dialogs.cancel();
    expect(await answer).toBeNull();
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
    editor.openEntryEditor({ type: 'ssh' });
    expect(editor.entry).toEqual({ mode: 'new', type: 'ssh', folderId: null });
    editor.openEntryEditor({ mode: 'new', type: 'web', folderId: 'f1' });
    expect(editor.entry).toEqual({ mode: 'new', type: 'web', folderId: 'f1' });
    editor.openEntryEditor({ id: 'e1' });
    expect(editor.entry).toEqual({ mode: 'edit', id: 'e1' });
    editor.openEntryEditor({ id: 'e1', connectionId: 'h2' });
    expect(editor.entry).toEqual({ mode: 'edit', id: 'e1', connectionId: 'h2' });
    editor.openCredentialEditor();
    expect(editor.credential).toEqual({ mode: 'new' });
    editor.openFolderEditor({ parentId: 'f1' });
    expect(editor.folder).toEqual({ mode: 'new', parentId: 'f1' });
    editor.closeEntryEditor();
    expect(editor.entry).toBeNull();
    editor.closeAll();
    expect([editor.credential, editor.folder]).toEqual([null, null]);
  });
});
