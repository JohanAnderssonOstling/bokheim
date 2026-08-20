<script lang="ts">
  import MenuItemButton from "../../../../components/shared/buttons/MenuItemButton.svelte";
  let {
    selectionText,
    position,
    onHighlight,
    onCopyCitation,
  }: {
    selectionText: string;
    position: { x: number; y: number } | null;
    onHighlight: () => void | Promise<void>;
    onCopyCitation: () => void | Promise<void>;
  } = $props();
</script>

{#if position && selectionText}
  <div
    class="selection-context-menu"
    role="menu"
    aria-label="Selected text actions"
    style={`left: ${position.x}px; top: ${position.y}px;`}
  >
    <MenuItemButton onclick={() => void onHighlight()}>Highlight</MenuItemButton>
    <MenuItemButton onclick={() => void onCopyCitation()}>Cite</MenuItemButton>
  </div>
{/if}

<style>
  .selection-context-menu {
    position: fixed;
    z-index: 20;
    display: grid;
    min-width: 162px;
    padding: 6px;
    border: 1px solid var(--chrome-strong);
    border-radius: 0;
    color: var(--ink);
    background: var(--chrome);
    box-shadow: 0 10px 32px color-mix(in srgb, var(--color-ink-text) 26%, transparent);
  }
</style>
