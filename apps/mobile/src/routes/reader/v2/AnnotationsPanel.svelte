<!-- Lists the reader's annotations and reports what was picked. Holds nothing. -->
<script>
  let { items, onSelect, onDelete } = $props();
</script>

{#if items.length}
  <div class="annotations">
    {#each items as annotation (annotation.id)}
      <div class="annotation">
        <button
          class="excerpt"
          style="border-inline-start-color: {annotation.color}"
          onclick={() => onSelect(annotation)}
        >
          <span class="text">{annotation.exact_text}</span>
          {#if annotation.note}<span class="note">{annotation.note}</span>{/if}
        </button>
        <button
          class="remove"
          aria-label="Delete annotation"
          onclick={() => onDelete(annotation)}>×</button
        >
      </div>
    {/each}
  </div>
{:else}
  <p class="empty">Highlight some text to make an annotation.</p>
{/if}

<style>
  .annotations {
    display: flex;
    flex-direction: column;
  }

  .annotation {
    display: flex;
    align-items: start;
    border-top: 1px solid var(--color-border);
  }

  .excerpt {
    display: grid;
    flex: 1;
    gap: 0.25rem;
    min-width: 0;
    padding: 0.5rem;
    text-align: start;
    background: none;
    border: 0;
    border-inline-start: 4px solid transparent;
    color: inherit;
    cursor: pointer;
  }

  .text {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
  }

  .note {
    color: var(--color-subtle-text);
    font-size: 0.8125rem;
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
