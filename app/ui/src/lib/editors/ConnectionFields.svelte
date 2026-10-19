<script lang="ts">
  import Button from '../components/Button.svelte';
  import Segmented from '../components/Segmented.svelte';
  import Select from '../components/Select.svelte';
  import TextField from '../components/TextField.svelte';
  import Toggle from '../components/Toggle.svelte';
  import { viewDeps } from '../detail/deps';
  import { EMPTY, MASKED, mikrotikModeName, rdpClientName } from '../detail/model';
  import { t } from '../i18n/index.svelte';
  import { CONNECTION_TYPES, typeName, usesSSH } from '../sidebar/model';
  import type { MikrotikMode, RdpClient, SshTerminal } from '../types';
  import FormRow from './FormRow.svelte';
  import FormSection from './FormSection.svelte';
  import {
    DEFAULT_PORTS,
    browserOptions,
    connectionTabLabel,
    credentialOptions,
    fromPicker,
    splitPastedUrl,
    toPicker,
    webUrlPreview,
    type ConnectionForm,
  } from './model';
  import SecretInput from './SecretInput.svelte';
  import SshKeyField from './SshKeyField.svelte';

  /**
   * One connection tab of the entry editor (Swift `ConnectionEditorView`'s per-connection sections): the
   * type (a new connection only), label, port, type options, web scheme/port/path (a pasted URL is split
   * into them), login, "Show as button on Details", "Set as default" and "Remove this connection".
   */
  interface Props {
    connection: ConnectionForm;
    /** The entry's name (generated key file names) and host (the URL preview). */
    entryName: string;
    entryHost: string;
    isDefault: boolean;
    /** Rust's `invalidPort` message, when it is this connection's. */
    portError?: string;
    /** A pasted URL's host that differs from the entry's ("Use <host>"). */
    hostOffer?: string | null;
    /** A pasted URL's host. */
    onpastehost: (host: string) => void;
    onusehost: (host: string) => void;
    onsetdefault: () => void;
    onremove: () => void;
    /** The port was edited (clears its error). */
    onportinput?: () => void;
  }

  let {
    connection = $bindable(),
    entryName,
    entryHost,
    isDefault,
    portError,
    hostOffer = null,
    onpastehost,
    onusehost,
    onsetdefault,
    onremove,
    onportinput,
  }: Props = $props();
  const deps = viewDeps();
  const uid = Math.random().toString(36).slice(2);
  const id = (name: string) => `conn-${name}-${uid}`;

  const library = $derived(deps.library.library);
  const credential = $derived(deps.library.credential(connection.credentialId));
  const ssh = $derived(usesSSH(connection.type));
  const web = $derived(connection.type === 'web');

  const MIKROTIK_MODES: MikrotikMode[] = ['winbox', 'ssh'];
  const typeOptions = $derived(CONNECTION_TYPES.map((type) => ({ value: type, label: typeName(type) })));
  const terminalOptions = $derived<{ value: SshTerminal; label: string }[]>([
    { value: 'embedded', label: t('Embedded') },
    { value: 'external', label: t('External terminal') },
  ]);
  const rdpOptions = $derived<{ value: RdpClient; label: string }[]>([
    { value: 'embedded', label: rdpClientName('embedded', deps.platform) },
    { value: 'windowsApp', label: rdpClientName('windowsApp', deps.platform) },
  ]);
  const portPlaceholder = $derived.by(() => {
    if (!web) return String(DEFAULT_PORTS[connection.type]);
    const scheme = connection.webScheme.trim().toLowerCase();
    return scheme === 'https' ? '443' : scheme === 'http' ? '80' : '';
  });
  const autoLabel = $derived(connectionTabLabel({ ...connection, label: '' }, credential));

  /** A full URL pasted into a web field fills scheme, port and path; its host goes to the entry. */
  function pasted(event: ClipboardEvent) {
    const text = event.clipboardData?.getData('text/plain') ?? '';
    if (!text.includes('://')) return;
    const parts = splitPastedUrl(text);
    if (!parts) return;
    event.preventDefault();
    connection.webScheme = parts.scheme;
    connection.port = parts.port === null ? '' : String(parts.port);
    connection.webPath = parts.path;
    onportinput?.();
    onpastehost(parts.host);
  }
</script>

<div class="fields">
  <FormSection title={t('Connection')}>
    {#if connection.id === null}
      <FormRow label={t('Type')}>
        <Segmented bind:value={connection.type} options={typeOptions} label={t('Type')} />
      </FormRow>
    {/if}
    <FormRow label={t('Label')} for={id('label')}>
      <TextField id={id('label')} bind:value={connection.label} placeholder={autoLabel} />
    </FormRow>
    {#if web}
      <FormRow
        label={t('Scheme')}
        for={id('scheme')}
        hint={t('Paste a full URL into these fields to fill them.')}
      >
        <TextField
          id={id('scheme')}
          bind:value={connection.webScheme}
          placeholder="https"
          onpaste={pasted}
          class="short"
        />
      </FormRow>
    {/if}
    <FormRow label={t('Port')} for={id('port')} error={portError}>
      <TextField
        id={id('port')}
        bind:value={connection.port}
        placeholder={portPlaceholder}
        inputmode="numeric"
        invalid={!!portError}
        oninput={() => onportinput?.()}
        onpaste={web ? pasted : undefined}
        class="short"
      />
    </FormRow>
    {#if web}
      <FormRow label={t('Path')} for={id('path')}>
        <TextField id={id('path')} bind:value={connection.webPath} placeholder="/admin" onpaste={pasted} />
      </FormRow>
      {#if hostOffer}
        <FormRow>
          <p class="offer">
            {t('The URL is on %@, this entry on %@.', hostOffer, entryHost.trim())}
            <Button onclick={() => onusehost(hostOffer!)}>{t('Use %@', hostOffer)}</Button>
          </p>
        </FormRow>
      {/if}
      <FormRow label={t('URL')}>
        <span class="readonly url"
          >{webUrlPreview(connection.webScheme, entryHost, connection.port, connection.webPath)}</span
        >
      </FormRow>
      <FormRow label={t('Browser')}>
        <Select
          value={toPicker(connection.browserBundleId)}
          label={t('Browser')}
          options={browserOptions(connection.savedBrowserBundleId, deps.browsers.installed)}
          onchange={(v) => (connection.browserBundleId = fromPicker(v))}
        />
      </FormRow>
    {/if}
    {#if connection.type === 'mikrotik'}
      <FormRow label={t('Default mode')}>
        <Select
          bind:value={connection.mikrotikMode}
          label={t('Default mode')}
          options={MIKROTIK_MODES.map((m) => ({ value: m, label: mikrotikModeName(m) }))}
        />
      </FormRow>
    {/if}
    {#if ssh}
      <FormRow label={t('SSH terminal')}>
        <Select bind:value={connection.sshTerminal} label={t('SSH terminal')} options={terminalOptions} />
      </FormRow>
    {/if}
    {#if connection.type === 'rdp'}
      <FormRow label={t('RDP client')}>
        <Select bind:value={connection.rdpClient} label={t('RDP client')} options={rdpOptions} />
      </FormRow>
      <FormRow>
        <Toggle label={t('Admin session (/admin)')} bind:checked={connection.rdpAdminSession} />
      </FormRow>
    {/if}
    <FormRow>
      <Toggle label={t('Show as button on Details')} bind:checked={connection.showAsButton} />
    </FormRow>
  </FormSection>

  <FormSection title={t('Authentication')}>
    <FormRow label={t('Credential')}>
      <Select
        value={toPicker(connection.credentialId)}
        label={t('Credential')}
        options={credentialOptions(library)}
        onchange={(v) => (connection.credentialId = fromPicker(v))}
      />
    </FormRow>
    {#if credential}
      {#if credential.username === ''}
        <FormRow
          label={t('Username')}
          for={id('user')}
          hint={t('The credential has no username – this connection uses its own.')}
        >
          <TextField id={id('user')} bind:value={connection.username} />
        </FormRow>
        {#if connection.type === 'rdp'}
          <FormRow label={t('Domain')} for={id('domain')}>
            <TextField id={id('domain')} bind:value={connection.domain} placeholder={t('optional')} />
          </FormRow>
        {/if}
      {:else}
        <FormRow label={t('User')}><span class="readonly">{credential.displayUser}</span></FormRow>
      {/if}
      <FormRow label={t('Password')}>
        <span class="readonly">{credential.hasPassword ? MASKED : EMPTY}</span>
      </FormRow>
      {#if ssh}
        <FormRow label={t('SSH key')}><span class="readonly">{credential.sshKeyPath ?? EMPTY}</span></FormRow>
      {/if}
    {:else}
      <FormRow label={t('Username')} for={id('user')}>
        <TextField id={id('user')} bind:value={connection.username} />
      </FormRow>
      {#if connection.type === 'rdp'}
        <FormRow label={t('Domain')} for={id('domain')}>
          <TextField id={id('domain')} bind:value={connection.domain} placeholder={t('optional')} />
        </FormRow>
      {/if}
      <FormRow label={t('Password')} for={id('password')}>
        <SecretInput id={id('password')} label={t('Password')} bind:value={connection.password} />
      </FormRow>
      {#if ssh}
        <FormRow label={t('SSH key')} for={id('key')}>
          <SshKeyField id={id('key')} bind:value={connection.sshKeyPath} name={entryName} />
        </FormRow>
        {#if connection.sshKeyPath.trim() !== ''}
          <FormRow label={t('Key passphrase')} for={id('passphrase')}>
            <SecretInput
              id={id('passphrase')}
              label={t('Key passphrase')}
              bind:value={connection.passphrase}
            />
          </FormRow>
        {/if}
      {/if}
    {/if}
  </FormSection>

  <div class="connection-actions">
    <Button disabled={isDefault} onclick={onsetdefault}>{t('Set as default')}</Button>
    <Button variant="destructive" onclick={onremove}>{t('Remove this connection')}</Button>
  </div>
</div>

<style>
  .fields :global(.short) {
    max-width: 100px;
  }
  .readonly {
    color: var(--fg-secondary);
  }
  .url {
    overflow-wrap: anywhere;
  }
  .offer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    color: var(--fg-secondary);
  }
  .connection-actions {
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
  }
</style>
