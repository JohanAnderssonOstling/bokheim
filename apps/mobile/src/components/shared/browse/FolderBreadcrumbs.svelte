<script lang="ts">
  import { BROWSING_CONTENT_CONTRACT } from "../../../lib/shared/types";

  let {
    rootLabel = "Library",
    levels = [],
    onselect,
  }: {
    rootLabel?: string;
    levels?: { name: string }[];
    onselect: (level: number) => void | Promise<void>;
  } = $props();

  function visibleLabel(label: string) {
    const characters = Array.from(label);
    const limit = BROWSING_CONTENT_CONTRACT.breadcrumb_item_max_characters;
    return characters.length > limit
      ? `${characters.slice(0, Math.max(0, limit - 1)).join("")}…`
      : label;
  }
</script>

<nav aria-label="Folder breadcrumbs">
  <button
    type="button"
    class:current={!levels.length}
    title={rootLabel}
    onclick={() => onselect(0)}>{visibleLabel(rootLabel)}</button
  >
  {#each levels as level, index}
    <span aria-hidden="true">/</span>
    <button
      type="button"
      class:current={index === levels.length - 1}
      title={level.name}
      onclick={() => onselect(index + 1)}>{visibleLabel(level.name)}</button
    >
  {/each}
</nav>

<style>
  nav {
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: start;
    gap: 5px;
    min-width: 0;
    min-height: 40px;
    overflow-x: auto;
    color: var(--color-border);
    white-space: nowrap;
    scrollbar-width: none;
  }
  nav::-webkit-scrollbar {
    display: none;
  }
  button {
    flex: none;
    min-height: 40px;
    padding: 5px;
    border: 0;
    color: var(--color-subtle-text);
    background: none;
    font: inherit;
    font-size: 0.8125rem;
    cursor: pointer;
  }
  button.current {
    color: var(--color-ink-text);
    font-weight: 700;
  }
</style>
