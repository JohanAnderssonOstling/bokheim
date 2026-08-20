<script lang="ts">
  import SquareButton from "./buttons/SquareButton.svelte";

  let {
    label,
    value,
    decreaseLabel,
    increaseLabel,
    decreaseDisabled = false,
    increaseDisabled = false,
    ondecrease,
    onincrease,
  }: {
    label: string;
    value: string;
    decreaseLabel: string;
    increaseLabel: string;
    decreaseDisabled?: boolean;
    increaseDisabled?: boolean;
    ondecrease: () => void;
    onincrease: () => void;
  } = $props();
</script>

<div class="setting-stepper-row">
  <span>{label}</span>
  <div class="stepper">
    <SquareButton
      label={decreaseLabel}
      disabled={decreaseDisabled}
      onclick={ondecrease}
    >
      −
    </SquareButton>
    <output>{value}</output>
    <SquareButton
      label={increaseLabel}
      disabled={increaseDisabled}
      onclick={onincrease}
    >
      +
    </SquareButton>
  </div>
</div>

<style>
  .setting-stepper-row {
    display: grid;
    grid-template-columns: minmax(6rem, 1fr) auto;
    align-items: center;
    gap: 0.75rem;
  }

  .setting-stepper-row > span {
    color: var(--color-subtle-text);
    font-size: 0.75rem;
    font-weight: 750;
  }

  .stepper {
    display: grid;
    grid-template-columns: 1.5rem 6.75rem 1.5rem;
    align-items: center;
  }

  output {
    min-height: 1.5rem;
    display: flex;
    align-items: center;
    justify-content: center;
    border-block: 1px solid var(--color-border);
    color: var(--color-subtle-text);
    background: var(--color-raised-bg);
    font-size: 0.75rem;
    font-weight: 700;
    white-space: nowrap;
  }
</style>
