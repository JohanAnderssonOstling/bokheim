<!--
  Owns which tab is showing and nothing else. The panels arrive as snippets, so
  their contents are written where the data lives and only rendered here — this
  component never learns what a chapter or an annotation is.
-->
<script>
  // The tab is held by the reader rather than here, because the top bar opens
  // the sidebar onto a particular panel.
  let { contents, annotationList, bookmarkList, tab, onTab, hidden = false } = $props();
</script>

<nav
  class="sidebar"
  class:hidden
>
  <div class="tabs">
    <button
      class:active={tab === "contents"}
      onclick={() => onTab("contents")}>Contents</button
    >
    <button
      class:active={tab === "annotations"}
      onclick={() => onTab("annotations")}>Annotations</button
    >
    <button
      class:active={tab === "bookmarks"}
      onclick={() => onTab("bookmarks")}>Bookmarks</button
    >
  </div>

  <div class="panel">
    {#if tab === "contents"}
      {@render contents()}
    {:else if tab === "annotations"}
      {@render annotationList()}
    {:else}
      {@render bookmarkList()}
    {/if}
  </div>
</nav>

<style>
  /* Hidden rather than unmounted, so scroll position survives a toggle. */
  .sidebar.hidden {
    display: none;
  }

  /* An overlay over the book rather than a column beside it, matching the v1
     reader. Wide layouts inset the text to compensate; narrow ones do not, so
     on a phone this covers the page instead of squeezing it. */
  .sidebar {
    position: absolute;
    z-index: 6;
    inset-block: 0;
    inset-inline-start: 0;
    display: flex;
    flex-direction: column;
    width: min(380px, 40vw);
    background: var(--color-surface-bg);
    border-inline-end: 1px solid var(--color-border);
  }

  .tabs {
    display: flex;
    flex: none;
  }

  .tabs button {
    flex: 1;
    padding: 0.5rem;
    background: none;
    border: 0;
    border-bottom: 2px solid transparent;
    color: inherit;
    cursor: pointer;
  }

  .tabs button.active {
    border-bottom-color: var(--color-ink-text);
  }

  /* The panel scrolls, not the sidebar, so the tabs stay put. */
  .panel {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
</style>
