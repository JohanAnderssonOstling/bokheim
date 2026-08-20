<script lang="ts">
  import SquareButton from "../../../../../components/shared/buttons/SquareButton.svelte";
  import SecondaryButton from "../../../../../components/shared/buttons/SecondaryButton.svelte";

  let {
    selectionText,
    onHighlight,
    onNote,
    onDismiss,
  }: {
    selectionText: string;
    onHighlight: () => void;
    onNote: () => void;
    onDismiss: () => void;
  } = $props();
</script>

{#if selectionText}
  <div
    class="selection-actions"
    role="toolbar"
    aria-label="Selected PDF text"
  >
    <q>{selectionText}</q>
    <SecondaryButton onclick={onHighlight}>Highlight</SecondaryButton>
    <SecondaryButton onclick={onNote}>Note</SecondaryButton>
    <SquareButton
      label="Dismiss selected text actions"
      onclick={onDismiss}>×</SquareButton
    >
  </div>
{/if}

<style>
  .selection-actions {
    position: fixed;
    z-index: 24;
    top: calc(var(--floating-overlay-top) + 8px);
    left: 50%;
    display: flex;
    width: min(560px, calc(100vw - 24px));
    align-items: center;
    gap: 6px;
    padding: 6px;
    box-sizing: border-box;
    border: 1px solid var(--color-border);
    background: var(--color-surface-bg);
    box-shadow: 0 10px 28px color-mix(in srgb, black 24%, transparent);
    transform: translateX(-50%);
  }

  .selection-actions q {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.78rem;
  }
</style>
