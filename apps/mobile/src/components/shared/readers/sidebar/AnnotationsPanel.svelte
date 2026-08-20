<script lang="ts">
  import SearchField from "../../SearchField.svelte";
  import SquareButton from "../../buttons/SquareButton.svelte";
  import type { ReaderAnnotation } from "$lib/shared/types.ts";
  import { activateReaderAnnotation, readerAnnotations } from "$lib/reader/marks";
  import { deleteAnnotation } from "$lib/reader/annotations";

  let {
    annotations = [],
    error = "",
    onActivate,
  }: {
    annotations?: ReaderAnnotation[];
    error?: string;
    onActivate?: (annotation: ReaderAnnotation) => void;
  } = $props();
  let filter = $state("");
  let deleted = $state<Set<string>>(new Set());
  let annotationItems = $derived(readerAnnotations() ?? annotations);
  const visible = $derived(
    annotationItems.filter(
      (annotation) =>
        !deleted.has(annotation.id) &&
        `${annotation.exact_text} ${annotation.note}`
          .toLocaleLowerCase()
          .includes(filter.toLocaleLowerCase()),
    ),
  );

  async function remove(id: string) {
    await deleteAnnotation(id);
    deleted = new Set([...deleted, id]);
  }
</script>

<SearchField
  class="reader-sidebar-filter"
  value={filter}
  placeholder="Filter annotations"
  ariaLabel="Filter annotations"
  oninput={(value) => (filter = value)}
/>

{#if error}
  <p
    class="panel-error"
    role="alert"
  >{error}</p>
{/if}
<div class="annotation-list">
  {#each visible as annotation (annotation.id)}
    <article>
      <button
        class="annotation-target"
        type="button"
        onclick={() =>
          void (readerAnnotations()
            ? activateReaderAnnotation(annotation.id)
            : onActivate?.(annotation))}
      >
        <span
          class="annotation-color"
          style={`--annotation-color: ${annotation.color}`}
        ></span>
        <q>{annotation.exact_text}</q>
        {#if annotation.note}<span>{annotation.note}</span>{/if}
      </button>
      <SquareButton
        label="Delete annotation"
        onclick={() => void remove(annotation.id)}
      >×</SquareButton>
    </article>
  {/each}
</div>

<style>
  :global(.reader-sidebar-filter),
  :global(.reader-sidebar-filter .search-input) {
    width: 100%;
    min-height: 2.5rem;
  }

  .annotation-list {
    display: grid;
    gap: 0.4rem;
    max-height: 42dvh;
    overflow: auto;
  }

  article {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 0.25rem;
    border: 1px solid var(--chrome-strong);
  }

  .annotation-target {
    display: grid;
    grid-template-columns: 0.35rem minmax(0, 1fr);
    gap: 0.25rem 0.5rem;
    text-align: start;
  }

  .annotation-color {
    grid-row: 1 / span 2;
    background: var(--annotation-color);
  }

  .annotation-target span:last-child {
    grid-column: 2;
    font-size: 0.76rem;
    opacity: 0.72;
  }

  .panel-error {
    color: var(--color-danger-text);
  }
</style>
