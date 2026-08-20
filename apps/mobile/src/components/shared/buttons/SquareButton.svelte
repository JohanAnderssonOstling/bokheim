<script lang="ts">
  let {
    label,
    active = false,
    disabled = false,
    ariaExpanded,
    ariaHasPopup,
    onclick,
  }: {
    label: string;
    active?: boolean;
    disabled?: boolean;
    ariaExpanded?: boolean;
    ariaHasPopup?: "menu" | "listbox" | "tree" | "grid" | "dialog";
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<button
  class="square-button"
  class:active
  type="button"
  aria-label={label}
  aria-expanded={ariaExpanded}
  aria-haspopup={ariaHasPopup}
  {disabled}
  {onclick}
>
  <slot />
</button>

<style>
  .square-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.5rem;
    height: 1.5rem;
    padding: 0;
    box-sizing: border-box;
    border: 1px solid var(--color-border, var(--chrome-strong));
    background: var(--color-surface-bg, var(--chrome));
    font: inherit;
    font-size: 1rem;
    line-height: 1;
  }

  .square-button:active,
  .square-button.active {
    background: var(--color-border, var(--chrome-strong));
  }

  .square-button:disabled {
    opacity: 0.4;
  }
</style>
