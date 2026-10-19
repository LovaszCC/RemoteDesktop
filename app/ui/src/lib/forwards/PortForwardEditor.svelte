<script lang="ts">
  // Swift `PortForwardEditor`: Name, Local port, Remote host, Remote port; Cancel / Save / Save & Start,
  // with Rust's validation message inline.
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import Select from '../components/Select.svelte';
  import TextField from '../components/TextField.svelte';
  import FormRow from '../editors/FormRow.svelte';
  import { errorMessage } from '../errors';
  import { t } from '../i18n/index.svelte';
  import type { PortForwardDraft } from '../types';
  import type { ForwardStore } from './forwards.svelte';

  interface Props {
    draft: PortForwardDraft;
    /** The SSH connections to choose from; empty (one connection) hides the picker. */
    connections?: readonly { value: string; label: string }[];
    forwards: ForwardStore;
    onclose: () => void;
    /** Start errors after saving (shown as an alert by the caller). */
    onstarterror?: (error: unknown) => void;
  }

  let { draft, connections = [], forwards, onclose, onstarterror }: Props = $props();
  // svelte-ignore state_referenced_locally
  let fields = $state({ ...draft });
  let error = $state<string | null>(null);
  let busy = $state(false);
  const uid = Math.random().toString(36).slice(2);

  async function save(start: boolean) {
    if (busy) return;
    busy = true;
    error = null;
    let id: string;
    try {
      id = await forwards.save({ ...fields, name: fields.name.trim() }, false);
    } catch (e) {
      error = errorMessage(e);
      busy = false;
      return;
    }
    onclose();
    if (start) await forwards.startForward(id).catch((e: unknown) => onstarterror?.(e));
  }
</script>

<Modal title={draft.id ? t('Edit port forward') : t('New port forward')} {onclose} width={460}>
  {#if connections.length > 1}
    <FormRow label={t('Connection')}>
      <Select bind:value={fields.connectionId} options={connections} label={t('Connection')} />
    </FormRow>
  {/if}
  <FormRow label={t('Name')} for="pf-name-{uid}">
    <TextField id="pf-name-{uid}" bind:value={fields.name} placeholder={t('Optional')} />
  </FormRow>
  <FormRow label={t('Local port')} for="pf-local-{uid}">
    <TextField
      id="pf-local-{uid}"
      bind:value={fields.localPort}
      placeholder="13389"
      inputmode="numeric"
      onsubmit={() => void save(true)}
    />
  </FormRow>
  <FormRow label={t('Remote host')} for="pf-host-{uid}">
    <TextField
      id="pf-host-{uid}"
      bind:value={fields.remoteHost}
      placeholder="192.0.2.20"
      onsubmit={() => void save(true)}
    />
  </FormRow>
  <FormRow
    label={t('Remote port')}
    for="pf-remote-{uid}"
    {error}
    hint={t(
      'Connections to localhost on the local port are forwarded through the SSH server to the remote host and port.',
    )}
  >
    <TextField
      id="pf-remote-{uid}"
      bind:value={fields.remotePort}
      placeholder="3389"
      inputmode="numeric"
      onsubmit={() => void save(true)}
    />
  </FormRow>
  {#snippet footer()}
    <Button onclick={onclose}>{t('Cancel')}</Button>
    <Button disabled={busy} onclick={() => void save(false)}>{t('Save')}</Button>
    <Button variant="primary" disabled={busy} onclick={() => void save(true)}>{t('Save & Start')}</Button>
  {/snippet}
</Modal>
