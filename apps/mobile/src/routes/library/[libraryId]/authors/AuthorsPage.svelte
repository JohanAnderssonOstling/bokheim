<script lang="ts">
  import AuthorRow from "./AuthorRow.svelte";
  import BookGrid from "../../../../components/shared/books/BookGrid.svelte";
  import EmptyPage from "../../../../components/shell/EmptyPage.svelte";
  import SearchField from "../../../../components/shared/SearchField.svelte";
  import type { AuthorsPageProps } from "./authors-page";
  let {
    libraryId,
    authors,
    filter = $bindable(),
    focusedIndex = $bindable(),
    selectedAuthorId,
    selectedAuthorName,
    books,
    detailed,
    openingBookId,
    downloadStates,
    onfilterkeydown,
    onauthorkeydown,
    onselectauthor,
    onclear,
    onadd,
    onopen,
    onopenbook,
    ondownload,
    onremove,
    ontrash,
  }: AuthorsPageProps = $props();
  const filteredAuthors = $derived.by(() => {
    const query = filter.trim().toLocaleLowerCase();
    return query
      ? authors.filter((author) => author.name.toLocaleLowerCase().includes(query))
      : authors;
  });
</script>

<section
  class="authors-layout"
  aria-label="Authors"
>
  {#if !selectedAuthorId}<div class="author-list-panel">
      <SearchField
        class="author-filter"
        bind:value={filter}
        id="author-filter"
        placeholder="Filter authors"
        ariaLabel="Filter authors"
        onkeydown={onfilterkeydown}
      />
      {#if filteredAuthors.length}<div class="author-list-scroll">
          <div class="author-list">
            {#each filteredAuthors as author, index (libraryId + ":" + author.id + ":" + author.cover_hashes.join(","))}<AuthorRow
                {author}
                {libraryId}
                authorIndex={index}
                active={selectedAuthorId === author.id}
                tabindex={index === focusedIndex ? 0 : -1}
                onfocus={() => (focusedIndex = index)}
                onkeydown={(event) => onauthorkeydown(event, index)}
                onselect={() => {
                  focusedIndex = index;
                  onselectauthor(author.id, author.name);
                }}
                {onopenbook}
              />{/each}
          </div>
        </div>
      {:else if filter.trim()}<div class="empty">No matching authors.</div>
      {:else}<EmptyPage
          title="No authors"
          primaryLabel="Add book"
          onprimary={onadd}
        />{/if}
    </div>{:else}
    <div class="author-books-panel">
      <div class="heading">
        <button
          class="back"
          type="button"
          onclick={onclear}>← <span>Authors</span></button
        >
        <h2>{selectedAuthorName}</h2>
      </div>
      <BookGrid
        {libraryId}
        {books}
        {detailed}
        emptyMessage="No books found for this author."
        {openingBookId}
        {downloadStates}
        {onopen}
        {ondownload}
        {onremove}
        {ontrash}
      />
    </div>
  {/if}
</section>

<style>
  .authors-layout {
    width: min(1248px, 100%);
    min-height: 0;
    height: 100%;
    margin: 0 auto;
  }
  .author-list-panel,
  .author-books-panel {
    min-width: 0;
    min-height: 0;
  }
  .author-list-panel {
    display: flex;
    height: 100%;
    flex-direction: column;
  }
  .author-books-panel {
    height: 100%;
    overflow-y: auto;
    overscroll-behavior-y: contain;
    scrollbar-gutter: stable;
  }
  :global(.author-filter) {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 9px;
    min-height: 40px;
    margin-bottom: 14px;
    padding: 0 14px;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-raised-bg);
  }
  :global(.author-filter .file-icon) {
    color: var(--color-accent);
  }
  :global(.author-filter .search-input) {
    min-width: 0;
    border: 0;
    outline: 0;
    color: inherit;
    background: transparent;
    font: inherit;
  }
  .back {
    min-height: 40px;
    padding: 0 14px;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: var(--color-accent);
    background: var(--color-raised-bg);
    font: inherit;
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .author-list-scroll {
    min-height: 0;
    flex: 1;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .author-list {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--book-grid-column-gap);
    padding-bottom: var(--book-grid-column-gap);
  }
  .heading {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 18px;
  }
  h2 {
    margin: 0;
    font-size: 1.3125rem;
    font-weight: 600;
  }
  .empty {
    padding: 28px;
    color: var(--color-subtle-text);
    text-align: center;
  }
</style>
