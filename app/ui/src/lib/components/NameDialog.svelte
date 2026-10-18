<script lang="ts">
  import { t } from '../i18n/index.svelte';
  import Button from './Button.svelte';
  import Modal from './Modal.svelte';
  import TextField from './TextField.svelte';

  /** A one-field name sheet (Swift `SavedSplitNameSheet`, the folder name): Enter submits, blank is refused. */
  interface Props {
    title: string;
    /** Placeholder and accessible name of the field. */
    label: string;
    initial?: string;
    confirmLabel: string;
    onsubmit: (name: string) => void;
    oncancel: () => void;
  }

  let { title, label, initial = '', confirmLabel, onsubmit, oncancel }: Props = $props();
  // svelte-ignore state_referenced_locally
  let name = $state(initial);
  const blank = $derived(name.trim() === '');

  function submit() {
    if (!blank) onsubmit(name.trim());
  }
</script>

<Modal {title} onclose={oncancel} width={340}>
  <TextField bind:value={name} placeholder={label} aria-label={label} onsubmit={submit} />
  {#snippet footer()}
    <Button onclick={oncancel}>{t('Cancel')}</Button>
    <Button variant="primary" disabled={blank} onclick={submit}>{confirmLabel}</Button>
  {/snippet}
</Modal>
