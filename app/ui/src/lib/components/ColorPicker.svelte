<script lang="ts">
  import Ban from '@lucide/svelte/icons/ban';
  import { LABEL_COLOR_MAP, nextColor } from '../colors';
  import { t } from '../i18n/index.svelte';
  import { colorTitle } from '../sidebar/model';
  import { LABEL_COLORS, type LabelColor } from '../types';

  /** Swift `LabelColorPicker`: 8 circles (clicking the selected one again clears it) and "No color". */
  interface Props {
    value?: LabelColor | null;
    disabled?: boolean;
    onchange?: (color: LabelColor | null) => void;
  }

  let { value = $bindable(null), disabled = false, onchange }: Props = $props();

  function choose(color: LabelColor | null) {
    value = color;
    onchange?.(color);
  }
</script>

<div class="color-picker" role="radiogroup" aria-label={t('Color')}>
  {#each LABEL_COLORS as color (color)}
    <button
      type="button"
      class="swatch"
      class:selected={value === color}
      role="radio"
      aria-checked={value === color}
      aria-label={colorTitle(color)}
      title={colorTitle(color)}
      style:--swatch={LABEL_COLOR_MAP[color]}
      {disabled}
      onclick={() => choose(nextColor(value, color))}
    ></button>
  {/each}
  <button
    type="button"
    class="none"
    class:selected={value === null}
    role="radio"
    aria-checked={value === null}
    aria-label={t('No color')}
    title={t('No color')}
    {disabled}
    onclick={() => choose(null)}
  >
    <Ban size={15} strokeWidth={1.75} />
  </button>
</div>

<style>
  .color-picker {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 3px 0;
  }
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: none;
    background: none;
  }
  button:disabled {
    opacity: 0.45;
  }
  .swatch {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--swatch);
  }
  .swatch.selected {
    box-shadow:
      0 0 0 2px var(--bg-elevated),
      0 0 0 4px var(--fg);
  }
  .none {
    color: var(--fg-tertiary);
  }
  .none.selected {
    color: var(--fg);
  }
</style>
