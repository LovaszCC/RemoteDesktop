<script lang="ts">
  import { untrack } from 'svelte';
  import Button from '../components/Button.svelte';
  import ColorPicker from '../components/ColorPicker.svelte';
  import Modal from '../components/Modal.svelte';
  import Segmented from '../components/Segmented.svelte';
  import Select from '../components/Select.svelte';
  import TextField from '../components/TextField.svelte';
  import Toggle from '../components/Toggle.svelte';
  import { toCommandError } from '../api';
  import { viewDeps } from '../detail/deps';
  import { EMPTY, MASKED, mikrotikModeName, rdpClientName } from '../detail/model';
  import { errorMessage } from '../errors';
  import { t } from '../i18n/index.svelte';
  import { CONNECTION_TYPES, NEW_TITLES, typeName, usesSSH } from '../sidebar/model';
  import type { ConnectionEditorRequest } from '../stores/editor.svelte';
  import type { MikrotikMode, RdpClient, SshTerminal } from '../types';
  import FormRow from './FormRow.svelte';
  import FormSection from './FormSection.svelte';
  import {
    DEFAULT_PORTS,
    browserOptions,
    buildConnectionSave,
    canSave,
    connectionFormFrom,
    credentialOptions,
    errorField,
    folderOptions,
    fromPicker,
    newConnectionForm,
    toPicker,
    type DraftField,
  } from './model';
  import SecretInput from './SecretInput.svelte';
  import SshKeyField from './SshKeyField.svelte';

  /** Swift `ConnectionEditorView`. */
  let { request, onclose }: { request: ConnectionEditorRequest; onclose: () => void } = $props();
  const deps = viewDeps();
  const uid = Math.random().toString(36).slice(2);
  const id = (name: string) => `conn-${name}-${uid}`;

  const isNew = untrack(() => request.mode === 'new');
  const editedId = untrack(() => (request.mode === 'edit' ? request.id : null));
  const existing = untrack(() => (editedId ? deps.library.connection(editedId) : undefined));
  let form = $state(
    untrack(() =>
      request.mode === 'new'
        ? newConnectionForm(request.type, request.folderId)
        : connectionFormFrom(existing!),
    ),
  );
  let errors = $state<Partial<Record<DraftField, string>>>({});
  let failure = $state<string | null>(null);
  let saving = $state(false);

  const library = $derived(deps.library.library);
  const credential = $derived(deps.library.credential(form.credentialId));
  const ssh = $derived(usesSSH(form.type));

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

  $effect(() => {
    if (form.type === 'web') void deps.browsers.load();
  });

  // The edited connection was deleted meanwhile.
  $effect(() => {
    if (editedId && !deps.library.connection(editedId)) onclose();
  });

  function edited(field: DraftField) {
    if (errors[field]) errors = { ...errors, [field]: undefined };
  }

  /** Return in a one-line field saves (the sheet's default action). */
  function submitOnEnter(event: KeyboardEvent) {
    if (event.key === 'Enter' && event.target instanceof HTMLInputElement && !event.isComposing) {
      event.preventDefault();
      void save();
    }
  }

  async function save() {
    if (!canSave(form.name) || saving || deps.readOnly) return;
    saving = true;
    errors = {};
    failure = null;
    const { draft, password, passphrase } = buildConnectionSave(form);
    try {
      const savedId = await deps.api.saveConnection(editedId, draft, password, passphrase);
      onclose();
      if (isNew) deps.library.selectConnection(savedId);
    } catch (error) {
      const field = errorField(toCommandError(error).code);
      if (field) errors = { [field]: errorMessage(error) };
      else failure = errorMessage(error);
    } finally {
      saving = false;
    }
  }
</script>

<Modal title={isNew ? t(NEW_TITLES[form.type]) : t('Edit connection')} {onclose} width={500}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <form
    class="editor"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
    onkeydown={submitOnEnter}
  >
    <FormSection>
      {#if isNew}
        <FormRow label={t('Type')}>
          <Segmented bind:value={form.type} options={typeOptions} label={t('Type')} />
        </FormRow>
      {/if}
      <FormRow label={t('Name')} for={id('name')} error={errors.name}>
        <TextField
          id={id('name')}
          bind:value={form.name}
          placeholder={t('e.g. web01 – prod')}
          invalid={!!errors.name}
          oninput={() => edited('name')}
          autofocus
        />
      </FormRow>
      <FormRow label={t('Folder')}>
        <Select
          value={toPicker(form.folderId)}
          label={t('Folder')}
          options={folderOptions(library)}
          onchange={(v) => (form.folderId = fromPicker(v))}
        />
      </FormRow>
      <FormRow label={t('Color')} hint={t("Also the color of the connection's tabs")}>
        <ColorPicker bind:value={form.color} />
      </FormRow>
      <FormRow>
        <Toggle label={t('Favorite')} bind:checked={form.isFavorite} />
      </FormRow>
    </FormSection>

    <FormSection title={t('Connection')}>
      {#if form.type === 'web'}
        <FormRow label={t('URL')} for={id('host')} error={errors.host}>
          <TextField
            id={id('host')}
            bind:value={form.host}
            placeholder="https://router.local/admin"
            invalid={!!errors.host}
            oninput={() => edited('host')}
          />
        </FormRow>
        <FormRow label={t('Browser')}>
          <Select
            value={toPicker(form.browserBundleId)}
            label={t('Browser')}
            options={browserOptions(existing?.browserBundleId ?? null, deps.browsers.installed)}
            onchange={(v) => (form.browserBundleId = fromPicker(v))}
          />
        </FormRow>
      {:else}
        <FormRow label={t('Host / IP')} for={id('host')} error={errors.host}>
          <TextField
            id={id('host')}
            bind:value={form.host}
            placeholder="server.local"
            invalid={!!errors.host}
            oninput={() => edited('host')}
          />
        </FormRow>
        <FormRow label={t('Port')} for={id('port')} error={errors.port}>
          <TextField
            id={id('port')}
            bind:value={form.port}
            placeholder={String(DEFAULT_PORTS[form.type])}
            inputmode="numeric"
            invalid={!!errors.port}
            oninput={() => edited('port')}
            class="port"
          />
        </FormRow>
      {/if}
      {#if form.type === 'mikrotik'}
        <FormRow label={t('Default mode')}>
          <Select
            bind:value={form.mikrotikMode}
            label={t('Default mode')}
            options={MIKROTIK_MODES.map((m) => ({ value: m, label: mikrotikModeName(m) }))}
          />
        </FormRow>
      {/if}
      {#if ssh}
        <FormRow label={t('SSH terminal')}>
          <Select bind:value={form.sshTerminal} label={t('SSH terminal')} options={terminalOptions} />
        </FormRow>
      {/if}
      {#if form.type === 'rdp'}
        <FormRow label={t('RDP client')}>
          <Select bind:value={form.rdpClient} label={t('RDP client')} options={rdpOptions} />
        </FormRow>
        <FormRow>
          <Toggle label={t('Admin session (/admin)')} bind:checked={form.rdpAdminSession} />
        </FormRow>
      {/if}
    </FormSection>

    <FormSection title={t('Authentication')}>
      <FormRow label={t('Credential')}>
        <Select
          value={toPicker(form.credentialId)}
          label={t('Credential')}
          options={credentialOptions(library)}
          onchange={(v) => (form.credentialId = fromPicker(v))}
        />
      </FormRow>
      {#if credential}
        {#if credential.username === ''}
          <FormRow
            label={t('Username')}
            for={id('user')}
            hint={t('The credential has no username – this connection uses its own.')}
          >
            <TextField id={id('user')} bind:value={form.username} />
          </FormRow>
          {#if form.type === 'rdp'}
            <FormRow label={t('Domain')} for={id('domain')}>
              <TextField id={id('domain')} bind:value={form.domain} placeholder={t('optional')} />
            </FormRow>
          {/if}
        {:else}
          <FormRow label={t('User')}><span class="readonly">{credential.displayUser}</span></FormRow>
        {/if}
        <FormRow label={t('Password')}>
          <span class="readonly">{credential.hasPassword ? MASKED : EMPTY}</span>
        </FormRow>
        {#if ssh}
          <FormRow label={t('SSH key')}
            ><span class="readonly">{credential.sshKeyPath ?? EMPTY}</span></FormRow
          >
        {/if}
      {:else}
        <FormRow label={t('Username')} for={id('user')}>
          <TextField id={id('user')} bind:value={form.username} />
        </FormRow>
        {#if form.type === 'rdp'}
          <FormRow label={t('Domain')} for={id('domain')}>
            <TextField id={id('domain')} bind:value={form.domain} placeholder={t('optional')} />
          </FormRow>
        {/if}
        <FormRow label={t('Password')} for={id('password')}>
          <SecretInput id={id('password')} label={t('Password')} bind:value={form.password} />
        </FormRow>
        {#if ssh}
          <FormRow label={t('SSH key')} for={id('key')}>
            <SshKeyField id={id('key')} bind:value={form.sshKeyPath} name={form.name} />
          </FormRow>
          {#if form.sshKeyPath.trim() !== ''}
            <FormRow label={t('Key passphrase')} for={id('passphrase')}>
              <SecretInput id={id('passphrase')} label={t('Key passphrase')} bind:value={form.passphrase} />
            </FormRow>
          {/if}
        {/if}
      {/if}
    </FormSection>

    <FormSection title={t('Notes')}>
      <textarea bind:value={form.notes} aria-label={t('Notes')} rows="3"></textarea>
    </FormSection>
  </form>
  {#snippet footer()}
    {#if failure}<p class="failure" role="alert">{failure}</p>{/if}
    <Button onclick={onclose}>{t('Cancel')}</Button>
    <Button variant="primary" disabled={!canSave(form.name) || saving || deps.readOnly} onclick={save}>
      {isNew ? t('Create') : t('Save')}
    </Button>
  {/snippet}
</Modal>

<style>
  .editor :global(.port) {
    max-width: 100px;
  }
  .readonly {
    color: var(--fg-secondary);
  }
  textarea {
    width: 100%;
    margin: 6px 0;
    padding: 4px 7px;
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius-small);
    background: var(--bg-control);
    resize: vertical;
    outline: none;
  }
  textarea:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus-ring);
  }
  .failure {
    flex: 1;
    margin: 0;
    color: var(--danger);
  }
</style>
