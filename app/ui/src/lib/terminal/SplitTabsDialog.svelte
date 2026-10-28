<script lang="ts">
  // "Split tabs…": ticks 2–4 of the open terminal tabs to put side by side in one split tab. Once four are
  // ticked the others can't be; "Split" needs two.
  import Button from '../components/Button.svelte';
  import Checkbox from '../components/Checkbox.svelte';
  import Modal from '../components/Modal.svelte';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import type { TabSession } from '../tabs.svelte';
  import { MAX_PANES } from './split';

  interface Props {
    /** The tabs to choose from, in tab bar order. */
    candidates: readonly TabSession[];
    /** A tab's shown title. */
    titleOf: (session: TabSession) => string;
    /** Ticked when it opens (the active tab). */
    initial?: readonly string[];
    onconfirm: (ids: string[]) => void;
    oncancel: () => void;
  }

  let { candidates, titleOf, initial = [], onconfirm, oncancel }: Props = $props();
  // Seeded once from the props: the dialog owns the ticks while it is open.
  // svelte-ignore state_referenced_locally
  let chosen = $state<string[]>(initial.filter((id) => candidates.some((c) => c.id === id)));

  const full = $derived(chosen.length >= MAX_PANES);

  function toggle(id: string, checked: boolean) {
    chosen = checked ? [...chosen, id] : chosen.filter((x) => x !== id);
  }
</script>

<Modal title={t('Split tabs')} onclose={oncancel} width={380}>
  <p class="hint">{t('Choose 2–4 terminal tabs to show side by side.')}</p>
  <ul class="list">
    {#each candidates as session (session.id)}
      {@const checked = chosen.includes(session.id)}
      <li>
        <Checkbox {checked} disabled={!checked && full} onchange={(on) => toggle(session.id, on)}>
          <span class="row">
            <ConnectionIcon type={session.type} size={14} />
            <span class="title">{titleOf(session)}</span>
          </span>
        </Checkbox>
      </li>
    {/each}
  </ul>
  {#snippet footer()}
    <Button onclick={oncancel}>{t('Cancel')}</Button>
    <Button variant="primary" disabled={chosen.length < 2} onclick={() => onconfirm(chosen)}>
      {t('Split')}
    </Button>
  {/snippet}
</Modal>

<style>
  .hint {
    margin: 0 0 var(--space-2);
    color: var(--fg-secondary);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .row {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .title {
    max-width: 280px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
