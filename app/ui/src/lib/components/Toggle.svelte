<script lang="ts">
  interface Props {
    checked?: boolean;
    disabled?: boolean;
    label: string;
    onchange?: (checked: boolean) => void;
  }

  let { checked = $bindable(false), disabled = false, label, onchange }: Props = $props();

  function flip() {
    checked = !checked;
    onchange?.(checked);
  }
</script>

<label class="toggle" class:disabled>
  <span class="label">{label}</span>
  <button type="button" role="switch" aria-checked={checked} aria-label={label} {disabled} onclick={flip}>
    <span class="knob"></span>
  </button>
</label>

<style>
  .toggle {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  button {
    position: relative;
    width: 30px;
    height: 18px;
    padding: 0;
    border: none;
    border-radius: 9px;
    background: var(--bg-selected);
    transition: background 0.15s;
  }
  button[aria-checked='true'] {
    background: var(--accent);
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
    transition: transform 0.15s;
  }
  button[aria-checked='true'] .knob {
    transform: translateX(12px);
  }
  .disabled {
    opacity: 0.5;
  }
</style>
