<!--
  Renders a table of contents and reports what was picked.

  Which entries are expanded is held here: nothing outside this panel needs to
  know, and it is presentation rather than reader state.

  Measurements match the v1 reader's contents panel: 2.75rem rows, a 2rem
  disclosure column, a 1rem-per-level indent, and the hover tint painted as an
  overlay so it composes over the current-entry background rather than
  replacing it.
-->
<script>
  import caretRight from "@icons/phosphor/caret-right.svg";
  import FileIcon from "../../../components/shared/FileIcon.svelte";
  import {
    initialExpandedKeys,
    toggleExpandedKey,
    tocPanelItems,
  } from "../../../components/shared/readers/sidebar/toc-panel-items";

  let { items, activeHref = "", onSelect } = $props();

  /// Null means "not touched yet", which shows the top level opened.
  let expanded = $state(null);

  let keys = $derived(expanded ?? initialExpandedKeys(items));
  let visible = $derived(tocPanelItems(items, keys));

  function toggle(key) {
    expanded = toggleExpandedKey(keys, key);
  }
</script>

{#if visible.length}
  <nav class="reader-toc-list">
    {#each visible as item (item.key)}
      <div
        class="reader-toc-row"
        class:current={item.data.href === activeHref}
        style={`--reader-toc-depth: ${item.depth}`}
      >
        {#if item.hasChildren}
          <button
            class="reader-toc-disclosure"
            type="button"
            aria-label={`${item.expanded ? "Collapse" : "Expand"} ${item.label}`}
            aria-expanded={item.expanded}
            onclick={() => toggle(item.key)}
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
          disabled={!item.data.href}
          onclick={() => onSelect(item.data.href)}
        >
          {item.label}
        </button>
      </div>
    {/each}
  </nav>
{:else}
  <p class="empty">This book has no table of contents.</p>
{/if}

<style>
  .reader-toc-list {
    display: grid;
    gap: 0.2rem;
  }

  .reader-toc-row {
    display: grid;
    grid-template-columns: 2rem minmax(0, 1fr);
    align-items: center;
    min-height: 2.75rem;
    padding-inline-start: calc(var(--reader-toc-depth) * 1rem);
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
    background-color: color-mix(in srgb, var(--color-border) 22%, transparent);
  }

  /* Nothing resets button chrome globally, so a control that does not state its
     own would render with the browser's default bevelled box. */
  .reader-toc-disclosure {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 2rem;
    min-height: 2.5rem;
    padding: 0;
    color: var(--color-subtle-text);
    background: transparent;
    border: 0;
    cursor: pointer;
    -webkit-tap-highlight-color: transparent;
  }

  .reader-toc-disclosure:hover {
    color: var(--color-ink-text);
  }

  .reader-toc-caret {
    display: inline-flex;
  }

  .reader-toc-caret.expanded {
    rotate: 90deg;
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
    white-space: normal;
    font: inherit;
    font-weight: 600;
    color: inherit;
    background: transparent;
    border: 0;
    cursor: pointer;
    -webkit-tap-highlight-color: transparent;
  }

  .reader-toc-target:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .empty {
    padding: 1rem;
  }
</style>
