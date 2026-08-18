<script lang="ts">
  import { onMount } from "svelte";
  import { commands } from "../../lib/commands";
  import { ROOT_DIRECTORY_ID } from "../../lib/directory-id";
  import { listen } from "../../lib/app-events";
  import { catalogActionKey, primaryCatalogAction } from "../../lib/catalog-actions";
  import { catalogLinkAction, catalogLinkCount } from "../../lib/catalog-links";
  import { openExternalUrl } from "../../lib/external-links";
  import { managedMenu } from "../../lib/keyboard-navigation";
  import { isNativeApp } from "../../lib/platform";
  import { BROWSING_PREFERENCES_CONTRACT, SETTINGS_CONTRACT, STORE_CONTRACT, type BrowsingPreferencePatch, type BrowsingPreferences } from "../../lib/types";
  import FolderBreadcrumbs from "../library/FolderBreadcrumbs.svelte";
  import FolderNavigationCard from "../library/FolderNavigationCard.svelte";
  import CatalogPublicationCard from "./CatalogPublicationCard.svelte";
  import type { CatalogImportRequest, CatalogLink, CatalogPage, CatalogPageView, CatalogPublication, CatalogPublicationAction, CatalogPublicationKey, CatalogSearch, CatalogSource, CatalogSubjectBrowseView, CatalogWideSearchResult, CatalogWideSearchView } from "../../lib/bindings";

  let { activeLibraryId, libraryName, preferences, onpreferencechange, onimported }: { activeLibraryId: string; libraryName: string; preferences: BrowsingPreferences; onpreferencechange: (patch: BrowsingPreferencePatch) => void | Promise<void>; onimported: () => void | Promise<void> } = $props();

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
  let customSaving = $state(false);
  let wideQuery = $state("");
  let wideResult = $state<CatalogWideSearchResult | null>(null);
  let wideSearching = $state(false);
  let subjectBrowse = $state<CatalogSubjectBrowseView | null>(null);
  let subjectLevels = $state<CatalogLevel[]>([]);
  let subjectSourceIds = $state<string[]>([]);
  let subjectQuery = $state("");
  let wideSearchGeneration = 0;
  let sourcesInitialized = false;
  const needsExternalAuthentication = $derived(selected?.authentication.kind === "external");
  const canSearch = $derived(Boolean(query.trim() || titleQuery.trim() || authorQuery.trim()));
  function catalogLanguageAvailable(option: { value: string; label: string }) {
    const value = option.value.toLocaleLowerCase();
    const label = option.label.toLocaleLowerCase();
    return sources.some((source) => source.languages.some((language) => {
      const candidate = language.trim().replaceAll("_", "-").toLocaleLowerCase();
      return candidate === value || candidate === label || candidate.startsWith(`${value}-`);
    }));
  }

  const catalogLanguageOptions = $derived(BROWSING_PREFERENCES_CONTRACT.options.catalog_languages.filter(catalogLanguageAvailable));
  const catalogLanguageValues = $derived(catalogLanguageOptions.map((option) => option.value));
  const catalogLanguageLabel = $derived.by(() => {
    const included = preferences.included_catalog_languages;
    if (!included.length) return SETTINGS_CONTRACT.all_catalog_languages_label;
    if (included.length === 1) return catalogLanguageOptions.find((option) => option.value === included[0])?.label ?? included[0];
    return `${included.length} languages`;
  });

  type WideSearchRefresh = { library_id: string; query: string; view: CatalogWideSearchView };
  type SourceSearchRefresh = { library_id: string; source_id: string; query: string; view: CatalogPageView };

  function message(reason: unknown) {
    return reason instanceof Error ? reason.message : String(reason);
  }

  function linkKey(link: CatalogLink) {
    const destination = link.target.kind === "page" ? link.target.locator : link.target.url;
    return `${link.target.kind}:${destination}:${link.title}`;
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
    return Object.fromEntries(keys.map((key) => [ownershipKey(key.source_id, key.publication.edition_id), true]));
  }

  function catalogPage(source: CatalogSource, locator: string | null) {
    return commands.getCatalogPage(activeLibraryId, source.id, locator);
  }

  function subjectGroups() {
    if (!subjectBrowse) return [];
    return sources.flatMap((source) => {
      const publications = subjectBrowse?.response.publications.filter((item) => item.source_id === source.id).map((item) => item.publication) ?? [];
      return publications.length ? [{ source, publications }] : [];
    });
  }

  const subjectSourceLabel = $derived.by(() => {
    if (!subjectSourceIds.length) return "All catalogues";
    if (subjectSourceIds.length === 1) return sources.find((source) => source.id === subjectSourceIds[0])?.title ?? subjectSourceIds[0];
    return `${subjectSourceIds.length} catalogues`;
  });

  function currentSubjectLocator() {
    return subjectLevels.at(-1)?.url || null;
  }

  async function loadSubjects(locator: string | null, title?: string, push = false) {
    if (loading) return;
    loading = true;
    error = "";
    try {
      const view = await commands.getCatalogSubjects(activeLibraryId, locator, subjectSourceIds, subjectQuery.trim(), 48);
      subjectBrowse = view;
      ownership = ownedEditions(view.owned_publications);
      if (!subjectLevels.length || locator === null) subjectLevels = [{ title: "Subjects", url: "" }];
      else if (push && title) subjectLevels = [...subjectLevels, { title, url: locator ?? "" }];
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  async function setSubjectSources(sourceIds: string[]) {
    subjectSourceIds = sourceIds;
    await loadSubjects(currentSubjectLocator());
  }

  async function toggleSubjectSource(sourceId: string) {
    if (!subjectSourceIds.length) await setSubjectSources([sourceId]);
    else if (subjectSourceIds.includes(sourceId)) await setSubjectSources(subjectSourceIds.filter((candidate) => candidate !== sourceId));
    else await setSubjectSources([...subjectSourceIds, sourceId]);
  }

  async function searchSubjects(event: SubmitEvent) {
    event.preventDefault();
    await loadSubjects(currentSubjectLocator());
  }

  async function goToSubjectLevel(index: number) {
    if (loading || index < 0 || index >= subjectLevels.length || index === subjectLevels.length - 1) return;
    const target = subjectLevels[index];
    subjectLevels = subjectLevels.slice(0, index + 1);
    await loadSubjects(target.url || null);
  }

  function closeSubjects() {
    subjectBrowse = null;
    subjectLevels = [];
    error = "";
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
    await searchWide(terms);
  }

  async function searchWide(terms: string) {
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

  function catalogLanguageIncluded(language: string): boolean {
    return preferences.included_catalog_languages.length === 0 || preferences.included_catalog_languages.includes(language);
  }

  async function setCatalogLanguages(languages: string[]) {
    await onpreferencechange({ preference: "included_catalog_languages", value: languages });
    if (wideResult && wideQuery.trim()) await searchWide(wideQuery.trim());
  }

  function toggleCatalogLanguage(language: string, included: boolean) {
    const current = preferences.included_catalog_languages.length === 0
      ? [...catalogLanguageValues]
      : preferences.included_catalog_languages.filter((value) => catalogLanguageValues.some((candidate) => candidate === value));
    const next = included ? [...new Set([...current, language])] : current.filter((value) => value !== language);
    return setCatalogLanguages(next.length === catalogLanguageValues.length ? [] : next);
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

{#snippet subjectLinks(links: CatalogLink[])}
  {#if links.length}
    <div class="catalog-folders">
      {#each links as link (linkKey(link))}
        {@const count = catalogLinkCount(link)}
        <div class="catalog-folder">
          <FolderNavigationCard
            name={link.title}
            {count}
            active={link.active}
            disabled={loading}
            onclick={() => {
              if (link.target.kind === "page") void loadSubjects(link.target.locator, link.title, true);
            }}
          />
        </div>
      {/each}
    </div>
  {/if}
{/snippet}

{#snippet publicationCards(source: CatalogSource, items: CatalogPublication[])}
  {@const visibleItems = items.filter((publication) => !ownership[ownershipKey(source.id, publication.identity.edition_id)])}
  {#if visibleItems.length}
    <div class="catalog-books">
      {#each visibleItems as publication (`${publication.identity.edition_id}:${publication.title}`)}
        {@const primaryAction = primaryCatalogAction(publication.actions)}
        {@const actionKey = primaryAction ? catalogActionKey(primaryAction) : ""}
        {@const actionState = importStates[actionKey] ?? "idle"}
        {@const actionError = importErrors[actionKey] ?? ""}
        <CatalogPublicationCard sourceId={source.id} {publication} action={primaryAction} {actionState} {actionError} onaction={(action) => runPublicationAction(source, publication, action)} />
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
    {#if subjectBrowse}
      <div class="catalog-bar">
        <button class="back" type="button" onclick={closeSubjects}>← <span>{STORE_CONTRACT.catalogs_label}</span></button>
        <strong class="catalog-source-name">Browse by subject</strong>
        {#if subjectLevels.length > 1}<FolderBreadcrumbs rootLabel={subjectLevels[1].title} levels={subjectLevels.slice(2).map((level) => ({ name: level.title }))} onselect={(level) => void goToSubjectLevel(level + 1)} />{/if}
      </div>
      <div class="store-toolbar subject-toolbar">
        {#if subjectLevels.length > 1}
          <form class="wide-search" onsubmit={searchSubjects}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0"></path></svg>
            <input type="search" bind:value={subjectQuery} placeholder="Search in this subject" aria-label="Search in this subject" autocomplete="off" />
            <button type="submit" disabled={loading}>Search</button>
          </form>
        {/if}
        <details class="toolbar-menu source-menu">
          <summary class:active={subjectSourceIds.length > 0}>{subjectSourceLabel}<span aria-hidden="true">⌄</span></summary>
          <div class="toolbar-menu-popover" role="menu" aria-label="Included catalogues">
            <button type="button" role="menuitemcheckbox" aria-checked={subjectSourceIds.length === 0} onclick={(event) => { closeToolbarMenu(event); void setSubjectSources([]); }}>All catalogues</button>
            {#each sources as source (source.id)}
              <button type="button" role="menuitemcheckbox" aria-checked={subjectSourceIds.includes(source.id)} onclick={(event) => { closeToolbarMenu(event); void toggleSubjectSource(source.id); }}>{source.title}</button>
            {/each}
          </div>
        </details>
      </div>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {@render subjectLinks(subjectBrowse.response.navigation)}
      {#each subjectGroups() as group (group.source.id)}
        <section class="wide-group">
          <header><span class="source-mark">{group.source.title.slice(0, 1)}</span><div><h2>{group.source.title}</h2><small>OPDS · {group.source.languages.join(" · ")}</small></div><button type="button" onclick={() => void chooseSource(group.source)}>Open catalog</button></header>
          {@render publicationCards(group.source, group.publications)}
        </section>
      {/each}
      {#if !subjectBrowse.response.navigation.length && !subjectBrowse.response.publications.length}<p class="wide-status">No catalog books are assigned to this subject yet.</p>{/if}
      <div class="catalog-pagination" aria-label="Subject results pages">
        {#if subjectBrowse.response.pagination.previous}<button type="button" disabled={loading} onclick={() => void loadSubjects(subjectBrowse?.response.pagination.previous ?? null)}>Previous</button>{/if}
        {#if subjectBrowse.response.pagination.current_page}<span>Page {subjectBrowse.response.pagination.current_page}</span>{/if}
        {#if subjectBrowse.response.pagination.next}<button type="button" disabled={loading} onclick={() => void loadSubjects(subjectBrowse?.response.pagination.next ?? null)}>Next</button>{/if}
      </div>
    {:else}
    {#if sourceRefresh.kind === "failed"}<div class="error source-error" role="alert"><span>Could not update catalogs; showing available catalogs. {sourceRefresh.message}</span><button type="button" onclick={() => void refreshSources()}>{STORE_CONTRACT.retry_label}</button></div>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="store-toolbar">
      <form class="wide-search" onsubmit={runWideSearch}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0"></path></svg>
        <input type="search" bind:value={wideQuery} placeholder={STORE_CONTRACT.search_all_placeholder} aria-label={STORE_CONTRACT.search_all_placeholder} autocomplete="off" />
        <button type="submit" disabled={!wideQuery.trim() || wideSearching}>{STORE_CONTRACT.search_label}</button>
      </form>
      <button class="toolbar-button" type="button" disabled={loading} onclick={() => void loadSubjects(null)}>Browse by subject</button>
      <details class="toolbar-menu language-menu" use:managedMenu>
        <summary class:active={preferences.included_catalog_languages.length > 0}>{catalogLanguageLabel}<span aria-hidden="true">⌄</span></summary>
        <div class="toolbar-menu-popover" role="menu" aria-label={SETTINGS_CONTRACT.included_catalog_languages_label}>
          <button type="button" role="menuitemcheckbox" aria-checked={preferences.included_catalog_languages.length === 0} onclick={() => void setCatalogLanguages([])}>{SETTINGS_CONTRACT.all_catalog_languages_label}</button>
          {#each catalogLanguageOptions as option}
            <button type="button" role="menuitemcheckbox" aria-checked={catalogLanguageIncluded(option.value)} onclick={() => void toggleCatalogLanguage(option.value, !catalogLanguageIncluded(option.value))}>{option.label}</button>
          {/each}
        </div>
      </details>
    </div>
    {#if wideResult}
      <header class="wide-heading"><h2>Results for “{wideResult.query}”</h2><button type="button" onclick={clearWideSearch}>Clear search</button></header>
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
      <div class="sources">
        {#each sources as source (source.id)}
          <div class="source-entry">
            <button class="source-card" type="button" onclick={() => void chooseSource(source)}>
              <span class="source-mark">{source.title.slice(0, 1)}</span>
              <span class="source-copy"><strong>{source.title}</strong><small>{source.description}</small></span>
              <span class="chevron">›</span>
            </button>
            {#if customSourceIds.includes(source.id)}<button class="remove-source" type="button" disabled={customSaving} aria-label={`Remove ${source.title}`} onclick={() => void removeCustomCatalog(source.id)}>Remove</button>{/if}
          </div>
        {/each}
      </div>
    {/if}
    {/if}
  {:else}
    <div class="catalog-bar">
      <button class="back" type="button" onclick={() => { selected = null; catalog = null; error = ""; }}>← <span>{STORE_CONTRACT.catalogs_label}</span></button>
      <strong class="catalog-source-name">{selected.title}</strong>
      {#if levels.length > 1}<FolderBreadcrumbs rootLabel={catalogPageTitle(selected.title, levels[1].title)} levels={levels.slice(2).map((level) => ({ name: catalogPageTitle(selected!.title, level.title) }))} onselect={(level) => void goToLevel(level + 1)} />{/if}
    </div>

    {#if needsExternalAuthentication && !catalog}
      <section class="credentials">
        <strong>{STORE_CONTRACT.sign_in_needed_label}</strong>
        <p>{selected.description}</p>
        <button class="primary" type="button" onclick={() => void openProviderAuthentication()}>{selected.authentication.kind === "external" ? selected.authentication.action_label : "Continue"} ↗</button>
      </section>
    {/if}

    {#if catalog}
      <div class="catalog-toolbar">
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
        {#if catalog.navigation.length}
          <details class="toolbar-menu">
            <summary>Browse ({catalog.navigation.length})<span aria-hidden="true">⌄</span></summary>
            <div class="toolbar-menu-popover" role="menu">
              {#each catalog.navigation as link (linkKey(link))}<button type="button" role="menuitem" disabled={link.active || loading} onclick={(event) => { closeToolbarMenu(event); void followCatalogLink(link); }}>{catalogPageTitle(selected.title, link.title)}{link.number_of_items !== null ? ` (${link.number_of_items.toLocaleString()})` : ""}</button>{/each}
            </div>
          </details>
        {/if}
        {#each catalog.facets as facet (`${facet.title}:${facet.links.map(linkKey).join(":")}`)}
          <details class="toolbar-menu facet-menu">
            <summary class:active={facet.links.findIndex((link) => link.active) > 0}>{facetLabel(facet.title, facet.links)}<span aria-hidden="true">⌄</span></summary>
            <div class="toolbar-menu-popover" role="menu">
              {#each facet.links as link (linkKey(link))}<button type="button" role="menuitemradio" aria-checked={link.active} disabled={link.active || loading} onclick={(event) => { closeToolbarMenu(event); void followCatalogLink(link); }}>{catalogPageTitle(selected.title, link.title)}{link.number_of_items !== null ? ` (${link.number_of_items.toLocaleString()})` : ""}</button>{/each}
            </div>
          </details>
        {/each}
      </div>

      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {#if catalog.freshness === "stale"}<p class="cache-notice" role="status">Showing a saved catalog page while a fresh copy is requested.</p>{/if}
      {#if !catalog.publications.length && !catalog.groups.length}{@render catalogLinks(catalog.navigation)}{/if}

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
  .credentials p { max-width: 42rem; margin: .25rem 0 0; color: var(--color-subtle-text); font-size: .86rem; line-height: 1.5; }
  .store-toolbar, .catalog-toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: .5rem; margin-bottom: 1rem; }
  .wide-search { display: grid; flex: 1; grid-template-columns: 1.2rem minmax(0, 1fr) auto; align-items: center; min-width: 14rem; max-width: 30rem; height: 2.5rem; border: 1px solid var(--color-border); border-radius: 0; background: var(--color-raised-bg); }
  .wide-search:focus-within { border-color: var(--color-accent); }
  .wide-search svg { width: 1.1rem; margin-left: .8rem; fill: none; stroke: var(--color-subtle-text); stroke-width: 1.8; }
  .wide-search input { min-width: 0; height: 100%; padding: .5rem .8rem; border: 0; color: inherit; background: transparent; outline: none; }
  .wide-search button { align-self: stretch; padding: 0 1rem; border: 0; border-left: 1px solid var(--color-border); color: var(--color-accent); background: transparent; font-weight: 750; cursor: pointer; }
  .toolbar-button, .toolbar-menu > summary { display: inline-flex; align-items: center; gap: .35rem; height: 2.5rem; max-width: 15rem; padding: 0 .7rem; overflow: hidden; border: 1px solid var(--color-border); border-radius: 0; color: var(--color-ink-text); background: var(--color-raised-bg); font-size: .72rem; font-weight: 700; white-space: nowrap; text-overflow: ellipsis; cursor: pointer; list-style: none; }
  .toolbar-menu > summary::-webkit-details-marker { display: none; }
  .toolbar-menu > summary.active { border-color: var(--color-accent); color: var(--color-accent); }
  .toolbar-menu { position: relative; }
  .toolbar-menu-popover { position: absolute; z-index: 20; top: calc(100% + .25rem); right: 0; display: grid; width: max-content; min-width: 13rem; max-width: min(22rem, 85vw); max-height: 22rem; padding: .3rem; overflow-y: auto; border: 1px solid var(--color-border); background: var(--color-raised-bg); box-shadow: 0 .7rem 1.8rem var(--color-box-shadow); }
  .toolbar-menu-popover button { min-height: 2.25rem; padding: .45rem .65rem; border: 0; color: var(--color-ink-text); background: transparent; text-align: left; font-size: .72rem; cursor: pointer; }
  .toolbar-menu-popover button:hover { background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  .toolbar-menu-popover button[aria-checked="true"] { color: var(--color-accent); font-weight: 750; }
  .source-error { display: flex; align-items: center; justify-content: space-between; gap: 1rem; }
  .source-error button { flex: none; min-height: 2.25rem; padding: .4rem .7rem; border: 1px solid currentColor; border-radius: 0; color: inherit; background: transparent; font-weight: 750; cursor: pointer; }
  .wide-heading { display: flex; align-items: center; justify-content: space-between; gap: 1rem; margin: .3rem 0 1rem; }
  .wide-heading h2 { margin: 0; color: var(--color-ink-text); font: 650 1.05rem var(--font-interface-family); }
  .wide-heading button, .wide-group > header button { padding: .45rem .6rem; border: 0; color: var(--color-accent); background: transparent; font-size: .72rem; font-weight: 750; cursor: pointer; }
  .wide-groups { display: grid; gap: 1.25rem; }
  .wide-group { display: grid; gap: .8rem; }
  .wide-group > header { display: grid; grid-template-columns: 2.8rem minmax(0, 1fr) auto; gap: .75rem; align-items: center; }
  .wide-group > header h2 { margin: 0 0 .15rem; color: var(--color-ink-text); font: 700 1rem var(--font-interface-family); }
  .wide-group > header small, .wide-status { margin: 0; color: var(--color-subtle-text); font-size: .7rem; }
  .wide-result-section { display: grid; gap: .65rem; }
  .wide-result-section h3 { margin: 0; color: var(--color-ink-text); font: 700 .82rem var(--font-interface-family); }
  .wide-external-links { display: flex; flex-wrap: wrap; gap: .5rem; }
  .wide-external-links button { display: inline-flex; align-items: center; gap: .35rem; min-height: 2.4rem; padding: .5rem .75rem; border: 1px solid var(--color-accent); border-radius: 0; color: var(--color-accent); background: transparent; font-weight: 750; cursor: pointer; }
  .sources { display: grid; grid-template-columns: 1fr; gap: .75rem; }
  .source-entry { position: relative; min-width: 0; }
  .source-card { display: grid; width: 100%; grid-template-columns: 2.75rem minmax(0, 1fr) auto; gap: .75rem; align-items: center; min-width: 0; min-height: 4.75rem; padding: .875rem; border: 1px solid var(--color-border); border-radius: 0; color: inherit; background: var(--color-raised-bg); text-align: left; cursor: pointer; }
  .source-card:hover { border-color: var(--color-accent); background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  .source-entry:has(.remove-source) .source-card { padding-bottom: 2.35rem; }
  .remove-source { position: absolute; right: .7rem; bottom: .45rem; padding: .25rem .35rem; border: 0; color: var(--color-danger-text); background: transparent; font-size: .64rem; font-weight: 750; cursor: pointer; }
  .source-mark { display: grid; width: 2.75rem; height: 2.75rem; place-items: center; border-radius: 0; color: var(--color-accent); background: var(--color-active-highlight); font: 700 1.25rem var(--font-interface-family); }
  .source-copy { display: grid; min-width: 0; gap: .2rem; }
  .source-copy strong { color: var(--color-ink-text); font-size: .9rem; }
  .source-copy small { color: var(--color-subtle-text); font-size: .72rem; line-height: 1.35; }
  .chevron { color: var(--color-subtle-text); font-size: 1.4rem; }
  .back { display: inline-flex; align-items: center; gap: .35rem; padding: .5rem 0; border: 0; color: var(--color-accent); background: transparent; font-weight: 750; cursor: pointer; }
  .catalog-bar { display: flex; align-items: center; flex-wrap: wrap; gap: .5rem; margin-bottom: 1rem; }
  .catalog-bar .back { min-height: 2.5rem; padding: .45rem .25rem; color: var(--color-ink-text); font-size: .72rem; }
  .catalog-source-name { flex: none; color: var(--color-ink-text); font-size: .82rem; }
  .credentials { display: grid; max-width: 32rem; gap: .9rem; padding: 1.15rem; border: 1px solid var(--color-border); border-radius: 0; background: var(--color-raised-bg); }
  .catalog-search input { min-width: 0; border: 1px solid var(--color-border); color: inherit; background: var(--color-surface-bg); outline: none; }
  .primary, .retry, .pagination button, .catalog-pagination button { width: max-content; min-height: 2.55rem; padding: .55rem .9rem; border: 1px solid var(--color-accent); border-radius: 0; color: var(--color-raised-bg); background: var(--color-accent); font-weight: 750; cursor: pointer; }
  .catalog-search { display: grid; flex: 1; grid-template-columns: 1.2rem minmax(0, 1fr) auto; align-items: center; min-width: 8.75rem; max-width: 30rem; border: 1px solid var(--color-border); border-radius: 0; background: var(--color-raised-bg); }
  .catalog-search svg { width: 1.1rem; margin-left: .7rem; fill: none; stroke: var(--color-subtle-text); stroke-width: 1.8; }
  .search-fields { display: flex; min-width: 0; }
  .catalog-search input { min-height: 2.5rem; padding: .5rem .65rem; border: 0; background: transparent; }
  .search-fields input { flex: 1 1 9rem; }
  .search-fields input + input { border-left: 1px solid var(--color-border); }
  .catalog-search button { align-self: stretch; padding: 0 .9rem; border: 0; border-left: 1px solid var(--color-border); color: var(--color-accent); background: transparent; font-weight: 750; cursor: pointer; }
  .catalog-folders { display: flex; flex-wrap: wrap; align-items: flex-start; gap: .45rem; margin: 1rem 0 1.4rem; }
  .catalog-folder { width: max-content; max-width: 100%; min-width: 0; }
  .catalog-groups { display: grid; gap: 1.5rem; margin-top: 1.5rem; }
  .catalog-group { display: grid; gap: .7rem; }
  .catalog-group > header { display: flex; align-items: baseline; gap: .55rem; padding-bottom: .5rem; border-bottom: 1px solid var(--color-border); }
  .catalog-group > header h2 { margin: 0; color: var(--color-ink-text); font: 700 1.08rem var(--font-interface-family); }
  .catalog-group > header small { color: var(--color-subtle-text); font-size: .66rem; }
  .catalog-group > header button { margin-left: auto; padding: .3rem .15rem; border: 0; color: var(--color-accent); background: transparent; font-size: .7rem; font-weight: 750; cursor: pointer; }
  .catalog-group .catalog-folders { margin: 0; }
  .catalog-books { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(var(--store-publication-min-width), 100%), 1fr)); gap: 16px; overflow-x: auto; }
  button:disabled { cursor: default; opacity: .55; }
  .loading, .empty { display: flex; align-items: center; gap: .65rem; min-height: 5rem; color: var(--color-subtle-text); font-size: .82rem; }
  .loading span { width: 1rem; height: 1rem; border: 2px solid var(--color-border); border-top-color: var(--color-accent); border-radius: 50%; animation: spin .8s linear infinite; }
  .error { padding: .75rem; border: 1px solid color-mix(in srgb, var(--color-danger-text) 35%, transparent); border-radius: 0; color: var(--color-danger-text); background: var(--color-danger-bg); font-size: .78rem; }
  .cache-notice { padding: .6rem .75rem; border: 1px solid var(--color-border); border-radius: 0; color: var(--color-subtle-text); background: var(--color-active-highlight); font-size: .72rem; }
  .pagination { display: flex; justify-content: center; gap: .75rem; margin-top: 1.2rem; }
  .catalog-pagination { display: flex; align-items: center; justify-content: center; gap: .75rem; margin-top: 1.2rem; color: var(--color-subtle-text); }
  @keyframes spin { to { transform: rotate(360deg); } }
  @container (max-width: 719px) { .search-fields { flex-direction: column; } .search-fields input + input { border-top: 1px solid var(--color-border); border-left: 0; } .wide-group > header { grid-template-columns: 2.8rem minmax(0, 1fr); } .wide-group > header button { grid-column: 1 / -1; justify-self: start; } .catalog-books { overflow-x: hidden; } }
  @container (min-width: 720px) { .sources { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @container (min-width: 1152px) { .sources { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
</style>
