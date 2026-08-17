<script lang="ts">
  import AuthorRow from "./AuthorRow.svelte";
  import AuthorCatalogResults from "./AuthorCatalogResults.svelte";
  import { isWebApp } from "../../lib/platform";
  import BookGrid from "./BookGrid.svelte";
  import EmptyPage from "../shell/EmptyPage.svelte";
  import type { AuthorSummary, BookCardData, DownloadState } from "../../lib/types";
  let { libraryId, authors, filter = $bindable(), focusedIndex = $bindable(), selectedAuthorId, selectedAuthorName, books, detailed, openingBookId, downloadStates, onfilterkeydown, onauthorkeydown, onselectauthor, onclear, onadd, onimported, onopen, onopenbook, ondownload, onremove, ontrash }: {
    libraryId: string; authors: AuthorSummary[]; filter: string; focusedIndex: number; selectedAuthorId: string; selectedAuthorName: string; books: BookCardData[]; detailed: boolean; openingBookId: string;
    downloadStates: Record<string, DownloadState>; onfilterkeydown: (event: KeyboardEvent) => void;
    onauthorkeydown: (event: KeyboardEvent, index: number) => void; onselectauthor: (id: string, name: string) => void | Promise<void>; onclear: () => void; onadd: () => void | Promise<void>; onimported: () => void | Promise<void>; onopenbook: (contentHash: string) => void | Promise<void>;
    onopen: (book: BookCardData) => void | Promise<void>; ondownload: (book: BookCardData) => void | Promise<void>; onremove: (book: BookCardData) => void | Promise<void>; ontrash: (book: BookCardData) => void | Promise<void>;
  } = $props();
</script>
<section class="authors-layout" aria-label="Authors">
  {#if !selectedAuthorId}<div class="author-list-panel">
    <label class="author-filter"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0"></path></svg><input id="author-filter" type="search" bind:value={filter} onkeydown={onfilterkeydown} autocomplete="off" aria-label="Filter authors" /></label>
    {#if authors.length}<div class="author-list-scroll"><div class="author-list">{#each authors as author, index (libraryId + ":" + author.id + ":" + author.cover_hashes.join(","))}<AuthorRow {author} {libraryId} authorIndex={index} active={selectedAuthorId === author.id} tabindex={index === focusedIndex ? 0 : -1} onfocus={() => (focusedIndex = index)} onkeydown={(event) => onauthorkeydown(event, index)} onselect={() => { focusedIndex = index; onselectauthor(author.id, author.name); }} {onopenbook} />{/each}</div></div>
    {:else if filter.trim()}<div class="empty">No matching authors.</div>
    {:else}<EmptyPage title="No authors" primaryLabel="Add book" onprimary={onadd} />{/if}
  </div>{:else}
  <div class="author-books-panel"><div class="heading"><button class="back" type="button" onclick={onclear}>← <span>Authors</span></button><h2>{selectedAuthorName}</h2></div><BookGrid {libraryId} {books} {detailed} emptyMessage="No books found for this author." {openingBookId} {downloadStates} {onopen} {ondownload} {onremove} {ontrash} />{#if !isWebApp}{#key selectedAuthorId}<AuthorCatalogResults authorId={selectedAuthorId} authorName={selectedAuthorName} activeLibraryId={libraryId} {onimported} />{/key}{/if}</div>
  {/if}
</section>
<style>
  .authors-layout { width: min(1248px, 100%); min-height: 0; height: 100%; margin: 0 auto; }
  .author-list-panel, .author-books-panel { min-width: 0; min-height: 0; } .author-list-panel { display: flex; height: 100%; flex-direction: column; } .author-filter { display: grid; grid-template-columns: auto minmax(0,1fr); align-items: center; gap: 9px; min-height: 40px; margin-bottom: 14px; padding: 0 14px; border: 1px solid var(--color-border); border-radius: 0; background: var(--color-raised-bg); }
  .author-filter svg { width: 1.05rem; fill: none; stroke: var(--color-accent); stroke-width: 1.8; } .author-filter input { min-width: 0; border: 0; outline: 0; color: inherit; background: transparent; font: inherit; }
  .back { min-height: 40px; padding: 0 14px; border: 1px solid var(--color-border); border-radius: 0; color: var(--color-accent); background: var(--color-raised-bg); font: inherit; font-size: .8125rem; font-weight: 600; }
  .author-list-scroll { min-height: 0; flex: 1; overflow-y: auto; overscroll-behavior: contain; } .author-list { display: flex; flex-wrap: wrap; align-items: flex-start; gap: var(--book-grid-column-gap); padding-bottom: var(--book-grid-column-gap); } .heading { display: flex; align-items: center; gap: 14px; margin-bottom: 18px; } h2 { margin: 0; font-size: 1.3125rem; font-weight: 600; } .empty { padding: 28px; color: var(--color-subtle-text); text-align: center; }
</style>
