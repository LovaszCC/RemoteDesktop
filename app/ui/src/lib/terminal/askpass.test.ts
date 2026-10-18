import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { fakeBridge } from '../testing/bridge';
import type { AskpassPromptDto } from '../types';
import { createAskpassStore } from './askpass.svelte';
import AskpassDialog from './AskpassDialog.svelte';

const prompt = (
  requestId: string,
  kind: AskpassPromptDto['kind'],
  text = 'u@h password: ',
): AskpassPromptDto => ({
  requestId,
  sessionId: 'S1',
  kind,
  prompt: text,
});

function setup() {
  const bridge = fakeBridge({ askpass_answer: null });
  const store = createAskpassStore(bridge);
  return { bridge, store };
}

let mounted: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

function render(store: ReturnType<typeof createAskpassStore>) {
  mounted = mount(AskpassDialog, { target: document.body, props: { askpass: store } });
  flushSync();
}

const button = (label: string) =>
  [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label)!;
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('askpass store', () => {
  it('queues prompts from the events and drops closed ones', async () => {
    const { bridge, store } = setup();
    await store.start();
    bridge.emit('askpass-prompt', prompt('R1', 'password'));
    bridge.emit('askpass-prompt', prompt('R2', 'confirm'));
    bridge.emit('askpass-prompt', prompt('R1', 'password'));
    expect(store.current?.requestId).toBe('R1');
    expect(store.queued).toBe(2);
    bridge.emit('askpass-closed', { requestId: 'R1' });
    expect(store.current?.requestId).toBe('R2');
    store.stop();
    expect(bridge.listenerCount('askpass-prompt')).toBe(0);
  });

  it('answers the current prompt, null refusing', async () => {
    const { bridge, store } = setup();
    store.add(prompt('R1', 'password'));
    store.add(prompt('R2', 'other'));
    await store.answer('secret');
    expect(bridge.invoke).toHaveBeenCalledWith('askpass_answer', { requestId: 'R1', answer: 'secret' });
    await store.answer(null);
    expect(bridge.invoke).toHaveBeenCalledWith('askpass_answer', { requestId: 'R2', answer: null });
    expect(store.current).toBeNull();
    await store.answer('ignored');
    expect(bridge.invoke).toHaveBeenCalledTimes(2);
  });
});

describe('AskpassDialog', () => {
  it('asks a password in a secure field and sends it with OK', async () => {
    const { bridge, store } = setup();
    store.add(prompt('R1', 'password'));
    render(store);
    expect(document.body.textContent).toContain('Password');
    const field = document.querySelector<HTMLInputElement>('input')!;
    expect(field.type).toBe('password');
    field.value = 'pw';
    field.dispatchEvent(new Event('input'));
    button('OK').click();
    await flush();
    expect(bridge.invoke).toHaveBeenCalledWith('askpass_answer', { requestId: 'R1', answer: 'pw' });
    flushSync();
    expect(document.querySelector('input')).toBeNull();
  });

  it('asks a passphrase; Cancel refuses', async () => {
    const { bridge, store } = setup();
    store.add(prompt('R1', 'passphrase', "Enter passphrase for key 'id': "));
    render(store);
    expect(document.body.textContent).toContain('Key passphrase');
    button('Cancel').click();
    await flush();
    expect(bridge.invoke).toHaveBeenCalledWith('askpass_answer', { requestId: 'R1', answer: null });
  });

  it('shows a host key question in monospace with Yes and No', async () => {
    const { bridge, store } = setup();
    const text = "The authenticity of host ... can't be established.\nAre you sure (yes/no)?";
    store.add(prompt('R1', 'confirm', text));
    store.add(prompt('R2', 'other', 'Verification code: '));
    render(store);
    expect(document.querySelector('[data-testid="askpass-text"]')?.textContent).toBe(text);
    expect(document.querySelector('input')).toBeNull();
    button('Yes').click();
    await flush();
    expect(bridge.invoke).toHaveBeenCalledWith('askpass_answer', { requestId: 'R1', answer: 'yes' });
    flushSync();
    // The next prompt: a plain text field.
    const field = document.querySelector<HTMLInputElement>('input')!;
    expect(field.type).toBe('text');
    field.value = '123';
    field.dispatchEvent(new Event('input'));
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await flush();
    expect(bridge.invoke).toHaveBeenCalledWith('askpass_answer', { requestId: 'R2', answer: '123' });
  });

  it('closes when the session ends', () => {
    const { store } = setup();
    store.add(prompt('R1', 'confirm', 'yes/no?'));
    render(store);
    expect(button('No')).toBeDefined();
    store.closed('R1');
    flushSync();
    expect(document.body.textContent).not.toContain('yes/no?');
  });
});
