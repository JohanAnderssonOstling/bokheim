<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { commands } from "../../../lib/app/commands";
  import { ROOT_DIRECTORY_ID } from "../../../lib/library/directory-id";
  import { listen } from "../../../lib/app/app-events";
  import { catalogActionKey, type CatalogImportAction } from "./actions";
  import { catalogLinkAction } from "./links";
  import { managedMenu } from "../../../lib/app/keyboard-navigation";
  import FileIcon from "../../../components/shared/FileIcon.svelte";
  import MenuItemButton from "../../../components/shared/buttons/MenuItemButton.svelte";
  import PrimaryButton from "../../../components/shared/buttons/PrimaryButton.svelte";
  import SecondaryButton from "../../../components/shared/buttons/SecondaryButton.svelte";
  import TextButton from "../../../components/shared/buttons/TextButton.svelte";
  import magnifyingGlass from "@icons/phosphor/magnifying-glass.svg";
  import {
    BROWSING_PREFERENCES_CONTRACT,
    SETTINGS_CONTRACT,
    STORE_CONTRACT,
    type BrowsingPreferencePatch,
    type BrowsingPreferences,
  } from "../../../lib/shared/types";
  import FolderBreadcrumbs from "../../../components/shared/browse/FolderBreadcrumbs.svelte";
  import CatalogNavigationGrid from "./CatalogNavigationGrid.svelte";
  import StoreBookGrid from "../shared/StoreBookGrid.svelte";
  import type {
    CatalogImportRequest,
    CatalogLink,
    CatalogPage,
    CatalogPageView,
    CatalogPublication,
    CatalogPublicationKey,
    CatalogSearch,
    CatalogSource,
  } from "../../../lib/bindings";

  let {
    activeLibraryId,
    libraryName,
    preferences,
    onpreferencechange,
    onimported,
    catalogSourceId = "",
  }: {
    activeLibraryId: string;
    libraryName: string;
    preferences: BrowsingPreferences;
    onpreferencechange: (patch: BrowsingPreferencePatch) => void | Promise<void>;
    onimported: () => void | Promise<void>;
    catalogSourceId?: string;
  } = $props();

  type CatalogLevel = { title: string; url: string };
  type ImportState = "idle" | "adding" | "added";
  type SourceRefresh =
    | { kind: "available" }
    | { kind: "refreshing" }
    | { kind: "current" }
    | { kind: "failed"; message: string };

  let sources = $state<CatalogSource[]>([]);
  let selected = $state<CatalogSource | null>(null);
  let catalog = $state<CatalogPage | null>(null);
  let searchDefinition = $state<CatalogSearch | null>(null);
  let levels = $state<CatalogLevel[]>([]);
  let query = $state("");
  let titleQuery = $state("");
  let authorQuery = $state("");
  let loading = $state(false);
  let error = $state("");
  let importStates = $state<Record<string, ImportState>>({});
  let importErrors = $state<Record<string, string>>({});
  let ownership = $state<Record<string, boolean>>({});
  let sourceRefresh = $state<SourceRefresh>({ kind: "available" });
  let sourcesInitialized = false;
  const canSearch = $derived(
    Boolean(query.trim() || titleQuery.trim() || authorQuery.trim()),
  );
  function catalogLanguageAvailable(option: { value: string; label: string }) {
    const value = option.value.toLocaleLowerCase();
    const label = option.label.toLocaleLowerCase();
    return sources.some((source) =>
      source.languages.some((language) => {
        const candidate = language.trim().replaceAll("_", "-").toLocaleLowerCase();
        return (
          candidate === value || candidate === label || candidate.startsWith(`${value}-`)
        );
      }),
    );
  }

  const catalogLanguageOptions = $derived(
    BROWSING_PREFERENCES_CONTRACT.options.catalog_languages.filter(
      catalogLanguageAvailable,
    ),
  );
  const catalogLanguageValues = $derived(
    catalogLanguageOptions.map((option) => option.value),
  );
  const catalogLanguageLabel = $derived.by(() => {
    const included = preferences.included_catalog_languages;
    if (!included.length) return SETTINGS_CONTRACT.all_catalog_languages_label;
    if (included.length === 1)
      return (
        catalogLanguageOptions.find((option) => option.value === included[0])?.label ??
        included[0]
      );
    return `${included.length} languages`;
  });

  type SourceSearchRefresh = {
    library_id: string;
    source_id: string;
    query: string;
    view: CatalogPageView;
  };

  function message(reason: unknown) {
    return reason instanceof Error ? reason.message : String(reason);
  }

  function linkKey(link: CatalogLink) {
    const destination =
      link.target.kind === "page" ? link.target.locator : link.target.url;
    return `${link.target.kind}:${destination}:${link.title}`;
  }

  function isOwnedPublication(source: CatalogSource, publication: CatalogPublication) {
    return Boolean(ownership[ownershipKey(source.id, publication.identity.edition_id)]);
  }

  function catalogPageTitle(catalogue: string, title: string) {
    if (!title.startsWith(catalogue)) return title;
    const remainder = title.slice(catalogue.length).replace(/^[\s:–—-]+/, "");
    return remainder || title;
  }

  function facetLabel(title: string, links: CatalogLink[]) {
    const active = links.find((link) => link.active);
    return active && selected ? catalogPageTitle(selected.title, active.title) : title;
  }

  function closeToolbarMenu(event: MouseEvent) {
    (event.currentTarget as HTMLElement).closest("details")?.removeAttribute("open");
  }

  function ownershipKey(sourceId: string, editionId: string) {
    return `${sourceId}:${editionId}`;
  }

  function ownedEditions(keys: CatalogPublicationKey[]) {
    return Object.fromEntries(
      keys.map((key) => [ownershipKey(key.source_id, key.publication.edition_id), true]),
    );
  }

  function catalogPage(source: CatalogSource, locator: string | null) {
    return commands.getCatalogPage(activeLibraryId, source.id, locator);
  }

  async function refreshSources() {
    if (sourceRefresh.kind === "refreshing") return;
    sourceRefresh = { kind: "refreshing" };
    try {
      sources = await commands.refreshCatalogSources();
      sourceRefresh = { kind: "current" };
    } catch (reason) {
      sourceRefresh = { kind: "failed", message: message(reason) };
    }
  }

  async function initializeSources() {
    try {
      sources = await commands.getCatalogSources();
    } catch (reason) {
      error = message(reason);
    }
    await refreshSources();
  }

  async function chooseSource(source: CatalogSource) {
    selected = source;
    catalog = null;
    searchDefinition = null;
    levels = [];
    query = "";
    titleQuery = "";
    authorQuery = "";
    error = "";
    importStates = {};
    ownership = {};
    importErrors = {};
    if (source.authentication.kind === "none")
      await loadCatalog(null, source.title, true);
  }

  function catalogLanguageIncluded(language: string): boolean {
    return (
      preferences.included_catalog_languages.length === 0 ||
      preferences.included_catalog_languages.includes(language)
    );
  }

  async function setCatalogLanguages(languages: string[]) {
    await onpreferencechange({
      preference: "included_catalog_languages",
      value: languages,
    });
  }

  function toggleCatalogLanguage(language: string, included: boolean) {
    const current =
      preferences.included_catalog_languages.length === 0
        ? [...catalogLanguageValues]
        : preferences.included_catalog_languages.filter((value) =>
            catalogLanguageValues.some((candidate) => candidate === value),
          );
    const next = included
      ? [...new Set([...current, language])]
      : current.filter((value) => value !== language);
    return setCatalogLanguages(next.length === catalogLanguageValues.length ? [] : next);
  }

  async function loadCatalog(url: string | null, title: string, root = false) {
    if (!selected || loading) return;
    if (url && catalog?.locator === url) return;
    loading = true;
    error = "";
    try {
      const view = await catalogPage(selected, url);
      const loaded = view.page;
      if (!selected) return;
      ownership = ownedEditions(view.owned_publications);
      catalog = loaded;
      if (root) {
        levels = [{ title: loaded.title || title, url: loaded.locator }];
        searchDefinition = loaded.search;
      } else {
        const existingLevel = levels.findIndex((level) => level.url === loaded.locator);
        levels =
          existingLevel >= 0
            ? levels.slice(0, existingLevel + 1)
            : [...levels, { title: loaded.title || title, url: loaded.locator }];
        if (!searchDefinition && loaded.search) searchDefinition = loaded.search;
      }
      query = "";
      titleQuery = "";
      authorQuery = "";
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  async function followCatalogLink(link: CatalogLink) {
    const action = catalogLinkAction(link);
    switch (action.kind) {
      case "disabled":
        return;
      case "load_page":
        await loadCatalog(action.locator, action.title);
        return;
      case "open_external":
        return;
    }
  }

  async function goToLevel(index: number) {
    if (!selected || loading || index < 0 || index >= levels.length) return;
    if (index === levels.length - 1) return;
    const target = levels[index];
    loading = true;
    error = "";
    try {
      const view = await catalogPage(selected, target.url);
      const loaded = view.page;
      ownership = ownedEditions(view.owned_publications);
      catalog = loaded;
      levels = levels.slice(0, index + 1);
      query = "";
      titleQuery = "";
      authorQuery = "";
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  async function runSearch(event: SubmitEvent) {
    event.preventDefault();
    if (!selected || !searchDefinition || !canSearch || loading) return;
    loading = true;
    error = "";
    try {
      const searchQuery = {
        query: query.trim(),
        title: titleQuery.trim() || null,
        author: authorQuery.trim() || null,
      };
      const view = await commands.searchCatalog(
        activeLibraryId,
        selected.id,
        searchDefinition,
        searchQuery,
      );
      const label = query.trim() || titleQuery.trim() || authorQuery.trim();
      installSourceSearch(view, label);
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  function installSourceSearch(view: CatalogPageView, label: string) {
    ownership = ownedEditions(view.owned_publications);
    catalog = view.page;
    levels = [
      ...levels.slice(0, 1),
      { title: `Search: ${label}`, url: view.page.locator },
    ];
  }

  async function addBook(
    source: CatalogSource,
    publication: CatalogPublication,
    request: CatalogImportRequest,
  ) {
    const importDestination = activeLibraryId
      ? { library_id: activeLibraryId, directory_id: ROOT_DIRECTORY_ID }
      : null;
    const actionKey = catalogActionKey({ kind: "import", request });
    if (
      !source ||
      !importDestination ||
      importStates[actionKey] === "adding" ||
      ownership[ownershipKey(source.id, publication.identity.edition_id)]
    )
      return;
    importStates = { ...importStates, [actionKey]: "adding" };
    importErrors = { ...importErrors, [actionKey]: "" };
    try {
      await commands.importCatalogBook(importDestination, request);
      importStates = { ...importStates, [actionKey]: "added" };
      ownership = {
        ...ownership,
        [ownershipKey(source.id, publication.identity.edition_id)]: true,
      };
      await onimported();
    } catch (reason) {
      importStates = { ...importStates, [actionKey]: "idle" };
      importErrors = { ...importErrors, [actionKey]: message(reason) };
    }
  }

  async function runPublicationAction(
    source: CatalogSource,
    publication: CatalogPublication,
    action: CatalogImportAction,
  ) {
    if (action.kind === "import") {
      await addBook(source, publication, action.request);
      return;
    }
    return;
  }

  $effect(() => {
    if (sourcesInitialized) return;
    sourcesInitialized = true;
    void initializeSources();
  });

  $effect(() => {
    if (!sourcesInitialized || !catalogSourceId || selected) return;
    const source = sources.find((candidate) => candidate.id === catalogSourceId);
    if (source) void chooseSource(source);
  });

  onMount(() => {
    const unlisten: Array<() => void> = [];
    let disposed = false;
    const retain = (dispose: () => void) =>
      disposed ? dispose() : unlisten.push(dispose);
    void listen<SourceSearchRefresh>("catalog:source-search-refreshed", ({ payload }) => {
      const label = query.trim() || titleQuery.trim() || authorQuery.trim();
      if (
        payload.library_id === activeLibraryId &&
        payload.source_id === selected?.id &&
        payload.query ===
          [query.trim(), titleQuery.trim(), authorQuery.trim()].filter(Boolean).join(" ")
      ) {
        installSourceSearch(payload.view, label);
      }
    }).then(retain);
    return () => {
      disposed = true;
      unlisten.forEach((dispose) => dispose());
    };
  });
</script>

<section
  class="store"
  aria-label="Book store"
  style={`--store-max-width: ${STORE_CONTRACT.max_width_px}px; --store-publication-min-width: ${STORE_CONTRACT.publication_min_width_px}px; --store-publication-height: ${STORE_CONTRACT.publication_height_px}px; --store-scrollbar-width: ${STORE_CONTRACT.description_scrollbar_width_px}px;`}
>
  {#if !selected}
    {#if sourceRefresh.kind === "failed"}<div
        class="error source-error"
        role="alert"
      >
        <span
          >Could not update catalogs; showing available catalogs. {sourceRefresh.message}</span
        ><SecondaryButton onclick={() => void refreshSources()}
          >{STORE_CONTRACT.retry_label}</SecondaryButton
        >
      </div>{/if}
    {#if error}<p
        class="error"
        role="alert"
      >
        {error}
      </p>{/if}
    <div class="store-toolbar">
      <button
        class="toolbar-button"
        type="button"
        disabled={loading}
        onclick={() => void goto("/store/subjects")}>Browse by subject</button
      >
      <details
        class="toolbar-menu language-menu"
        use:managedMenu
      >
        <summary class:active={preferences.included_catalog_languages.length > 0}
          >{catalogLanguageLabel}<span aria-hidden="true">⌄</span></summary
        >
        <div
          class="toolbar-menu-popover"
          role="menu"
          aria-label={SETTINGS_CONTRACT.included_catalog_languages_label}
        >
          <MenuItemButton
            role="menuitemcheckbox"
            ariaChecked={preferences.included_catalog_languages.length === 0}
            active={preferences.included_catalog_languages.length === 0}
            onclick={() => void setCatalogLanguages([])}
            >{SETTINGS_CONTRACT.all_catalog_languages_label}</MenuItemButton
          >
          {#each catalogLanguageOptions as option}
            <MenuItemButton
              role="menuitemcheckbox"
              ariaChecked={catalogLanguageIncluded(option.value)}
              active={catalogLanguageIncluded(option.value)}
              onclick={() =>
                void toggleCatalogLanguage(
                  option.value,
                  !catalogLanguageIncluded(option.value),
                )}
            >
              {option.label}
              {#snippet trailing()}
                {#if catalogLanguageIncluded(option.value)}<span aria-hidden="true"
                    >✓</span
                  >{/if}
              {/snippet}
            </MenuItemButton>
          {/each}
        </div>
      </details>
    </div>
    <div class="sources">
      {#each sources as source (source.id)}
        <div class="source-entry">
          <button
            class="source-card"
            type="button"
            onclick={() => void goto(`/store/catalog/${encodeURIComponent(source.id)}`)}
          >
            <span class="source-mark">{source.title.slice(0, 1)}</span>
            <span class="source-copy"
              ><strong>{source.title}</strong><small>{source.description}</small></span
            >
            <span class="chevron">›</span>
          </button>
        </div>
      {/each}
    </div>
  {:else}
    <div class="catalog-bar">
      <FolderBreadcrumbs
        rootLabel="Store"
        levels={[
          { name: selected.title },
          ...levels
            .slice(1)
            .map((level) => ({ name: catalogPageTitle(selected!.title, level.title) })),
        ]}
        onselect={(level) => {
          if (level === 0) {
            selected = null;
            catalog = null;
            error = "";
          } else void goToLevel(level - 1);
        }}
      />
    </div>

    {#if catalog}
      <div class="catalog-toolbar">
        {#if searchDefinition}
          <form
            class="catalog-search"
            onsubmit={runSearch}
          >
            <FileIcon source={magnifyingGlass} />
            <div class="search-fields">
              {#if searchDefinition.fields.includes("query")}<input
                  type="search"
                  bind:value={query}
                  placeholder={`Search ${selected.title}`}
                  aria-label={`Search ${selected.title}`}
                  autocomplete="off"
                />{/if}
              {#if searchDefinition.fields.includes("title")}<input
                  type="search"
                  bind:value={titleQuery}
                  placeholder="Title"
                  aria-label="Title"
                  autocomplete="off"
                />{/if}
              {#if searchDefinition.fields.includes("author")}<input
                  type="search"
                  bind:value={authorQuery}
                  placeholder="Author"
                  aria-label="Author"
                  autocomplete="off"
                />{/if}
            </div>
          </form>
        {/if}
        {#if catalog.navigation.length}
          <details class="toolbar-menu">
            <summary
              >Browse ({catalog.navigation.length})<span aria-hidden="true">⌄</span
              ></summary
            >
            <div
              class="toolbar-menu-popover"
              role="menu"
            >
              {#each catalog.navigation as link (linkKey(link))}<MenuItemButton
                  disabled={link.active || loading}
                  onclick={(event) => {
                    closeToolbarMenu(event);
                    void followCatalogLink(link);
                  }}
                  >{catalogPageTitle(selected.title, link.title)}{link.number_of_items !==
                  null
                    ? ` (${link.number_of_items.toLocaleString()})`
                    : ""}</MenuItemButton
                >{/each}
            </div>
          </details>
        {/if}
        {#each catalog.facets as facet (`${facet.title}:${facet.links.map(linkKey).join(":")}`)}
          <details class="toolbar-menu facet-menu">
            <summary class:active={facet.links.findIndex((link) => link.active) > 0}
              >{facetLabel(facet.title, facet.links)}<span aria-hidden="true">⌄</span
              ></summary
            >
            <div
              class="toolbar-menu-popover"
              role="menu"
            >
              {#each facet.links as link (linkKey(link))}<MenuItemButton
                  role="menuitemradio"
                  ariaChecked={link.active}
                  active={link.active}
                  disabled={link.active || loading}
                  onclick={(event) => {
                    closeToolbarMenu(event);
                    void followCatalogLink(link);
                  }}
                  >{catalogPageTitle(selected.title, link.title)}{link.number_of_items !==
                  null
                    ? ` (${link.number_of_items.toLocaleString()})`
                    : ""}</MenuItemButton
                >{/each}
            </div>
          </details>
        {/each}
      </div>

      {#if error}<p
          class="error"
          role="alert"
        >
          {error}
        </p>{/if}
      {#if catalog.freshness === "stale"}<p
          class="cache-notice"
          role="status"
        >
          Showing a saved catalog page while a fresh copy is requested.
        </p>{/if}
      {#if !catalog.publications.length && !catalog.groups.length}<CatalogNavigationGrid
          links={catalog.navigation}
          getLinkKey={linkKey}
          {loading}
          onselect={followCatalogLink}
        />{/if}

      <StoreBookGrid
        source={selected}
        items={catalog.publications}
        isOwned={isOwnedPublication}
        actionStates={importStates}
        actionErrors={importErrors}
        onaction={runPublicationAction}
      />

      {#if catalog.groups.length}
        <div class="catalog-groups">
          {#each catalog.groups as group, index (group.page_locator ?? `${group.title}:${index}`)}
            <section class="catalog-group">
              <header>
                <h2>{group.title}</h2>
                {#if group.number_of_items !== null}<small
                    >{group.number_of_items.toLocaleString()} books</small
                  >{/if}{#if group.page_locator}<TextButton
                    disabled={loading}
                    onclick={() => void loadCatalog(group.page_locator, group.title)}
                    >View all</TextButton
                  >{/if}
              </header>
              <CatalogNavigationGrid
                links={group.navigation}
                getLinkKey={linkKey}
                {loading}
                onselect={followCatalogLink}
              />
              <StoreBookGrid
                source={selected}
                items={group.publications}
                isOwned={isOwnedPublication}
                actionStates={importStates}
                actionErrors={importErrors}
                onaction={runPublicationAction}
              />
            </section>
          {/each}
        </div>
      {/if}

      {#if !catalog.publications.length && !catalog.navigation.length && !catalog.groups.length && !catalog.facets.length && !loading}
        <div class="empty">This catalog page has no supported books or folders.</div>
      {/if}

      <div class="pagination">
        {#if catalog.pagination.previous}<PrimaryButton
            onclick={() =>
              void loadCatalog(catalog!.pagination.previous!, "Previous page")}
            >← Previous</PrimaryButton
          >{/if}
        {#if catalog.pagination.next}<PrimaryButton
            onclick={() => void loadCatalog(catalog!.pagination.next!, "Next page")}
            >Next →</PrimaryButton
          >{/if}
      </div>
    {:else if loading}
      <div class="loading"><span></span>Opening {selected.title}…</div>
    {:else if error}
      <p
        class="error"
        role="alert"
      >
        {error}
      </p>
      <PrimaryButton onclick={() => void loadCatalog(null, selected!.title, true)}
        >Try again</PrimaryButton
      >
    {/if}
  {/if}
</section>

<style>
  button,
  input {
    font: inherit;
  }
  button {
    -webkit-tap-highlight-color: transparent;
  }
  .store {
    container-type: inline-size;
    width: min(var(--store-max-width), 100%);
    margin: 0 auto;
    padding: 0 0 2rem;
  }
  .credentials p {
    max-width: 42rem;
    margin: 0.25rem 0 0;
    color: var(--color-subtle-text);
    font-size: 0.86rem;
    line-height: 1.5;
  }
  .store-toolbar,
  .catalog-toolbar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }
  .toolbar-button,
  .toolbar-menu > summary {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    height: 2.5rem;
    max-width: 15rem;
    padding: 0 0.7rem;
    overflow: hidden;
    border: 1px solid var(--color-border);
    border-radius: 0;
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
  .toolbar-menu {
    position: relative;
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
  .language-menu .toolbar-menu-popover :global(.menu-item-button.active) {
    box-shadow: inset 3px 0 0 var(--color-accent);
  }
  .source-error {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }
  .sources {
    display: grid;
    grid-template-columns: 1fr;
    gap: 0.75rem;
  }
  .source-entry {
    position: relative;
    min-width: 0;
  }
  .source-card {
    display: grid;
    width: 100%;
    grid-template-columns: 2.75rem minmax(0, 1fr) auto;
    gap: 0.75rem;
    align-items: center;
    min-width: 0;
    min-height: 4.75rem;
    padding: 0.875rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: inherit;
    background: var(--color-raised-bg);
    text-align: left;
    cursor: pointer;
  }
  .source-card:hover {
    border-color: var(--color-accent);
    background-image: linear-gradient(
      var(--color-hover-overlay),
      var(--color-hover-overlay)
    );
  }
  .source-mark {
    display: grid;
    width: 2.75rem;
    height: 2.75rem;
    place-items: center;
    border-radius: 0;
    color: var(--color-accent);
    background: var(--color-active-highlight);
    font: 700 1.25rem var(--font-interface-family);
  }
  .source-copy {
    display: grid;
    min-width: 0;
    gap: 0.2rem;
  }
  .source-copy strong {
    color: var(--color-ink-text);
    font-size: 0.9rem;
  }
  .source-copy small {
    color: var(--color-subtle-text);
    font-size: 0.72rem;
    line-height: 1.35;
  }
  .chevron {
    color: var(--color-subtle-text);
    font-size: 1.4rem;
  }
  .catalog-bar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }
  .credentials {
    display: grid;
    max-width: 32rem;
    gap: 0.9rem;
    padding: 1.15rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-raised-bg);
  }
  .catalog-search input {
    min-width: 0;
    border: 1px solid var(--color-border);
    color: inherit;
    background: var(--color-surface-bg);
    outline: none;
  }
  .catalog-search {
    display: grid;
    flex: 1;
    grid-template-columns: 1.2rem minmax(0, 1fr);
    align-items: center;
    min-width: 8.75rem;
    max-width: 30rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-raised-bg);
  }
  .catalog-search :global(.file-icon) {
    margin-left: 0.7rem;
    color: var(--color-subtle-text);
  }
  .search-fields {
    display: flex;
    min-width: 0;
  }
  .catalog-search input {
    min-height: 2.5rem;
    padding: 0.5rem 0.65rem;
    border: 0;
    background: transparent;
  }
  .search-fields input {
    flex: 1 1 9rem;
  }
  .search-fields input + input {
    border-left: 1px solid var(--color-border);
  }
  .catalog-groups {
    display: grid;
    gap: 1.5rem;
    margin-top: 1.5rem;
  }
  .catalog-group {
    display: grid;
    gap: 0.7rem;
  }
  .catalog-group > header {
    display: flex;
    align-items: baseline;
    gap: 0.55rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid var(--color-border);
  }
  .catalog-group > header h2 {
    margin: 0;
    color: var(--color-ink-text);
    font: 700 1.08rem var(--font-interface-family);
  }
  .catalog-group > header small {
    color: var(--color-subtle-text);
    font-size: 0.66rem;
  }
  .catalog-group > header :global(.text-button) {
    margin-left: auto;
  }
  button:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .loading,
  .empty {
    display: flex;
    align-items: center;
    gap: 0.65rem;
    min-height: 5rem;
    color: var(--color-subtle-text);
    font-size: 0.82rem;
  }
  .loading span {
    width: 1rem;
    height: 1rem;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  .error {
    padding: 0.75rem;
    border: 1px solid color-mix(in srgb, var(--color-danger-text) 35%, transparent);
    border-radius: 0;
    color: var(--color-danger-text);
    background: var(--color-danger-bg);
    font-size: 0.78rem;
  }
  .cache-notice {
    padding: 0.6rem 0.75rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: var(--color-subtle-text);
    background: var(--color-active-highlight);
    font-size: 0.72rem;
  }
  .pagination {
    display: flex;
    justify-content: center;
    gap: 0.75rem;
    margin-top: 1.2rem;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @container (max-width: 719px) {
    .search-fields {
      flex-direction: column;
    }
    .search-fields input + input {
      border-top: 1px solid var(--color-border);
      border-left: 0;
    }
  }
  @container (min-width: 720px) {
    .sources {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
  @container (min-width: 1152px) {
    .sources {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }
</style>
