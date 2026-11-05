<script lang="ts">
  // The serial settings: baud rate (common values offered, any number typed), and under "Advanced" data
  // bits, parity, stop bits and flow control. Shared by the connect, "Settings…" and entry editor forms.
  import Select from '../components/Select.svelte';
  import TextField from '../components/TextField.svelte';
  import FormRow from '../editors/FormRow.svelte';
  import { t } from '../i18n/index.svelte';
  import {
    BAUD_RATES,
    DATA_BITS,
    FLOW_CONTROLS,
    PARITIES,
    STOP_BITS,
    flowControlName,
    parityName,
    type SerialForm,
  } from './serial';

  interface Props {
    form: SerialForm;
    /** Prefix of the field ids (several forms may be on screen). */
    idPrefix?: string;
    /** The baud field's error, if any. */
    baudError?: string | null;
  }

  let { form = $bindable(), idPrefix = 'serial', baudError = null }: Props = $props();
  const custom = $derived(!BAUD_RATES.includes(Number(form.baud.trim())));
</script>

<FormRow label={t('Baud rate')} for="{idPrefix}-baud" error={baudError}>
  <div class="baud">
    <TextField
      id="{idPrefix}-baud"
      bind:value={form.baud}
      list="{idPrefix}-baud-rates"
      inputmode="numeric"
      invalid={baudError !== null}
    />
    <datalist id="{idPrefix}-baud-rates">
      {#each BAUD_RATES as rate (rate)}<option value={String(rate)}></option>{/each}
    </datalist>
    {#if custom && baudError === null}<span class="note">{t('custom')}</span>{/if}
  </div>
</FormRow>
<details class="advanced">
  <summary>{t('Advanced')}</summary>
  <FormRow label={t('Data bits')} for="{idPrefix}-data-bits">
    <Select
      label={t('Data bits')}
      value={String(form.dataBits)}
      options={DATA_BITS.map((bits) => ({ value: String(bits), label: String(bits) }))}
      onchange={(bits) => (form.dataBits = Number(bits))}
    />
  </FormRow>
  <FormRow label={t('Parity')}>
    <Select
      label={t('Parity')}
      bind:value={form.parity}
      options={PARITIES.map((parity) => ({ value: parity, label: parityName(parity) }))}
    />
  </FormRow>
  <FormRow label={t('Stop bits')}>
    <Select
      label={t('Stop bits')}
      bind:value={form.stopBits}
      options={STOP_BITS.map((bits) => ({ value: bits, label: bits }))}
    />
  </FormRow>
  <FormRow label={t('Flow control')}>
    <Select
      label={t('Flow control')}
      bind:value={form.flowControl}
      options={FLOW_CONTROLS.map((flow) => ({ value: flow, label: flowControlName(flow) }))}
    />
  </FormRow>
</details>

<style>
  .baud {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .note {
    color: var(--fg-secondary);
  }
  .advanced summary {
    margin: var(--space-1) 0;
    cursor: default;
    color: var(--fg-secondary);
  }
</style>
