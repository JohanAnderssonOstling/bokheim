<script lang="ts">
  import type { Snippet } from "svelte";
  import BookGrid from "./BookGrid.svelte";
  import BrowseNavigationGrid from "./BrowseNavigationGrid.svelte";
  import ContextualLibrarySearch from "./ContextualLibrarySearch.svelte";
  import FolderNavigationCard from "./FolderNavigationCard.svelte";
  import FileTypeFilter from "./FileTypeFilter.svelte";
  import EmptyPage from "../shell/EmptyPage.svelte";
  import CatalogPublicationCard from "../store/CatalogPublicationCard.svelte";
  import MetadataReviewPanel from "./MetadataReviewPanel.svelte";
  import { commands } from "../../lib/commands";
  import { catalogActionKey, primaryCatalogAction } from "../../lib/catalog-actions";
  import { ROOT_DIRECTORY_ID } from "../../lib/directory-id";
  import { openExternalUrl } from "../../lib/external-links";
  import { BROWSING_CONTENT_CONTRACT, STORE_CONTRACT, type BookCardData, type DownloadState, type LibraryDirectory, type LibraryFileTypeFilter } from "../../lib/types";
  import type { CatalogPublicationAction, CatalogRecommendation, CatalogRecommendationResponse } from "../../lib/bindings";
  let { libraryId, subjects, books, subjectPath, fileType, loading, query = $bindable(), detailed, openingBookId, downloadStates, searchResults, onsearch, onfiletype, onopensubject, onimported, onopen, ondownload, onremove, ontrash }: {
    libraryId: string; subjects: LibraryDirectory[]; books: BookCardData[]; subjectPath: { id: string; name: string }[]; loading: boolean; query: string; detailed: boolean;
    fileType: LibraryFileTypeFilter; openingBookId: string; downloadStates: Record<string, DownloadState>; searchResults: Snippet; onsearch: () => void; onfiletype: (value: LibraryFileTypeFilter) => void | Promise<void>; onimported: () => void | Promise<void>;
    onopensubject: (subject: LibraryDirectory) => void | Promise<void>; onopen: (book: BookCardData) => void | Promise<void>; ondownload: (book: BookCardData) => void | Promise<void>; onremove: (book: BookCardData) => void | Promise<void>; ontrash: (book: BookCardData) => void | Promise<void>;
  } = $props();
  const placeholder = $derived(subjectPath.length ? `Search books or authors in ${subjectPath.at(-1)?.name} and subsubjects` : "Search books or authors in all subjects");
  const selectedTypeLabel = $derived(BROWSING_CONTENT_CONTRACT.file_type_filters.find((option) => option.value === fileType)?.label ?? "books");
  let recommendations = $state<CatalogRecommendationResponse | null>(null);
  let recommendationsLoading = $state(false);
  let recommendationsError = $state("");
  let actionStates = $state<Record<string, "idle" | "adding" | "added">>({});
  let actionErrors = $state<Record<string, string>>({});
  let recommendationGeneration = 0;

  function message(reason: unknown) { return reason instanceof Error ? reason.message : String(reason); }
  function recommendationKey(item: CatalogRecommendation) { return `${item.source_id}:${item.publication.identity.edition_id}:${item.publication.identity.work_id ?? ""}`; }

  async function loadRecommendations(library: string, path: string | null) {
    const generation = ++recommendationGeneration;
    recommendationsLoading = true;
    recommendationsError = "";
    try {
      const result = await commands.getCatalogSubjectRecommendations(library, path, 12);
      if (generation === recommendationGeneration) recommendations = result;
    } catch (reason) {
      if (generation === recommendationGeneration) recommendationsError = message(reason);
    } finally {
      if (generation === recommendationGeneration) recommendationsLoading = false;
    }
  }

  async function runRecommendationAction(item: CatalogRecommendation, action: CatalogPublicationAction) {
    const actionKey = catalogActionKey(action);
    if (actionStates[actionKey] === "adding") return;
    actionErrors = { ...actionErrors, [actionKey]: "" };
    if (action.kind === "external") {
      try { await openExternalUrl(action.url); } catch (reason) { actionErrors = { ...actionErrors, [actionKey]: message(reason) }; }
      return;
    }
    actionStates = { ...actionStates, [actionKey]: "adding" };
    try {
      await commands.importCatalogBook({ library_id: libraryId, directory_id: ROOT_DIRECTORY_ID }, action.request);
      actionStates = { ...actionStates, [actionKey]: "added" };
      recommendations = recommendations ? { recommendations: recommendations.recommendations.filter((candidate) => recommendationKey(candidate) !== recommendationKey(item)) } : null;
      await onimported();
    } catch (reason) {
      actionStates = { ...actionStates, [actionKey]: "idle" };
      actionErrors = { ...actionErrors, [actionKey]: message(reason) };
    }
  }

  $effect(() => {
    const path = subjectPath.at(-1)?.id ?? null;
    if (libraryId) void loadRecommendations(libraryId, path);
  });
</script>
<section class="browse-page" aria-label="Subjects" style={`--store-publication-min-width: ${STORE_CONTRACT.publication_min_width_px}px; --store-publication-height: ${STORE_CONTRACT.publication_height_px}px; --store-scrollbar-width: ${STORE_CONTRACT.description_scrollbar_width_px}px;`}>
  <ContextualLibrarySearch bind:value={query} {placeholder} oninput={onsearch} />
  <FileTypeFilter value={fileType} onchange={onfiletype} />
  {#if query.trim()}{@render searchResults()}
  {:else if loading}<div class="state" aria-live="polite">Loading subjects…</div>
  {:else}
    {#if subjects.length}<BrowseNavigationGrid>{#each subjects as subject}<FolderNavigationCard kind="subject" flat name={subject.name} count={subject.book_count} onclick={() => onopensubject(subject)} />{/each}</BrowseNavigationGrid>{/if}
    {#if subjectPath.at(-1)?.id === "__uncategorized__"}<MetadataReviewPanel {libraryId} onchanged={onimported} />{/if}
    <BookGrid {libraryId} {books} {detailed} emptyMessage="" {openingBookId} {downloadStates} {onopen} {ondownload} {onremove} {ontrash} />
    {#if fileType === "all" && recommendationsLoading}<div class="state recommendations-state" aria-live="polite">Finding catalogue recommendations…</div>
    {:else if fileType === "all" && recommendations?.recommendations.length}
      <section class="recommendations" aria-labelledby="subject-recommendations-title">
        <header><p>From your catalogues</p><h2 id="subject-recommendations-title">{subjectPath.length ? `More in ${subjectPath.at(-1)?.name}` : "Recommended from your subjects"}</h2></header>
        <div class="catalog-books">
          {#each recommendations.recommendations as item (recommendationKey(item))}
            {@const action = primaryCatalogAction(item.publication.actions)}
            {@const actionKey = action ? catalogActionKey(action) : ""}
            <CatalogPublicationCard sourceId={item.source_id} publication={item.publication} {action} actionState={actionStates[actionKey] ?? "idle"} actionError={actionErrors[actionKey] ?? ""} onaction={(selected) => runRecommendationAction(item, selected)} />
          {/each}
        </div>
      </section>
    {:else if fileType === "all" && recommendationsError}<div class="recommendations-error" role="alert"><span>Catalogue recommendations unavailable: {recommendationsError}</span><button type="button" onclick={() => void loadRecommendations(libraryId, subjectPath.at(-1)?.id ?? null)}>Try again</button></div>{/if}
    {#if !subjects.length && !books.length && (fileType !== "all" || (!recommendationsLoading && !recommendations?.recommendations.length))}<EmptyPage title={fileType === "all" ? "No subjects" : `No ${selectedTypeLabel.toLowerCase()} here`} description={fileType === "all" ? "No books have subject information here yet." : `This subject and its subsubjects contain no ${selectedTypeLabel.toLowerCase()}.`} />{/if}
  {/if}
</section>
<style>
  .browse-page { width: 100%; }
  .state { display: grid; min-height: 10rem; place-content: center; color: var(--color-subtle-text); }
  .recommendations { display: grid; gap: .8rem; margin-top: 1.5rem; padding-top: 1.2rem; border-top: 1px solid var(--color-border); }
  .recommendations header p, .recommendations header h2 { margin: 0; }
  .recommendations header p { color: var(--color-accent); font-size: .65rem; font-weight: 800; letter-spacing: .1em; text-transform: uppercase; }
  .recommendations header h2 { margin-top: .15rem; color: var(--color-ink-text); font: 650 1.1rem var(--font-interface-family); }
  .catalog-books { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(var(--store-publication-min-width), 100%), 1fr)); gap: 16px; overflow-x: auto; }
  .recommendations-state { min-height: 5rem; margin-top: 1rem; border-top: 1px solid var(--color-border); }
  .recommendations-error { display: flex; align-items: center; justify-content: space-between; gap: .75rem; margin-top: 1rem; padding: .7rem; border: 1px solid color-mix(in srgb, var(--color-danger-text) 35%, transparent); border-radius: 0; color: var(--color-danger-text); background: var(--color-danger-bg); font-size: .72rem; }
  .recommendations-error button { flex: none; padding: .4rem .6rem; border: 1px solid currentColor; border-radius: 0; color: inherit; background: transparent; font-weight: 750; cursor: pointer; }
  @container (max-width: 719px) { .catalog-books { overflow-x: hidden; } }
</style>
