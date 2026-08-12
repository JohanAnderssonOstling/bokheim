<script lang="ts">
  import { onMount } from "svelte";
  import { commands } from "../../lib/commands";
  import { ROOT_DIRECTORY_ID } from "../../lib/directory-id";
  import { listen } from "../../lib/app-events";
  import { catalogActionKey } from "../../lib/catalog-actions";
  import { catalogLinkAction, catalogLinkCount } from "../../lib/catalog-links";
  import { openExternalUrl } from "../../lib/external-links";
  import { isNativeApp } from "../../lib/platform";
  import { STORE_CONTRACT } from "../../lib/types";
  import FolderBreadcrumbs from "../library/FolderBreadcrumbs.svelte";
  import FolderNavigationCard from "../library/FolderNavigationCard.svelte";
  import CatalogCover from "./CatalogCover.svelte";
  import type { CatalogImportRequest, CatalogLink, CatalogPage, CatalogPageView, CatalogPublication, CatalogPublicationAction, CatalogPublicationKey, CatalogSearch, CatalogSource, CatalogWideSearchResult, CatalogWideSearchView } from "../../lib/bindings";

  let { activeLibraryId, libraryName, onimported }: { activeLibraryId: string; libraryName: string; onimported: () => void | Promise<void> } = $props();

  type CatalogLevel = { title: string; url: string };
  type ImportState = "idle" | "adding" | "added";
  type SourceRefresh = { kind: "available" } | { kind: "refreshing" } | { kind: "current" } | { kind: "failed"; message: string };

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
  let customSourceIds = $state<string[]>([]);
  let customTitle = $state("");
  let customUrl = $state("");
  let customSaving = $state(false);
  let wideQuery = $state("");
  let wideResult = $state<CatalogWideSearchResult | null>(null);
  let wideSearching = $state(false);
  let wideSearchGeneration = 0;
  let sourcesInitialized = false;
  const needsExternalAuthentication = $derived(selected?.authentication.kind === "external");
  const canSearch = $derived(Boolean(query.trim() || titleQuery.trim() || authorQuery.trim()));

  type WideSearchRefresh = { library_id: string; query: string; view: CatalogWideSearchView };
  type SourceSearchRefresh = { library_id: string; source_id: string; query: string; view: CatalogPageView };

  function message(reason: unknown) {
    return reason instanceof Error ? reason.message : String(reason);
  }

  function linkKey(link: CatalogLink) {
    const destination = link.target.kind === "page" ? link.target.locator : link.target.url;
    return `${link.target.kind}:${destination}:${link.title}`;
  }

  function authenticationSummary(source: CatalogSource) {
    if (source.authentication.kind === "none") return "No sign-in required";
    return "Continues in provider";
  }

  function ownershipKey(sourceId: string, editionId: string) {
    return `${sourceId}:${editionId}`;
  }

  function ownedEditions(keys: CatalogPublicationKey[]) {
    return Object.fromEntries(keys.map((key) => [ownershipKey(key.source_id, key.publication.edition_id), true]));
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
      if (isNativeApp) customSourceIds = (await commands.getCustomCatalogSources()).map((source) => source.id);
    } catch (reason) {
      error = message(reason);
    }
    await refreshSources();
  }

  async function addCustomCatalog(event: SubmitEvent) {
    event.preventDefault();
    if (!isNativeApp || customSaving || !customTitle.trim() || !customUrl.trim()) return;
    customSaving = true;
    error = "";
    try {
      const source = await commands.addCustomCatalog(customTitle.trim(), customUrl.trim());
      sources = [...sources.filter((candidate) => candidate.id !== source.id), source];
      customSourceIds = [...customSourceIds, source.id];
      customTitle = "";
      customUrl = "";
    } catch (reason) {
      error = message(reason);
    } finally {
      customSaving = false;
    }
  }

  async function removeCustomCatalog(sourceId: string) {
    if (!isNativeApp || customSaving || !customSourceIds.includes(sourceId)) return;
    customSaving = true;
    error = "";
    try {
      await commands.removeCustomCatalog(sourceId);
      sources = sources.filter((source) => source.id !== sourceId);
      customSourceIds = customSourceIds.filter((id) => id !== sourceId);
    } catch (reason) {
      error = message(reason);
    } finally {
      customSaving = false;
    }
  }

  async function chooseSource(source: CatalogSource) {
    wideSearchGeneration += 1;
    wideSearching = false;
    wideResult = null;
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
    if (source.authentication.kind === "none") await loadCatalog(null, source.title, true);
  }

  async function runWideSearch(event: SubmitEvent) {
    event.preventDefault();
    const terms = wideQuery.trim();
    if (!terms || wideSearching) return;
    const generation = ++wideSearchGeneration;
    wideSearching = true;
    error = "";
    try {
      const view = await commands.searchCatalogs(activeLibraryId, terms);
      if (generation !== wideSearchGeneration || wideQuery.trim() !== terms) return;
      installWideSearch(view);
    } catch (reason) {
      if (generation === wideSearchGeneration) error = message(reason);
    } finally {
      if (generation === wideSearchGeneration) wideSearching = false;
    }
  }

  function installWideSearch(view: CatalogWideSearchView) {
    ownership = ownedEditions(view.owned_publications);
    wideResult = view.result;
  }

  function clearWideSearch() {
    wideSearchGeneration += 1;
    wideSearching = false;
    wideResult = null;
    wideQuery = "";
    ownership = {};
    error = "";
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
        levels = existingLevel >= 0 ? levels.slice(0, existingLevel + 1) : [...levels, { title: loaded.title || title, url: loaded.locator }];
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
        try {
          await openExternalUrl(action.url);
        } catch (reason) {
          error = `Could not open catalog website: ${message(reason)}`;
        }
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
      const searchQuery = { query: query.trim(), title: titleQuery.trim() || null, author: authorQuery.trim() || null };
      const view = await commands.searchCatalog(activeLibraryId, selected.id, searchDefinition, searchQuery);
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
    levels = [...levels.slice(0, 1), { title: `Search: ${label}`, url: view.page.locator }];
  }

  async function addBook(source: CatalogSource, publication: CatalogPublication, request: CatalogImportRequest) {
    const importDestination = activeLibraryId ? { library_id: activeLibraryId, directory_id: ROOT_DIRECTORY_ID } : null;
    const actionKey = catalogActionKey({ kind: "import", request });
    if (!source || !importDestination || importStates[actionKey] === "adding" || ownership[ownershipKey(source.id, publication.identity.edition_id)]) return;
    importStates = { ...importStates, [actionKey]: "adding" };
    importErrors = { ...importErrors, [actionKey]: "" };
    try {
      await commands.importCatalogBook(importDestination, request);
      importStates = { ...importStates, [actionKey]: "added" };
      ownership = { ...ownership, [ownershipKey(source.id, publication.identity.edition_id)]: true };
      await onimported();
    } catch (reason) {
      importStates = { ...importStates, [actionKey]: "idle" };
      importErrors = { ...importErrors, [actionKey]: message(reason) };
    }
  }

  async function runPublicationAction(source: CatalogSource, publication: CatalogPublication, action: CatalogPublicationAction) {
    if (action.kind === "import") {
      await addBook(source, publication, action.request);
      return;
    }
    try {
      await openExternalUrl(action.url);
    } catch (reason) {
      error = `Could not open acquisition page: ${message(reason)}`;
    }
  }

  async function openProviderAuthentication() {
    if (selected?.authentication.kind !== "external") return;
    try {
      await openExternalUrl(selected.authentication.url);
    } catch (reason) {
      error = `Could not open provider: ${message(reason)}`;
    }
  }

  $effect(() => {
    if (sourcesInitialized) return;
    sourcesInitialized = true;
    void initializeSources();
  });

  onMount(() => {
    const unlisten: Array<() => void> = [];
    let disposed = false;
    const retain = (dispose: () => void) => disposed ? dispose() : unlisten.push(dispose);
    void listen<WideSearchRefresh>("catalog:search-refreshed", ({ payload }) => {
      if (payload.library_id === activeLibraryId && payload.query === wideQuery.trim()) installWideSearch(payload.view);
    }).then(retain);
    void listen<SourceSearchRefresh>("catalog:source-search-refreshed", ({ payload }) => {
      const label = query.trim() || titleQuery.trim() || authorQuery.trim();
      if (payload.library_id === activeLibraryId && payload.source_id === selected?.id && payload.query === [query.trim(), titleQuery.trim(), authorQuery.trim()].filter(Boolean).join(" ")) {
        installSourceSearch(payload.view, label);
      }
    }).then(retain);
    return () => {
      disposed = true;
      unlisten.forEach((dispose) => dispose());
    };
  });

</script>

{#snippet catalogLinks(links: CatalogLink[])}
  {#if links.length}
    <div class="catalog-folders">
      {#each links as link (linkKey(link))}
        {@const count = catalogLinkCount(link)}
        <div class="catalog-folder">
          <FolderNavigationCard
            name={link.title}
            {count}
            external={link.target.kind === "external"}
            active={link.active}
            disabled={loading}
            onclick={() => void followCatalogLink(link)}
          />
        </div>
      {/each}
    </div>
  {/if}
{/snippet}

{#snippet facetLinks(links: CatalogLink[])}
  <div class="facet-options">
    {#each links as link (linkKey(link))}
      {@const count = catalogLinkCount(link)}
      <button type="button" class:active={link.active} disabled={link.active || loading} aria-pressed={link.active} onclick={() => void followCatalogLink(link)}><span>{link.title}</span>{#if count}<small>{count}</small>{/if}{#if link.active}<i aria-hidden="true">✓</i>{/if}</button>
    {/each}
  </div>
{/snippet}

{#snippet publicationCards(source: CatalogSource, items: CatalogPublication[])}
  {@const visibleItems = items.filter((publication) => !ownership[ownershipKey(source.id, publication.identity.edition_id)])}
  {#if visibleItems.length}
    <div class="catalog-books">
      {#each visibleItems as publication (`${publication.identity.edition_id}:${publication.title}`)}
        {@const primaryAction = publication.actions.find((action) => action.kind === "import") ?? publication.actions.find((action) => action.kind === "external")}
        {@const actionKey = primaryAction ? catalogActionKey(primaryAction) : ""}
        {@const actionState = importStates[actionKey] ?? "idle"}
        {@const actionError = importErrors[actionKey] ?? ""}
        <article class="catalog-book">
          <div class="store-book-identity">
            <div class="remote-cover">
              <CatalogCover sourceId={source.id} imageUrl={publication.image_url} title={publication.title} />
            </div>
            <h2>{publication.title}</h2>
            <p>{publication.authors.map((author) => author.name).join(", ") || "Unknown author"}</p>
          </div>
          <div class="store-book-detail">
            <!-- svelte-ignore a11y_no_noninteractive_tabindex (keyboard users must be able to scroll overflowing descriptions) -->
            <div class="book-description" role="region" aria-label={`Description of ${publication.title}`} tabindex="0">
              {#if publication.description?.format === "plain_text"}
                <p>{publication.description.text}</p>
              {:else if publication.description?.safe_html}
                {@html publication.description.safe_html}
              {:else if publication.description}
                <p>{publication.description.plain_text}</p>
              {:else}
                <p>No description available.</p>
              {/if}
            </div>
            {#if actionError}<p class="publication-error" role="alert">{actionError}</p>{/if}
            {#if primaryAction}
              <button class="publication-action" class:external-action={primaryAction.kind === "external"} type="button" disabled={primaryAction.kind === "import" && actionState === "adding"} onclick={() => void runPublicationAction(source, publication, primaryAction)}>
                {#if primaryAction.kind === "import" && actionState === "adding"}<span class="button-progress" aria-hidden="true"></span>{/if}
                {actionError ? STORE_CONTRACT.retry_label : primaryAction.kind === "import" ? actionState === "adding" ? STORE_CONTRACT.adding_label : STORE_CONTRACT.add_label : STORE_CONTRACT.open_label}
                {#if primaryAction.kind === "external"}<span aria-hidden="true">↗</span>{/if}
              </button>
            {/if}
          </div>
        </article>
      {/each}
    </div>
  {/if}
{/snippet}

<section
  class="store"
  aria-label="Book store"
  style={`--store-max-width: ${STORE_CONTRACT.max_width_px}px; --store-publication-min-width: ${STORE_CONTRACT.publication_min_width_px}px; --store-publication-height: ${STORE_CONTRACT.publication_height_px}px; --store-scrollbar-width: ${STORE_CONTRACT.description_scrollbar_width_px}px;`}
>
  {#if !selected}
    {#if sourceRefresh.kind === "failed"}<div class="error source-error" role="alert"><span>Could not update catalogs; showing available catalogs. {sourceRefresh.message}</span><button type="button" onclick={() => void refreshSources()}>{STORE_CONTRACT.retry_label}</button></div>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <form class="wide-search" onsubmit={runWideSearch}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0"></path></svg>
      <input type="search" bind:value={wideQuery} placeholder={STORE_CONTRACT.search_all_placeholder} aria-label={STORE_CONTRACT.search_all_placeholder} autocomplete="off" />
      <button type="submit" disabled={!wideQuery.trim() || wideSearching}>{STORE_CONTRACT.search_label}</button>
    </form>
    {#if wideResult}
      <header class="wide-heading"><div><p class="eyebrow">All catalogs</p><h1>Results for “{wideResult.query}”</h1></div><button type="button" onclick={clearWideSearch}>Clear search</button></header>
      <div class="wide-groups">
        {#each wideResult.groups as group (group.source.id)}
          <section class="wide-group">
            <header><span class="source-mark">{group.source.title.slice(0, 1)}</span><div><h2>{group.source.title}</h2><small>OPDS · {group.source.languages.join(" · ")}</small></div><button type="button" onclick={() => void chooseSource(group.source)}>Open catalog</button></header>
            {#if group.outcome.kind === "results"}
              {@const resultCount = group.outcome.page.publications.length + group.outcome.page.groups.reduce((count, catalogGroup) => count + catalogGroup.publications.length, 0)}
              {#if group.outcome.page.publications.length}
                <div class="wide-result-section"><h3>Title matches</h3>{@render publicationCards(group.source, group.outcome.page.publications)}</div>
              {/if}
              {#each group.outcome.page.groups as resultGroup, resultGroupIndex (`${resultGroup.title}:${resultGroupIndex}`)}
                {#if resultGroup.publications.length}
                  <div class="wide-result-section"><h3>{resultGroup.title}</h3>{@render publicationCards(group.source, resultGroup.publications)}</div>
                {/if}
              {/each}
              {#if group.outcome.page.navigation.some((link) => link.target.kind === "external")}
                <div class="wide-external-links">
                  {#each group.outcome.page.navigation as link (linkKey(link))}
                    {#if link.target.kind === "external"}<button type="button" onclick={() => void followCatalogLink(link)}>{link.title}<span aria-hidden="true">↗</span></button>{/if}
                  {/each}
                </div>
              {/if}
              {#if !resultCount && !group.outcome.page.navigation.some((link) => link.target.kind === "external")}<p class="wide-status">No matching books were returned.</p>{/if}
            {:else if group.outcome.kind === "unsupported"}
              <p class="wide-status">This catalog does not advertise search.</p>
            {:else if group.outcome.kind === "timed_out"}
              <p class="wide-status">This catalog did not respond in time.</p>
            {:else}
              <p class="wide-status error">Search failed: {group.outcome.message}</p>
            {/if}
          </section>
        {/each}
      </div>
    {:else}
      {#if isNativeApp}
        <form class="custom-catalog" onsubmit={addCustomCatalog}>
          <header><strong>Add OPDS catalog</strong><small>Stored only on this device and contacted directly.</small></header>
          <div class="custom-fields">
            <label><span>Name</span><input bind:value={customTitle} maxlength="160" required autocomplete="off" /></label>
            <label><span>Catalog URL</span><input bind:value={customUrl} type="url" inputmode="url" placeholder="https://…" required autocomplete="url" /></label>
          </div>
          <div class="custom-options">
            <button type="submit" disabled={customSaving || !customTitle.trim() || !customUrl.trim()}>{customSaving ? "Saving…" : "Add catalog"}</button>
          </div>
        </form>
      {/if}
      <div class="sources">
        {#each sources as source (source.id)}
          <div class="source-entry">
            <button class="source-card" type="button" onclick={() => void chooseSource(source)}>
              <span class="source-mark">{source.title.slice(0, 1)}</span>
              <span class="source-copy"><strong>{source.title}</strong><small>{source.description}</small><i>OPDS · {source.languages.join(" · ")} · {authenticationSummary(source)}</i></span>
              <span class="chevron">›</span>
            </button>
            {#if customSourceIds.includes(source.id)}<button class="remove-source" type="button" disabled={customSaving} aria-label={`Remove ${source.title}`} onclick={() => void removeCustomCatalog(source.id)}>Remove</button>{/if}
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <header class="catalog-heading">
      <button class="back" type="button" onclick={() => { selected = null; catalog = null; error = ""; }}>← <span>{STORE_CONTRACT.catalogs_label}</span></button>
      <div><p class="eyebrow">OPDS · {selected.languages.join(" · ")}</p><h1>{selected.title}</h1></div>
    </header>

    {#if needsExternalAuthentication && !catalog}
      <section class="credentials">
        <strong>{STORE_CONTRACT.sign_in_needed_label}</strong>
        <p>{selected.description}</p>
        <button class="primary" type="button" onclick={() => void openProviderAuthentication()}>{selected.authentication.kind === "external" ? selected.authentication.action_label : "Continue"} ↗</button>
      </section>
    {/if}

    {#if catalog}
      <div class="catalog-tools">
        <FolderBreadcrumbs rootLabel={levels[0]?.title ?? selected.title} levels={levels.slice(1).map((level) => ({ name: level.title }))} onselect={(level) => void goToLevel(level)} />
        {#if searchDefinition}
          <form class="catalog-search" onsubmit={runSearch}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0"></path></svg>
            <div class="search-fields">
              {#if searchDefinition.fields.includes("query")}<input type="search" bind:value={query} placeholder={`Search ${selected.title}`} aria-label={`Search ${selected.title}`} autocomplete="off" />{/if}
              {#if searchDefinition.fields.includes("title")}<input type="search" bind:value={titleQuery} placeholder="Title" aria-label="Title" autocomplete="off" />{/if}
              {#if searchDefinition.fields.includes("author")}<input type="search" bind:value={authorQuery} placeholder="Author" aria-label="Author" autocomplete="off" />{/if}
            </div>
            <button type="submit" disabled={!canSearch || loading}>Search</button>
          </form>
        {/if}
      </div>

      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {#if catalog.freshness === "stale"}<p class="cache-notice" role="status">Showing a saved catalog page while a fresh copy is requested.</p>{/if}
      {@render catalogLinks(catalog.navigation)}

      {#if catalog.facets.length}
        <div class="catalog-facets" aria-label="Catalog filters">
          {#each catalog.facets as facet (`${facet.title}:${facet.links.map(linkKey).join(":")}`)}
            <section class="catalog-facet"><h2>{facet.title}</h2>{@render facetLinks(facet.links)}</section>
          {/each}
        </div>
      {/if}

      {@render publicationCards(selected, catalog.publications)}

      {#if catalog.groups.length}
        <div class="catalog-groups">
          {#each catalog.groups as group, index (group.page_locator ?? `${group.title}:${index}`)}
            <section class="catalog-group">
              <header><h2>{group.title}</h2>{#if group.number_of_items !== null}<small>{group.number_of_items.toLocaleString()} books</small>{/if}{#if group.page_locator}<button type="button" disabled={loading} onclick={() => void loadCatalog(group.page_locator, group.title)}>View all</button>{/if}</header>
              {@render catalogLinks(group.navigation)}
              {@render publicationCards(selected, group.publications)}
            </section>
          {/each}
        </div>
      {/if}

      {#if !catalog.publications.length && !catalog.navigation.length && !catalog.groups.length && !catalog.facets.length && !loading}
        <div class="empty">This catalog page has no supported books or folders.</div>
      {/if}

      <div class="pagination">
        {#if catalog.pagination.previous}<button type="button" onclick={() => void loadCatalog(catalog!.pagination.previous!, "Previous page")}>← Previous</button>{/if}
        {#if catalog.pagination.next}<button type="button" onclick={() => void loadCatalog(catalog!.pagination.next!, "Next page")}>Next →</button>{/if}
      </div>
    {:else if loading}
      <div class="loading"><span></span>Opening {selected.title}…</div>
    {:else if error}
      <p class="error" role="alert">{error}</p>
      <button class="retry" type="button" onclick={() => void loadCatalog(null, selected!.title, true)}>Try again</button>
    {/if}
  {/if}

</section>

<style>
  button, input { font: inherit; }
  button { -webkit-tap-highlight-color: transparent; }
  .store { container-type: inline-size; width: min(var(--store-max-width), 100%); margin: 0 auto; padding: 0 0 2rem; }
  .catalog-heading { display: flex; align-items: flex-end; justify-content: space-between; gap: 1rem; margin-bottom: 1.35rem; }
  h1 { margin: .1rem 0; color: var(--color-ink-text); font: 700 clamp(1.65rem, 5vw, 2.35rem) var(--font-interface-family); }
  .credentials p { max-width: 42rem; margin: .25rem 0 0; color: var(--color-subtle-text); font-size: .86rem; line-height: 1.5; }
  .eyebrow { margin: 0; color: var(--color-accent) !important; font-size: .68rem !important; font-weight: 800; letter-spacing: .11em; text-transform: uppercase; }
  .wide-search { display: grid; grid-template-columns: 1.2rem minmax(0, 1fr) auto; align-items: center; margin-bottom: 1.2rem; border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-raised-bg); }
  .wide-search:focus-within { border-color: var(--color-accent); }
  .wide-search svg { width: 1.1rem; margin-left: .8rem; fill: none; stroke: var(--color-subtle-text); stroke-width: 1.8; }
  .wide-search input { min-width: 0; min-height: 3rem; padding: .65rem .8rem; border: 0; color: inherit; background: transparent; outline: none; }
  .wide-search button { align-self: stretch; padding: 0 1rem; border: 0; border-left: 1px solid var(--color-border); color: var(--color-accent); background: transparent; font-weight: 750; cursor: pointer; }
  .source-error { display: flex; align-items: center; justify-content: space-between; gap: 1rem; }
  .source-error button { flex: none; min-height: 2.25rem; padding: .4rem .7rem; border: 1px solid currentColor; border-radius: var(--radius-sm); color: inherit; background: transparent; font-weight: 750; cursor: pointer; }
  .wide-heading { display: flex; align-items: center; justify-content: space-between; gap: 1rem; margin: .3rem 0 1rem; }
  .wide-heading button, .wide-group > header button { padding: .45rem .6rem; border: 0; color: var(--color-accent); background: transparent; font-size: .72rem; font-weight: 750; cursor: pointer; }
  .wide-groups { display: grid; gap: 1.25rem; }
  .wide-group { display: grid; gap: .8rem; padding: .9rem; border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-raised-bg); }
  .wide-group > header { display: grid; grid-template-columns: 2.8rem minmax(0, 1fr) auto; gap: .75rem; align-items: center; }
  .wide-group > header h2 { margin: 0 0 .15rem; color: var(--color-ink-text); font: 700 1rem var(--font-interface-family); }
  .wide-group > header small, .wide-status { margin: 0; color: var(--color-subtle-text); font-size: .7rem; }
  .wide-result-section { display: grid; gap: .65rem; }
  .wide-result-section h3 { margin: 0; color: var(--color-ink-text); font: 700 .82rem var(--font-interface-family); }
  .wide-group .catalog-book { background: var(--color-surface-bg); }
  .wide-external-links { display: flex; flex-wrap: wrap; gap: .5rem; }
  .wide-external-links button { display: inline-flex; align-items: center; gap: .35rem; min-height: 2.4rem; padding: .5rem .75rem; border: 1px solid var(--color-accent); border-radius: var(--radius-sm); color: var(--color-accent); background: transparent; font-weight: 750; cursor: pointer; }
  .custom-catalog { display: grid; gap: .75rem; margin-bottom: 1rem; padding: .9rem; border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-raised-bg); }
  .custom-catalog header { display: grid; gap: .15rem; }
  .custom-catalog header strong { color: var(--color-ink-text); font: 700 .9rem var(--font-interface-family); }
  .custom-catalog header small, .custom-catalog label { color: var(--color-subtle-text); font-size: .68rem; }
  .custom-fields { display: grid; grid-template-columns: minmax(10rem, .7fr) minmax(14rem, 1.3fr); gap: .6rem; }
  .custom-catalog label:not(.check) { display: grid; gap: .25rem; font-weight: 700; }
  .custom-catalog input:not([type="checkbox"]) { min-width: 0; min-height: 2.35rem; padding: .45rem .6rem; border: 1px solid var(--color-border); border-radius: var(--radius-sm); color: inherit; background: var(--color-surface-bg); outline: none; }
  .custom-options { display: flex; flex-wrap: wrap; align-items: end; gap: .65rem; }
  .custom-options button { min-height: 2.35rem; margin-left: auto; padding: .45rem .75rem; border: 1px solid var(--color-accent); border-radius: var(--radius-sm); color: var(--color-raised-bg); background: var(--color-accent); font-weight: 750; cursor: pointer; }
  .sources { display: grid; grid-template-columns: 1fr; gap: .75rem; }
  .source-entry { position: relative; min-width: 0; }
  .source-card { display: grid; width: 100%; grid-template-columns: 2.8rem minmax(0, 1fr) auto; gap: .8rem; align-items: center; min-width: 0; padding: .9rem; border: 1px solid var(--color-border); border-radius: var(--radius-md); color: inherit; background: var(--color-raised-bg); text-align: left; cursor: pointer; }
  .source-card:hover { border-color: var(--color-accent); background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  .source-entry:has(.remove-source) .source-card { padding-bottom: 2.35rem; }
  .remove-source { position: absolute; right: .7rem; bottom: .45rem; padding: .25rem .35rem; border: 0; color: var(--color-danger-text); background: transparent; font-size: .64rem; font-weight: 750; cursor: pointer; }
  .source-mark { display: grid; width: 2.8rem; height: 2.8rem; place-items: center; border-radius: var(--radius-sm); color: var(--color-accent); background: var(--color-active-highlight); font: 700 1.25rem var(--font-interface-family); }
  .source-copy { display: grid; min-width: 0; gap: .2rem; }
  .source-copy strong { color: var(--color-ink-text); font-size: .9rem; }
  .source-copy small { color: var(--color-subtle-text); font-size: .72rem; line-height: 1.35; }
  .source-copy i { color: var(--color-accent); font-size: .64rem; font-style: normal; font-weight: 700; }
  .chevron { color: var(--color-subtle-text); font-size: 1.4rem; }
  .back { display: inline-flex; align-items: center; gap: .35rem; padding: .5rem 0; border: 0; color: var(--color-accent); background: transparent; font-weight: 750; cursor: pointer; }
  .catalog-heading { justify-content: flex-start; align-items: center; }
  .catalog-heading > div { padding-left: 1rem; border-left: 1px solid var(--color-border); }
  .credentials { display: grid; max-width: 32rem; gap: .9rem; padding: 1.15rem; border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-raised-bg); }
  .catalog-search input { min-width: 0; border: 1px solid var(--color-border); color: inherit; background: var(--color-surface-bg); outline: none; }
  .primary, .retry, .pagination button { width: max-content; min-height: 2.55rem; padding: .55rem .9rem; border: 1px solid var(--color-accent); border-radius: var(--radius-sm); color: var(--color-raised-bg); background: var(--color-accent); font-weight: 750; cursor: pointer; }
  .catalog-tools { display: grid; gap: .85rem; margin-bottom: 1rem; }
  .catalog-search { display: grid; grid-template-columns: 1.2rem minmax(0, 1fr) auto; align-items: center; max-width: 46rem; border: 1px solid var(--color-border); border-radius: var(--radius-sm); background: var(--color-raised-bg); }
  .catalog-search svg { width: 1.1rem; margin-left: .7rem; fill: none; stroke: var(--color-subtle-text); stroke-width: 1.8; }
  .search-fields { display: flex; min-width: 0; }
  .catalog-search input { min-height: 2.7rem; padding: .55rem .65rem; border: 0; background: transparent; }
  .search-fields input { flex: 1 1 9rem; }
  .search-fields input + input { border-left: 1px solid var(--color-border); }
  .catalog-search button { align-self: stretch; padding: 0 .9rem; border: 0; border-left: 1px solid var(--color-border); color: var(--color-accent); background: transparent; font-weight: 750; cursor: pointer; }
  .catalog-folders { display: flex; flex-wrap: wrap; align-items: flex-start; gap: .45rem; margin: 1rem 0 1.4rem; }
  .catalog-folder { width: max-content; max-width: 100%; min-width: 0; }
  .catalog-facets { display: grid; gap: .75rem; margin: 1rem 0 1.4rem; padding: .85rem; border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-raised-bg); }
  .catalog-facet { display: grid; gap: .45rem; }
  .catalog-facet h2 { margin: 0; color: var(--color-ink-text); font: 700 .74rem var(--font-interface-family); }
  .facet-options { display: flex; flex-wrap: wrap; gap: .4rem; }
  .facet-options button { display: inline-flex; align-items: center; gap: .38rem; min-height: 2rem; padding: .35rem .6rem; border: 1px solid var(--color-border); border-radius: 999px; color: var(--color-subtle-text); background: var(--color-surface-bg); font-size: .68rem; cursor: pointer; }
  .facet-options button.active { border-color: var(--color-accent); color: var(--color-accent); background: var(--color-active-highlight); font-weight: 750; opacity: 1; }
  .facet-options small { color: inherit; font-size: .62rem; opacity: .8; }
  .facet-options i { font-style: normal; }
  .catalog-groups { display: grid; gap: 1.5rem; margin-top: 1.5rem; }
  .catalog-group { display: grid; gap: .7rem; }
  .catalog-group > header { display: flex; align-items: baseline; gap: .55rem; padding-bottom: .5rem; border-bottom: 1px solid var(--color-border); }
  .catalog-group > header h2 { margin: 0; color: var(--color-ink-text); font: 700 1.08rem var(--font-interface-family); }
  .catalog-group > header small { color: var(--color-subtle-text); font-size: .66rem; }
  .catalog-group > header button { margin-left: auto; padding: .3rem .15rem; border: 0; color: var(--color-accent); background: transparent; font-size: .7rem; font-weight: 750; cursor: pointer; }
  .catalog-group .catalog-folders { margin: 0; }
  .catalog-books { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(var(--store-publication-min-width), 100%), 1fr)); gap: 16px; overflow-x: auto; }
  .catalog-book { display: grid; grid-template-columns: var(--book-cover-width) minmax(0, 1fr); gap: 16px; min-width: var(--store-publication-min-width); height: var(--store-publication-height); overflow: hidden; border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-raised-bg); }
  .store-book-identity { display: flex; width: var(--book-cover-width); min-width: 0; height: var(--store-publication-height); flex-direction: column; }
  .remote-cover { display: grid; width: var(--book-cover-width); height: var(--book-cover-height); flex: none; place-items: center; overflow: hidden; border-radius: 8px; color: var(--color-accent); background: var(--color-active-highlight); box-shadow: 0 .35rem .9rem color-mix(in srgb, var(--color-box-shadow) 40%, transparent); font: 700 1.4rem var(--font-interface-family); }
  .store-book-identity h2, .store-book-identity p { width: var(--book-cover-width); overflow: hidden; text-overflow: ellipsis; }
  .store-book-identity h2 { display: -webkit-box; min-height: 2.3em; line-height: 1.15; white-space: normal; line-clamp: 2; -webkit-box-orient: vertical; -webkit-line-clamp: 2; }
  .store-book-identity p { white-space: nowrap; }
  .store-book-identity h2 { height: 24px; margin: 9px 0 0; padding: 0 2px; color: var(--color-ink-text); font: 600 17px/24px var(--font-interface-family); }
  .store-book-identity p { height: 22px; margin: 0; padding: 0 2px; color: var(--color-subtle-text); font: 400 15px/22px var(--font-interface-family); }
  .store-book-detail { display: flex; min-width: 0; min-height: 0; flex-direction: column; gap: 10px; padding: 16px 16px 16px 0; }
  .book-description { flex: 1; min-height: 0; overflow-x: hidden; overflow-y: auto; scrollbar-gutter: stable; scrollbar-width: auto; color: var(--color-subtle-text); font-size: 13px; line-height: 1.45; }
  .book-description::-webkit-scrollbar { width: var(--store-scrollbar-width); }
  .book-description::-webkit-scrollbar-track { background: transparent; }
  .book-description::-webkit-scrollbar-thumb { border-radius: 999px; background: var(--color-border); }
  .book-description :global(*) { max-width: 100%; }
  .book-description :global(p), .book-description :global(h1), .book-description :global(h2), .book-description :global(h3), .book-description :global(h4), .book-description :global(h5), .book-description :global(h6), .book-description :global(blockquote), .book-description :global(pre), .book-description :global(ul), .book-description :global(ol), .book-description :global(dl) { margin: 0; }
  .book-description :global(p + p), .book-description :global(div + div), .book-description :global(* + ul), .book-description :global(* + ol) { margin-top: .55em; }
  .book-description :global(ul), .book-description :global(ol) { padding-left: 1.15rem; }
  .publication-error { max-height: 48px; margin: 0; overflow-y: auto; color: var(--color-danger-text); font-size: 11px; line-height: 1.35; }
  .publication-action { display: inline-flex; flex: none; align-self: end; align-items: center; justify-content: center; gap: 6px; min-width: 88px; height: 40px; padding: 0 12px; border: 1px solid var(--color-accent); border-radius: var(--radius-sm); color: var(--color-raised-bg); background: var(--color-accent); font-size: 13px; font-weight: 750; cursor: pointer; }
  .publication-action.external-action { color: var(--color-accent); background: transparent; }
  .button-progress { width: 14px; height: 14px; border: 2px solid color-mix(in srgb, currentColor 35%, transparent); border-top-color: currentColor; border-radius: 50%; animation: spin .8s linear infinite; }
  button:disabled { cursor: default; opacity: .55; }
  .loading, .empty { display: flex; align-items: center; gap: .65rem; min-height: 5rem; color: var(--color-subtle-text); font-size: .82rem; }
  .loading span { width: 1rem; height: 1rem; border: 2px solid var(--color-border); border-top-color: var(--color-accent); border-radius: 50%; animation: spin .8s linear infinite; }
  .error { padding: .75rem; border: 1px solid color-mix(in srgb, var(--color-danger-text) 35%, transparent); border-radius: var(--radius-sm); color: var(--color-danger-text); background: var(--color-danger-bg); font-size: .78rem; }
  .cache-notice { padding: .6rem .75rem; border: 1px solid var(--color-border); border-radius: var(--radius-sm); color: var(--color-subtle-text); background: var(--color-active-highlight); font-size: .72rem; }
  .pagination { display: flex; justify-content: center; gap: .75rem; margin-top: 1.2rem; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @container (max-width: 719px) { .catalog-heading { align-items: flex-start; flex-wrap: wrap; } .custom-fields { grid-template-columns: 1fr; } .search-fields { flex-direction: column; } .search-fields input + input { border-top: 1px solid var(--color-border); border-left: 0; } .wide-group > header { grid-template-columns: 2.8rem minmax(0, 1fr); } .wide-group > header button { grid-column: 1 / -1; justify-self: start; } }
  @container (min-width: 720px) { .sources { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @container (min-width: 1152px) { .sources { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
</style>
