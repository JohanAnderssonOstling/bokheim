<!-- Lists the reader's bookmarks and reports what was picked. Holds nothing:
     the current-page state lives in the reader, which is what can tell a
     bookmarked page from one that isn't. -->
<script>
  let { bookmarks, bookmarked = false, onAdd, onSelect } = $props();
</script>

<button
  class="add"
  type="button"
  disabled={bookmarked}
  onclick={onAdd}
>
  {bookmarked ? "This page is bookmarked" : "Bookmark this page"}
</button>

{#if bookmarks.items.length}
  <div class="bookmarks">
    {#each bookmarks.items as bookmark (bookmark.id)}
      <div class="bookmark">
        <button
          class="label"
          onclick={() => onSelect(bookmark)}
        >
          {bookmark.label}
        </button>
        <button
          class="remove"
          aria-label="Delete bookmark"
          onclick={() => bookmarks.remove(bookmark)}>×</button
        >
      </div>
    {/each}
  </div>
{:else}
  <p class="empty">No bookmarks yet.</p>
{/if}

<style>
  .add {
    width: 100%;
    padding: 0.75rem;
    text-align: start;
    background: none;
    border: 0;
    border-bottom: 1px solid var(--color-border);
    color: inherit;
    cursor: pointer;
  }

  .add:disabled {
    color: var(--color-subtle-text);
    cursor: default;
  }

  .bookmarks {
    display: flex;
    flex-direction: column;
  }

  .bookmark {
    display: flex;
    align-items: start;
    border-top: 1px solid var(--color-border);
  }

  .label {
    flex: 1;
    min-width: 0;
    padding: 0.5rem;
    text-align: start;
    background: none;
    border: 0;
    color: inherit;
    cursor: pointer;
  }

  .remove {
    padding: 0.5rem;
    background: none;
    border: 0;
    color: var(--color-subtle-text);
    cursor: pointer;
  }

  .empty {
    padding: 1rem;
  }
</style>
