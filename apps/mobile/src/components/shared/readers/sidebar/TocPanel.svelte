<script lang="ts">
  import caretRight from "@icons/phosphor/caret-right.svg";
  import FileIcon from "../../FileIcon.svelte";
  import type { ReaderTocPanelItem } from "./toc-panel-items.ts";

  let {
    items,
    onActivate,
    onToggle,
    activeItemKey = "",
  }: {
    items: ReaderTocPanelItem[];
    onActivate: (item: ReaderTocPanelItem) => void | Promise<void>;
    onToggle?: (item: ReaderTocPanelItem) => void;
    activeItemKey?: string;
  } = $props();

  const hasChildren = (item: ReaderTocPanelItem) =>
    item.hasChildren === true && typeof onToggle === "function";
  const toggleLabel = (item: ReaderTocPanelItem) =>
    `${item.expanded ? "Collapse" : "Expand"} ${item.label}`;
</script>

{#if items.length}
  <nav class="reader-toc-list">
    {#each items as item (item.key)}
      <div
        class:current={item.key === activeItemKey}
        class="reader-toc-row"
        style={`--reader-toc-depth: ${item.depth}`}
      >
        {#if hasChildren(item)}
          <button
            class="reader-toc-disclosure"
            type="button"
            aria-label={toggleLabel(item)}
            aria-expanded={item.expanded}
            onclick={() => onToggle?.(item)}
          >
            <span
              class="reader-toc-caret"
              class:expanded={item.expanded}
            >
              <FileIcon
                source={caretRight}
                size="0.7rem"
              />
            </span>
          </button>
        {:else}
          <span
            class="reader-toc-disclosure-spacer"
            aria-hidden="true"
          ></span>
        {/if}
        <button
          class="reader-toc-target"
          type="button"
          disabled={item.disabled}
          onclick={() => void onActivate(item)}
        >
          {item.label}
        </button>
      </div>
    {/each}
  </nav>
{/if}

<style>
  .reader-toc-list {
    display: grid;
    gap: 0.2rem;
    padding: var(--reader-toc-padding, 0);
  }

  .reader-toc-row {
    display: grid;
    grid-template-columns: 2rem minmax(0, 1fr);
    align-items: center;
    min-height: 2.75rem;
    padding-inline-start: calc(var(--reader-toc-depth) * 1rem);
    border-radius: 0;
  }

  /* An entry the reader can act on lights up as one unit, caret included.
     Painted as an overlay rather than a background swap so it composes over
     the current-entry tint instead of replacing it. */
  .reader-toc-row:hover:not(:has(.reader-toc-target:disabled)) {
    background-image: linear-gradient(
      var(--color-hover-overlay),
      var(--color-hover-overlay)
    );
  }

  .reader-toc-row.current {
    background-color: var(
      --reader-toc-current-bg,
      color-mix(in srgb, var(--color-border) 22%, transparent)
    );
  }

  /* Nothing resets button chrome globally, so a disclosure that does not state
     its own would render with the browser's default bevelled box. */
  .reader-toc-disclosure {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 2rem;
    min-height: 2.5rem;
    padding: 0;
    border: 0;
    border-radius: 0;
    color: var(--color-ink-muted);
    background: transparent;
    cursor: pointer;
    -webkit-tap-highlight-color: transparent;
  }

  .reader-toc-disclosure:hover {
    color: var(--color-ink-text);
  }

  .reader-toc-caret {
    display: inline-flex;
    transition: transform 120ms ease;
  }

  .reader-toc-caret.expanded {
    transform: rotate(90deg);
  }

  @media (prefers-reduced-motion: reduce) {
    .reader-toc-caret {
      transition: none;
    }
  }

  .reader-toc-disclosure-spacer {
    width: 2rem;
  }

  .reader-toc-target {
    justify-content: flex-start;
    min-width: 0;
    min-height: 2.75rem;
    padding: 0.55rem 0.7rem 0.55rem 0.2rem;
    text-align: start;
    font-weight: 600;
    white-space: normal;
    border: 0;
    background: transparent;
    cursor: pointer;
    -webkit-tap-highlight-color: transparent;
  }

  .reader-toc-target:disabled {
    opacity: 0.55;
    cursor: default;
  }
</style>
