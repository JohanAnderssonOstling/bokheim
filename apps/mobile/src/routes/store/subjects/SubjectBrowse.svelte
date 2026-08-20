<script lang="ts">
  import FolderBreadcrumbs from "../../../components/shared/browse/FolderBreadcrumbs.svelte";
  import CatalogNavigationGrid from "../catalog/CatalogNavigationGrid.svelte";
  import SearchField from "../../../components/shared/SearchField.svelte";
  import MenuItemButton from "../../../components/shared/buttons/MenuItemButton.svelte";
  import PrimaryButton from "../../../components/shared/buttons/PrimaryButton.svelte";
  import TextButton from "../../../components/shared/buttons/TextButton.svelte";
  import StoreBookGrid from "../shared/StoreBookGrid.svelte";
  import type { CatalogImportAction } from "../catalog/actions";
  import type {
    CatalogLink,
    CatalogPublication,
    CatalogSource,
    CatalogSubjectBrowseView,
  } from "../../../lib/bindings";

  type CatalogLevel = { title: string; url: string };
  type SubjectGroup = { source: CatalogSource; publications: CatalogPublication[] };

  let {
    browse,
    levels,
    sources,
    sourceIds,
    query,
    loading,
    error,
    groups,
    sourceLabel,
    getLinkKey,
    isOwned,
    actionStates,
    actionErrors,
    onclose,
    onlevel,
    onsearch,
    onsourcechange,
    onselect,
    onopensource,
    onaction,
    onpage,
  }: {
    browse: CatalogSubjectBrowseView;
    levels: CatalogLevel[];
    sources: CatalogSource[];
    sourceIds: string[];
    query: string;
    loading: boolean;
    error: string;
    groups: SubjectGroup[];
    sourceLabel: string;
    getLinkKey: (link: CatalogLink) => string;
    isOwned: (source: CatalogSource, publication: CatalogPublication) => boolean;
    actionStates: Record<string, "idle" | "adding" | "added">;
    actionErrors: Record<string, string>;
    onclose: () => void;
    onlevel: (index: number) => void | Promise<void>;
    onsearch: (event: SubmitEvent) => void | Promise<void>;
    onsourcechange: (sourceIds: string[]) => void | Promise<void>;
    onselect: (link: CatalogLink) => void | Promise<void>;
    onopensource: (source: CatalogSource) => void | Promise<void>;
    onaction: (
      source: CatalogSource,
      publication: CatalogPublication,
      action: CatalogImportAction,
    ) => void | Promise<void>;
    onpage: (locator: string) => void | Promise<void>;
  } = $props();

  function toggleSource(sourceId: string) {
    const next =
      sourceIds.length === 0
        ? [sourceId]
        : sourceIds.includes(sourceId)
          ? sourceIds.filter((id) => id !== sourceId)
          : [...sourceIds, sourceId];
    return onsourcechange(next);
  }
</script>

<div class="subject-browse">
  <div class="catalog-bar">
    <FolderBreadcrumbs
      rootLabel="Store"
      levels={[
        { name: "Subjects" },
        ...levels.slice(1).map((level) => ({ name: level.title })),
      ]}
      onselect={(level) => (level === 0 ? onclose() : void onlevel(level - 1))}
    />
  </div>
  <div class="store-toolbar subject-toolbar">
    <SearchField
      class="wide-search"
      value={query}
      oninput={(value) => (query = value)}
      placeholder={levels.length > 1 ? "Search in this subject" : "Search subjects"}
      ariaLabel={levels.length > 1 ? "Search in this subject" : "Search subjects"}
      onsubmit={onsearch}
    />
    <details class="toolbar-menu source-menu">
      <summary class:active={sourceIds.length > 0}
        >{sourceLabel}<span aria-hidden="true">⌄</span></summary
      >
      <div
        class="toolbar-menu-popover"
        role="menu"
        aria-label="Included catalogues"
      >
        <MenuItemButton
          role="menuitemcheckbox"
          ariaChecked={sourceIds.length === 0}
          active={sourceIds.length === 0}
          onclick={() => void onsourcechange([])}>All catalogues</MenuItemButton
        >
        {#each sources as source (source.id)}
          <MenuItemButton
            role="menuitemcheckbox"
            ariaChecked={sourceIds.includes(source.id)}
            active={sourceIds.includes(source.id)}
            onclick={() => void toggleSource(source.id)}>{source.title}</MenuItemButton
          >
        {/each}
      </div>
    </details>
  </div>
  {#if error}<p
      class="error"
      role="alert"
    >
      {error}
    </p>{/if}
  <CatalogNavigationGrid
    links={browse.response.navigation}
    {getLinkKey}
    {loading}
    {onselect}
  />
  {#each groups as group (group.source.id)}
    <section class="wide-group">
      <header>
        <span class="source-mark">{group.source.title.slice(0, 1)}</span>
        <div>
          <h2>{group.source.title}</h2>
          <small>OPDS · {group.source.languages.join(" · ")}</small>
        </div>
        <TextButton onclick={() => void onopensource(group.source)}
          >Open catalog</TextButton
        >
      </header>
      <StoreBookGrid
        source={group.source}
        items={group.publications}
        {isOwned}
        {actionStates}
        {actionErrors}
        {onaction}
      />
    </section>
  {/each}
  {#if !browse.response.navigation.length && !browse.response.publications.length}<p
      class="wide-status"
    >
      No catalog books are assigned to this subject yet.
    </p>{/if}
  <div
    class="catalog-pagination"
    aria-label="Subject results pages"
  >
    {#if browse.response.pagination.previous}<PrimaryButton
        disabled={loading}
        onclick={() => void onpage(browse.response.pagination.previous!)}
        >Previous</PrimaryButton
      >{/if}
    {#if browse.response.pagination.current_page}<span
        >Page {browse.response.pagination.current_page}</span
      >{/if}
    {#if browse.response.pagination.next}<PrimaryButton
        disabled={loading}
        onclick={() => void onpage(browse.response.pagination.next!)}>Next</PrimaryButton
      >{/if}
  </div>
</div>

<style>
  .catalog-bar,
  .store-toolbar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .toolbar-menu {
    position: relative;
  }

  .toolbar-menu > summary {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    height: 2.5rem;
    max-width: 15rem;
    padding: 0 0.7rem;
    overflow: hidden;
    border: 1px solid var(--color-border);
    color: var(--color-ink-text);
    background: var(--color-raised-bg);
    font-size: 0.72rem;
    font-weight: 700;
    white-space: nowrap;
    text-overflow: ellipsis;
    cursor: pointer;
    list-style: none;
  }

  .toolbar-menu > summary::-webkit-details-marker {
    display: none;
  }

  .toolbar-menu > summary.active {
    border-color: var(--color-accent);
    color: var(--color-accent);
  }

  .toolbar-menu-popover {
    position: absolute;
    z-index: 20;
    top: calc(100% + 0.25rem);
    right: 0;
    display: grid;
    width: max-content;
    min-width: 13rem;
    max-width: min(22rem, 85vw);
    max-height: 22rem;
    padding: 0.3rem;
    overflow-y: auto;
    border: 1px solid var(--color-border);
    background: var(--color-raised-bg);
    box-shadow: 0 0.7rem 1.8rem var(--color-box-shadow);
  }

  .wide-group {
    display: grid;
    gap: 0.8rem;
  }

  .wide-group > header {
    display: grid;
    grid-template-columns: 2.8rem minmax(0, 1fr) auto;
    gap: 0.75rem;
    align-items: center;
  }

  .wide-group > header h2 {
    margin: 0 0 0.15rem;
    color: var(--color-ink-text);
    font: 700 1rem var(--font-interface-family);
  }

  .wide-group > header small,
  .wide-status {
    margin: 0;
    color: var(--color-subtle-text);
    font-size: 0.7rem;
  }

  .source-mark {
    display: grid;
    width: 2.75rem;
    height: 2.75rem;
    place-items: center;
    color: var(--color-accent);
    background: var(--color-active-highlight);
    font: 700 1.25rem var(--font-interface-family);
  }

  .error {
    padding: 0.75rem;
    border: 1px solid color-mix(in srgb, var(--color-danger-text) 35%, transparent);
    color: var(--color-danger-text);
    background: var(--color-danger-bg);
    font-size: 0.78rem;
  }

  .catalog-pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    margin-top: 1.2rem;
    color: var(--color-subtle-text);
  }

  @container (max-width: 719px) {
    .wide-group > header {
      grid-template-columns: 2.8rem minmax(0, 1fr);
    }

    .wide-group > header :global(.text-button) {
      grid-column: 1 / -1;
    }
  }
</style>
