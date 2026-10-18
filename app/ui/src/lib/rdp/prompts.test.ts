import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { createAlertStore } from '../stores/alert.svelte';
import { fakeBridge } from '../testing/bridge';
import type { RdpCertPromptDto, RdpCredentialsPromptDto } from '../types';
import { createRdpPromptsStore } from './prompts.svelte';
import RdpPromptDialog from './RdpPromptDialog.svelte';

const cert = (requestId: string, patch: Partial<RdpCertPromptDto> = {}): RdpCertPromptDto => ({
  requestId,
  sessionId: 'S1',
  connectionName: 'Desk',
  host: 'desk.example.com',
  port: 3389,
  fingerprint: 'AB:CD:EF',
  changed: false,
  nameMismatch: false,
  commonName: 'desk.example.com',
  subject: 'CN=desk.example.com',
  issuer: 'CN=Example CA',
  ...patch,
});

const credentials = (
  requestId: string,
  patch: Partial<RdpCredentialsPromptDto> = {},
): RdpCredentialsPromptDto => ({
  requestId,
  sessionId: 'S1',
  connectionName: 'Desk',
  username: 'alice',
  domain: 'EXAMPLE',
  canSave: true,
  rejected: false,
  ...patch,
});

function setup(replies: Parameters<typeof fakeBridge>[0] = {}) {
  const bridge = fakeBridge({ rdp_cert_answer: null, rdp_credentials_answer: null, ...replies });
  const alerts = createAlertStore();
  const store = createRdpPromptsStore({ ...bridge, alerts });
  return { bridge, store, alerts };
}

let mounted: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

function render(store: ReturnType<typeof createRdpPromptsStore>) {
  mounted = mount(RdpPromptDialog, { target: document.body, props: { prompts: store } });
  flushSync();
}

const button = (label: string) =>
  [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label)!;
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const input = (id: string) => document.getElementById(id) as HTMLInputElement;
function type(id: string, value: string) {
  const field = input(id);
  field.value = value;
  field.dispatchEvent(new Event('input', { bubbles: true }));
}

describe('rdp prompts store', () => {
  it('queues both kinds from the events and drops closed ones', async () => {
    const { bridge, store } = setup();
    await store.start();
    bridge.emit('rdp-cert-prompt', cert('R1'));
    bridge.emit('rdp-credentials-prompt', credentials('R2'));
    bridge.emit('rdp-cert-prompt', cert('R1'));
    expect(store.current).toMatchObject({ kind: 'certificate', requestId: 'R1' });
    expect(store.queued).toBe(2);
    bridge.emit('rdp-prompt-closed', { requestId: 'R1' });
    expect(store.current).toMatchObject({ kind: 'credentials', requestId: 'R2' });
    store.stop();
    expect(bridge.listenerCount('rdp-cert-prompt')).toBe(0);
  });

  it('answers only the matching kind', async () => {
    const { bridge, store } = setup();
    store.add({ kind: 'credentials', ...credentials('R1') });
    await store.answerCertificate('reject');
    expect(store.queued).toBe(1);
    await store.answerCredentials(null);
    await store.answerCredentials(null);
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_credentials_answer', { requestId: 'R1', answer: null });
  });

  it('shows a failed save', async () => {
    const { store, alerts } = setup({
      rdp_credentials_answer: () => {
        throw { code: 'readOnly', message: 'The database is open read-only.', detail: null };
      },
    });
    store.add({ kind: 'credentials', ...credentials('R1') });
    await store.answerCredentials({ username: 'a', domain: '', password: 'p', save: true });
    expect(alerts.current).toContain('read-only');
  });
});

describe('RdpPromptDialog', () => {
  it('shows an unknown certificate with its fingerprint and answers each button', async () => {
    const { bridge, store } = setup();
    for (const id of ['R1', 'R2', 'R3']) store.add({ kind: 'certificate', ...cert(id) });
    render(store);
    expect(document.body.textContent).toContain('Unknown certificate: Desk');
    expect(document.body.textContent).toContain('Server: desk.example.com:3389');
    expect(document.body.textContent).toContain('Issuer: CN=Example CA');
    expect(document.querySelector('[data-testid="rdp-fingerprint"]')?.textContent).toBe(
      'Fingerprint: AB:CD:EF',
    );
    expect(document.querySelector('[role="alert"]')).toBeNull();
    button('Accept').click();
    await flush();
    flushSync();
    button('Just this time').click();
    await flush();
    flushSync();
    button('Reject').click();
    await flush();
    flushSync();
    expect(bridge.invoke.mock.calls).toEqual([
      ['rdp_cert_answer', { requestId: 'R1', choice: 'acceptPermanently' }],
      ['rdp_cert_answer', { requestId: 'R2', choice: 'acceptOnce' }],
      ['rdp_cert_answer', { requestId: 'R3', choice: 'reject' }],
    ]);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it('warns about a changed certificate and a name mismatch; Esc rejects', async () => {
    const { bridge, store } = setup();
    store.add({ kind: 'certificate', ...cert('R1', { changed: true, nameMismatch: true }) });
    render(store);
    expect(document.body.textContent).toContain('The certificate of Desk has changed!');
    const warnings = [...document.querySelectorAll('[role="alert"]')].map((w) => w.textContent);
    expect(warnings).toEqual([
      'Warning: the certificate is not for this host name.',
      'Warning: you accepted a different certificate for this server before.',
    ]);
    document
      .querySelector('[role="dialog"]')!
      .dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await flush();
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_cert_answer', { requestId: 'R1', choice: 'reject' });
  });

  it('signs in with the pre-filled names, the typed password and Save', async () => {
    const { bridge, store } = setup();
    store.add({ kind: 'credentials', ...credentials('R1') });
    render(store);
    expect(document.body.textContent).toContain('Sign in to Desk');
    expect(document.body.textContent).toContain('The server requires Network Level Authentication.');
    expect(input('rdp-username').value).toBe('alice');
    expect(input('rdp-domain').value).toBe('EXAMPLE');
    type('rdp-username', ' bob ');
    type('rdp-password', 'pw');
    const save = document.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    save.click();
    flushSync();
    button('Connect').click();
    await flush();
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_credentials_answer', {
      requestId: 'R1',
      answer: { username: 'bob', domain: 'EXAMPLE', password: 'pw', save: true },
    });
  });

  it('shows the rejection, hides Save without permission and cancels', async () => {
    const { bridge, store } = setup();
    store.add({
      kind: 'credentials',
      ...credentials('R1', { canSave: false, rejected: true, username: '' }),
    });
    render(store);
    expect(document.querySelector('[role="alert"]')?.textContent).toBe(
      'The server rejected the user name or password.',
    );
    expect(document.querySelector('input[type="checkbox"]')).toBeNull();
    button('Cancel').click();
    await flush();
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_credentials_answer', { requestId: 'R1', answer: null });
  });

  it('never saves when the connection can not take it', async () => {
    const { bridge, store } = setup();
    store.add({ kind: 'credentials', ...credentials('R1', { canSave: false }) });
    render(store);
    input('rdp-password').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await flush();
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_credentials_answer', {
      requestId: 'R1',
      answer: { username: 'alice', domain: 'EXAMPLE', password: '', save: false },
    });
  });
});
