<script lang="ts">
  import SquareButton from "../../buttons/SquareButton.svelte";

  let {
    title,
    closeLabel = "Close",
    onclose,
    variant = "default",
  }: {
    title: string;
    closeLabel?: string;
    onclose: () => void;
    variant?: "default" | "bordered" | "sheet";
  } = $props();
</script>

<div
  class="panel-heading"
  class:bordered={variant === "bordered"}
  class:sheet={variant === "sheet"}
>
  <strong>{title}</strong>
  <SquareButton
    label={closeLabel}
    onclick={onclose}
  >
    ×
  </SquareButton>
</div>

<style>
  .panel-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 2.5rem;
  }

  .panel-heading strong {
    font-family: var(--font-interface-family);
  }

  .panel-heading.bordered {
    min-height: 48px;
    padding: 0 8px 0 14px;
    border-bottom: 1px solid var(--color-border);
  }

  .panel-heading.sheet {
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--chrome-strong);
  }
</style>
