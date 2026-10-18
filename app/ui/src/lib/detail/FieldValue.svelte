<script lang="ts">
  import Copy from '@lucide/svelte/icons/copy';
  import IconButton from '../components/IconButton.svelte';
  import { t } from '../i18n/index.svelte';
  import type { DetailField } from './model';

  /** One grid row: the label and the value with its copy button. */
  let { field, oncopy }: { field: DetailField; oncopy: (text: string) => void } = $props();
</script>

<span class="label">{t(field.label)}</span>
<span class="value">
  <span class="selectable">{field.value}</span>
  {#if field.copy}
    {@const text = field.copy}
    <IconButton icon={Copy} size={12} label={t('Copy')} onclick={() => oncopy(text)} />
  {/if}
</span>
