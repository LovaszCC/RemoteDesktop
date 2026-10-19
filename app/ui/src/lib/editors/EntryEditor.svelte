<script lang="ts">
  import { tick, untrack } from 'svelte';
  import Button from '../components/Button.svelte';
  import ColorPicker from '../components/ColorPicker.svelte';
  import Modal from '../components/Modal.svelte';
  import Segmented from '../components/Segmented.svelte';
  import Select from '../components/Select.svelte';
  import Tabs from '../components/Tabs.svelte';
  import TextField from '../components/TextField.svelte';
  import Toggle from '../components/Toggle.svelte';
  import { toCommandError } from '../api';
  import { viewDeps } from '../detail/deps';
  import { errorMessage } from '../errors';
  import { t } from '../i18n/index.svelte';
  import { CONNECTION_TYPES, typeName } from '../sidebar/model';
  import type { EntryEditorRequest } from '../stores/editor.svelte';
  import type { ConnectionType } from '../types';
  import ConnectionFields from './ConnectionFields.svelte';
  import FormRow from './FormRow.svelte';
  import FormSection from './FormSection.svelte';
  import {
    buildEntrySave,
    canSave,
    connectionTabLabel,
    entryFormFrom,
    errorField,
    folderOptions,
    fromPicker,
    invalidPortKey,
    newConnectionForm,
    newEntryForm,
    splitEntryHostUrl,
    toPicker,
  } from './model';

  /**
   * The entry editor (design option B): a General tab with the shared fields, one tab per connection
   * (the default one marked with the check), and a "+" tab that adds a connection of the picked type.
   * One Save sends everything (`save_entry`).
   */
  let { request, onclose }: { request: EntryEditorRequest; onclose: () => void } = $props();
  const deps = viewDeps();
  const uid = Math.random().toString(36).slice(2);
  const id = (name: string) => `entry-${name}-${uid}`;
  const GENERAL = 'general';
  const ADD = 'add';

  const isNew = untrack(() => request.mode === 'new');
  const editedId = untrack(() => (request.mode === 'edit' ? request.id : null));
  const editedEntry = untrack(() => (editedId === null ? undefined : deps.library.entry(editedId)));
  // An unknown entry (deleted meanwhile, a stale id) gets a blank form; the effect below closes the sheet.
  let form = $state(
    untrack(() =>
      request.mode === 'new'
        ? newEntryForm(request.type, request.folderId)
        : editedEntry
          ? entryFormFrom(editedEntry, deps.library.connectionsOf(request.id))
          : newEntryForm('ssh', null),
    ),
  );
  let selected = $state(
    untrack(() =>
      request.mode === 'edit' && request.addConnection
        ? ADD
        : request.mode === 'edit' &&
            request.connectionId &&
            form.connections.some((c) => c.key === request.connectionId)
          ? request.connectionId
          : GENERAL,
    ),
  );
  let errors = $state<{ name?: string; host?: string; port?: { key: string; message: string } }>({});
  let failure = $state<string | null>(null);
  let saving = $state(false);
  /** Per connection key: the host of a pasted URL that differs from the entry's. */
  let hostOffers = $state<Record<string, string>>({});

  const library = $derived(deps.library.library);
  const typeOptions = $derived(CONNECTION_TYPES.map((type) => ({ value: type, label: typeName(type) })));
  const tabs = $derived([
    { id: GENERAL, label: t('General') },
    ...form.connections.map((c) => ({
      id: c.key,
      label: connectionTabLabel(c, deps.library.credential(c.credentialId)),
      marked: c.key === form.defaultKey,
    })),
    { id: ADD, label: '+', title: t('Add connection') },
  ]);

  $effect(() => {
    if (form.connections.some((c) => c.type === 'web')) void deps.browsers.load();
  });

  // The edited entry was deleted meanwhile.
  $effect(() => {
    if (editedId && !deps.library.entry(editedId)) onclose();
  });

  async function focus(tab: string, field: string) {
    selected = tab;
    await tick();
    document.getElementById(field)?.focus();
  }

  function addConnection(type: ConnectionType) {
    const connection = newConnectionForm(type);
    form.connections.push(connection);
    form.defaultKey ??= connection.key;
    selected = connection.key;
  }

  function removeConnection(key: string) {
    const index = form.connections.findIndex((c) => c.key === key);
    if (index < 0) return;
    form.connections.splice(index, 1);
    if (form.defaultKey === key) form.defaultKey = form.connections[0]?.key ?? null;
    if (errors.port?.key === key) errors = { ...errors, port: undefined };
    hostOffers = withoutKey(hostOffers, key);
    selected = form.connections[Math.min(index, form.connections.length - 1)]?.key ?? GENERAL;
  }

  function withoutKey(record: Record<string, string>, key: string): Record<string, string> {
    return Object.fromEntries(Object.entries(record).filter(([k]) => k !== key));
  }

  /** The host of a URL pasted into connection `key`: fills an empty entry host, else offers it. */
  function pastedHost(key: string, host: string) {
    const current = form.host.trim();
    const offers = withoutKey(hostOffers, key);
    if (current === '') {
      form.host = host;
      edited('host');
    } else if (current.toLowerCase() !== host.toLowerCase()) {
      offers[key] = host;
    }
    hostOffers = offers;
  }

  function useHost(host: string) {
    form.host = host;
    edited('host');
    hostOffers = {};
  }

  /** A full URL pasted into the General host of an entry with a web connection is split. */
  function pastedIntoHost(event: ClipboardEvent) {
    const text = event.clipboardData?.getData('text/plain') ?? '';
    if (!splitEntryHostUrl(form, text)) return;
    event.preventDefault();
    edited('host');
    edited('port');
    hostOffers = {};
  }

  function edited(field: 'name' | 'host' | 'port') {
    if (errors[field]) errors = { ...errors, [field]: undefined };
  }

  /** Return in a one-line field saves (the sheet's default action). */
  function submitOnEnter(event: KeyboardEvent) {
    if (event.key === 'Enter' && event.target instanceof HTMLInputElement && !event.isComposing) {
      event.preventDefault();
      void save();
    }
  }

  /** Shows a Rust validation error where it belongs: General's name or host, a connection's port, else
   * the footer. */
  function showError(error: unknown) {
    const message = errorMessage(error);
    const field = errorField(toCommandError(error).code);
    if (field === 'name' || field === 'host') {
      errors = { [field]: message };
      void focus(GENERAL, id(field));
      return;
    }
    const key =
      field === 'port'
        ? (invalidPortKey(form) ??
          form.connections.find((c) => c.port.trim() !== '')?.key ??
          form.connections[0]?.key)
        : undefined;
    if (key) {
      errors = { port: { key, message } };
      selected = key;
    } else {
      failure = message;
    }
  }

  async function save() {
    if (!canSave(form.name) || saving || deps.readOnly) return;
    saving = true;
    errors = {};
    failure = null;
    // A typed full URL in the host is split the same way as a pasted one.
    if (splitEntryHostUrl(form, form.host)) hostOffers = {};
    const { entry, connections } = buildEntrySave(form);
    try {
      const savedId = await deps.api.saveEntry(editedId, entry, connections);
      onclose();
      if (isNew) deps.library.select(`e:${savedId}`);
    } catch (error) {
      showError(error);
    } finally {
      saving = false;
    }
  }
</script>

<Modal title={isNew ? t('New entry') : t('Edit entry')} {onclose} width={540}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <form
    class="editor"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
    onkeydown={submitOnEnter}
  >
    <Tabs
      items={tabs}
      {selected}
      onselect={(tab) => (selected = tab)}
      label={t('Connections')}
      controls={id('panel')}
      idPrefix={id('tab')}
    />
    <div class="panel" role="tabpanel" id={id('panel')} aria-labelledby={`${id('tab')}-${selected}`}>
      {#if selected === GENERAL}
        <FormSection>
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
          <FormRow label={t('Host / IP')} for={id('host')} error={errors.host}>
            <TextField
              id={id('host')}
              bind:value={form.host}
              placeholder="server.local"
              invalid={!!errors.host}
              oninput={() => edited('host')}
              onpaste={pastedIntoHost}
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
        <FormSection title={t('Notes')}>
          <textarea bind:value={form.notes} aria-label={t('Notes')} rows="3"></textarea>
        </FormSection>
      {:else if selected === ADD}
        <FormSection title={t('Add connection')}>
          <FormRow label={t('Type')}>
            <Segmented
              value={'' as ConnectionType | ''}
              options={typeOptions}
              label={t('Type')}
              onchange={(type) => type && addConnection(type)}
            />
          </FormRow>
        </FormSection>
      {:else}
        {#each form.connections as connection, index (connection.key)}
          {#if connection.key === selected}
            <ConnectionFields
              bind:connection={() => connection, (value) => (form.connections[index] = value)}
              entryName={form.name}
              entryHost={form.host}
              isDefault={connection.key === form.defaultKey}
              portError={errors.port?.key === connection.key ? errors.port.message : undefined}
              hostOffer={hostOffers[connection.key] ?? null}
              onpastehost={(host) => pastedHost(connection.key, host)}
              onusehost={useHost}
              onsetdefault={() => (form.defaultKey = connection.key)}
              canremove={form.connections.length > 1}
              onremove={() => removeConnection(connection.key)}
              onportinput={() => edited('port')}
            />
          {/if}
        {/each}
      {/if}
    </div>
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
  .editor {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
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
