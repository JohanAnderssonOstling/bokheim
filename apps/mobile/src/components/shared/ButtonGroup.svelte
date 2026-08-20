<script lang="ts">
  type ButtonOption = {
    value: string;
    label: string;
    disabled?: boolean;
  };

  let {
    options,
    value,
    onSelect,
    ariaLabel,
    width = "auto",
    height = "2rem",
    buttonWidth = undefined,
  }: {
    options: readonly ButtonOption[];
    value: string;
    ariaLabel?: string;
    onSelect: (nextValue: string) => void;
    width?: string;
    height?: string;
    /** Fixed width for every option. The group grows to fit its buttons. */
    buttonWidth?: string;
  } = $props();
</script>

<div
  class="button-group"
  class:fixed-button-width={buttonWidth !== undefined}
  style={`width: ${buttonWidth === undefined ? width : "auto"}; height: ${height}; --button-group-button-width: ${buttonWidth ?? "auto"};`}
  aria-label={ariaLabel}
>
  {#each options as option}
    {#key option.value}
      <button
        type="button"
        class="button"
        class:active={value === option.value}
        aria-pressed={value === option.value}
        disabled={option.disabled}
        onclick={() => onSelect(option.value)}
      >
        {option.label}
      </button>
    {/key}
  {/each}
</div>

<style>
  .button-group {
    display: flex;
    flex: none;
    flex-wrap: nowrap;
    overflow: hidden;
    border: 1px solid var(--color-border);
    background: var(--color-border);
  }

  .button-group button {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    min-width: 0;
    height: 100%;
    box-sizing: border-box;
    border: 0;
    border-radius: 0;

    color: var(--color-ink-text);
    background: var(--color-raised-bg);
    font-family: var(--font-interface-family, Georgia, "Times New Roman", serif);
    font-size: 1rem;
    font-weight: 600;
    cursor: pointer;
  }

  .button-group button + button {
    border-inline-start: 1px solid var(--color-border);
  }

  .button-group.fixed-button-width button {
    flex: 0 0 var(--button-group-button-width);
    width: var(--button-group-button-width);
  }

  .button-group button.active {
    color: var(--color-raised-bg);
    background: var(--color-accent);
  }

  .button-group button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
