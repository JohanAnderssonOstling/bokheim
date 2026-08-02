<script lang="ts">
  import { commands } from "../../lib/bindings";
  import type { OpdsCatalog, OpdsEndpoint, OpdsEndpointId, OpdsEpubAcquisition, OpdsPublication, OpdsSearch } from "../../lib/bindings";

  let { activeLibraryId, onimported }: { activeLibraryId: string; onimported: () => void | Promise<void> } = $props();

  type CatalogLevel = { title: string; url: string };
  type ImportState = "idle" | "adding" | "added";

  let endpoints = $state<OpdsEndpoint[]>([]);
  let selected = $state<OpdsEndpoint | null>(null);
  let catalog = $state<OpdsCatalog | null>(null);
  let searchDefinition = $state<OpdsSearch | null>(null);
  let levels = $state<CatalogLevel[]>([]);
  let username = $state("");
  let password = $state("");
  let query = $state("");
  let loading = $state(false);
  let error = $state("");
  let importStates = $state<Record<string, ImportState>>({});

  const publications = $derived(catalog ? [...catalog.publications, ...catalog.groups.flatMap((group) => group.publications)] : []);
  const navigation = $derived(catalog ? [...catalog.navigation, ...catalog.groups.flatMap((group) => group.navigation)] : []);
  const needsCredentials = $derived(selected?.authentication.kind === "basic");
  const canConnect = $derived(!needsCredentials || username.trim().length > 0);

  function message(reason: unknown) {
    return reason instanceof Error ? reason.message : String(reason);
  }

  function credentials() {
    return { username: username.trim() || null, password: password || null };
  }

  async function chooseEndpoint(endpoint: OpdsEndpoint) {
    selected = endpoint;
    catalog = null;
    searchDefinition = null;
    levels = [];
    query = "";
    username = "";
    password = "";
    error = "";
    importStates = {};
    if (endpoint.authentication.kind === "none") await loadCatalog(null, endpoint.title, true);
  }

  async function loadCatalog(url: string | null, title: string, root = false) {
    if (!selected || loading) return;
    loading = true;
    error = "";
    try {
      const auth = credentials();
      const loaded = await commands.getOpdsCatalog(selected.id, url, auth.username, auth.password);
      catalog = loaded;
      if (root) {
        levels = [{ title: loaded.title || title, url: loaded.source_url }];
        searchDefinition = loaded.search;
      } else {
        levels = [...levels, { title: loaded.title || title, url: loaded.source_url }];
        if (!searchDefinition && loaded.search) searchDefinition = loaded.search;
      }
      query = "";
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  async function connectAuthenticated() {
    if (!selected || !canConnect) return;
    await loadCatalog(null, selected.title, true);
  }

  async function goToLevel(index: number) {
    if (!selected || loading || index < 0 || index >= levels.length) return;
    const target = levels[index];
    loading = true;
    error = "";
    try {
      const auth = credentials();
      catalog = await commands.getOpdsCatalog(selected.id, target.url, auth.username, auth.password);
      levels = levels.slice(0, index + 1);
      query = "";
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  async function runSearch(event: SubmitEvent) {
    event.preventDefault();
    if (!selected || !searchDefinition || !query.trim() || loading) return;
    loading = true;
    error = "";
    try {
      const auth = credentials();
      const results = await commands.searchOpdsCatalog(selected.id, searchDefinition, query.trim(), auth.username, auth.password);
      catalog = results;
      levels = [...levels.slice(0, 1), { title: `Search: ${query.trim()}`, url: results.source_url }];
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  async function addBook(publication: OpdsPublication, acquisition: OpdsEpubAcquisition) {
    const endpoint = selected;
    if (!endpoint || !activeLibraryId || importStates[publication.id] === "adding") return;
    importStates = { ...importStates, [publication.id]: "adding" };
    error = "";
    try {
      const auth = credentials();
      await commands.importOpdsEpub(endpoint.id, acquisition, auth.username, auth.password);
      importStates = { ...importStates, [publication.id]: "added" };
      await onimported();
    } catch (reason) {
      importStates = { ...importStates, [publication.id]: "idle" };
      error = message(reason);
    }
  }

  $effect(() => {
    if (endpoints.length) return;
    void commands.getOpdsEndpoints().then((sources) => (endpoints = sources)).catch((reason) => (error = message(reason)));
  });
</script>

<section class="store" aria-label="Book store">
  {#if !selected}
    <header class="store-heading">
      <div><p class="eyebrow">Online catalogs</p><h1>Store</h1><p>Browse free and open ebook collections from around the world.</p></div>
      <span class="source-count">{endpoints.length} sources</span>
    </header>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="sources">
      {#each endpoints as endpoint (endpoint.id)}
        <button class="source-card" type="button" onclick={() => void chooseEndpoint(endpoint)}>
          <span class="source-mark">{endpoint.title.slice(0, 1)}</span>
          <span class="source-copy"><strong>{endpoint.title}</strong><small>{endpoint.description}</small><i>{endpoint.languages.join(" · ")} · {endpoint.access === "open" ? "Open access" : "Membership"}</i></span>
          <span class="chevron">›</span>
        </button>
      {/each}
    </div>
  {:else}
    <header class="catalog-heading">
      <button class="back" type="button" onclick={() => { selected = null; catalog = null; error = ""; }}>← <span>Sources</span></button>
      <div><p class="eyebrow">{selected.languages.join(" · ")}</p><h1>{selected.title}</h1></div>
    </header>

    {#if needsCredentials && !catalog}
      <form class="credentials" onsubmit={(event) => { event.preventDefault(); void connectAuthenticated(); }}>
        <strong>Sign in to this catalog</strong>
        <p>{selected.description}</p>
        <label><span>{selected.authentication.kind === "basic" ? selected.authentication.username_label : "Username"}</span><input bind:value={username} autocomplete="username" type="email" /></label>
        {#if selected.authentication.kind === "basic" && selected.authentication.password === "required"}
          <label><span>Password</span><input bind:value={password} autocomplete="current-password" type="password" /></label>
        {:else}
          <small>Standard Ebooks expects the password to be left empty.</small>
        {/if}
        <button class="primary" type="submit" disabled={!canConnect || loading}>{loading ? "Connecting…" : "Open catalog"}</button>
      </form>
    {/if}

    {#if catalog}
      <div class="catalog-tools">
        <nav class="breadcrumbs" aria-label="Catalog path">
          {#each levels as level, index}
            {#if index}<span>/</span>{/if}<button type="button" class:current={index === levels.length - 1} onclick={() => void goToLevel(index)}>{level.title}</button>
          {/each}
        </nav>
        {#if searchDefinition}
          <form class="catalog-search" onsubmit={runSearch}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0"></path></svg>
            <input type="search" bind:value={query} placeholder={`Search ${selected.title}`} autocomplete="off" />
            <button type="submit" disabled={!query.trim() || loading}>Search</button>
          </form>
        {/if}
      </div>

      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {#if loading}<div class="loading"><span></span>Loading catalog…</div>{/if}

      {#if navigation.length}
        <div class="catalog-folders">
          {#each navigation as link (link.url)}
            <button type="button" onclick={() => void loadCatalog(link.url, link.title)}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6.5h7l2 2h9v10.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"></path></svg><span>{link.title}</span><i>›</i></button>
          {/each}
        </div>
      {/if}

      {#if publications.length}
        <div class="catalog-books">
          {#each publications as publication (`${publication.id}:${publication.title}`)}
            {@const acquisition = publication.free_epub}
            <article class="catalog-book">
              <div class="remote-cover">
                {#if publication.image_url}<img src={publication.image_url} alt="" loading="lazy" referrerpolicy="no-referrer" />{:else}<span>{publication.title.slice(0, 1)}</span>{/if}
              </div>
              <div class="book-copy"><h2>{publication.title}</h2><p>{publication.authors.join(", ") || "Unknown author"}</p>{#if publication.description}<small>{publication.description}</small>{/if}</div>
              {#if acquisition}
                <button class="download" type="button" disabled={!activeLibraryId || importStates[publication.id] === "adding" || importStates[publication.id] === "added"} onclick={() => void addBook(publication, acquisition)}>
                  {importStates[publication.id] === "adding" ? "Adding…" : importStates[publication.id] === "added" ? "Added" : activeLibraryId ? "Add to library" : "Create a library first"}
                </button>
              {/if}
            </article>
          {/each}
        </div>
      {:else if !navigation.length && !loading}
        <div class="empty">This catalog page has no EPUB books or folders.</div>
      {/if}

      <div class="pagination">
        {#if catalog.pagination.previous}<button type="button" onclick={() => void loadCatalog(catalog!.pagination.previous!, "Previous page")}>← Previous</button>{/if}
        {#if catalog.pagination.next}<button type="button" onclick={() => void loadCatalog(catalog!.pagination.next!, "Next page")}>Next →</button>{/if}
      </div>
    {:else if !needsCredentials && loading}
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
  .store { width: min(78rem, 100%); margin: 0 auto; padding: .25rem 0 2rem; }
  .store-heading, .catalog-heading { display: flex; align-items: flex-end; justify-content: space-between; gap: 1rem; margin-bottom: 1.35rem; }
  h1 { margin: .1rem 0; color: var(--color-heading-text); font: 700 clamp(1.65rem, 5vw, 2.35rem) var(--font-heading-family); }
  .store-heading p, .credentials p { max-width: 42rem; margin: .25rem 0 0; color: var(--color-subtle-text); font-size: .86rem; line-height: 1.5; }
  .eyebrow { margin: 0; color: var(--color-accent) !important; font-size: .68rem !important; font-weight: 800; letter-spacing: .11em; text-transform: uppercase; }
  .source-count { flex: none; padding: .35rem .65rem; border-radius: 999px; color: var(--color-accent); background: var(--color-active-highlight); font-size: .7rem; font-weight: 750; }
  .sources { display: grid; grid-template-columns: 1fr; gap: .75rem; }
  .source-card { display: grid; grid-template-columns: 2.8rem minmax(0, 1fr) auto; gap: .8rem; align-items: center; min-width: 0; padding: .9rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-md); color: inherit; background: var(--color-white-surface); text-align: left; cursor: pointer; }
  .source-card:hover { border-color: var(--color-accent); background: var(--color-hover-bg); }
  .source-mark { display: grid; width: 2.8rem; height: 2.8rem; place-items: center; border-radius: var(--radius-sm); color: var(--color-accent); background: var(--color-active-highlight); font: 700 1.25rem var(--font-heading-family); }
  .source-copy { display: grid; min-width: 0; gap: .2rem; }
  .source-copy strong { color: var(--color-heading-text); font-size: .9rem; }
  .source-copy small { color: var(--color-subtle-text); font-size: .72rem; line-height: 1.35; }
  .source-copy i { color: var(--color-accent); font-size: .64rem; font-style: normal; font-weight: 700; }
  .chevron { color: var(--color-subtle-text); font-size: 1.4rem; }
  .back { display: inline-flex; align-items: center; gap: .35rem; padding: .5rem 0; border: 0; color: var(--color-accent); background: transparent; font-weight: 750; cursor: pointer; }
  .catalog-heading { justify-content: flex-start; align-items: center; }
  .catalog-heading > div { padding-left: 1rem; border-left: 1px solid var(--color-border-color); }
  .credentials { display: grid; max-width: 32rem; gap: .9rem; padding: 1.15rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-md); background: var(--color-white-surface); }
  .credentials label { display: grid; gap: .35rem; color: var(--color-subtle-text); font-size: .72rem; font-weight: 700; }
  .credentials input, .catalog-search input { min-width: 0; border: 1px solid var(--color-border-color); color: inherit; background: var(--color-surface-bg); outline: none; }
  .credentials input { min-height: 2.7rem; padding: .65rem .75rem; border-radius: var(--radius-sm); }
  .credentials small { color: var(--color-subtle-text); }
  .primary, .retry, .pagination button { width: max-content; min-height: 2.55rem; padding: .55rem .9rem; border: 1px solid var(--color-accent); border-radius: var(--radius-sm); color: var(--color-white-surface); background: var(--color-accent); font-weight: 750; cursor: pointer; }
  .catalog-tools { display: grid; gap: .85rem; margin-bottom: 1rem; }
  .breadcrumbs { display: flex; align-items: center; gap: .35rem; overflow-x: auto; color: var(--color-subtle-text); white-space: nowrap; }
  .breadcrumbs button { padding: .25rem .1rem; border: 0; color: var(--color-accent); background: transparent; cursor: pointer; }
  .breadcrumbs button.current { color: var(--color-heading-text); font-weight: 750; }
  .catalog-search { display: grid; grid-template-columns: 1.2rem minmax(0, 1fr) auto; align-items: center; max-width: 40rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); background: var(--color-white-surface); }
  .catalog-search svg { width: 1.1rem; margin-left: .7rem; fill: none; stroke: var(--color-subtle-text); stroke-width: 1.8; }
  .catalog-search input { min-height: 2.7rem; padding: .55rem .65rem; border: 0; background: transparent; }
  .catalog-search button { align-self: stretch; padding: 0 .9rem; border: 0; border-left: 1px solid var(--color-border-color); color: var(--color-accent); background: transparent; font-weight: 750; cursor: pointer; }
  .catalog-folders { display: grid; grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr)); gap: .65rem; margin: 1rem 0 1.4rem; }
  .catalog-folders button { display: grid; grid-template-columns: 1.3rem minmax(0, 1fr) auto; gap: .6rem; align-items: center; min-width: 0; padding: .7rem .8rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); color: inherit; background: var(--color-white-surface); text-align: left; cursor: pointer; }
  .catalog-folders svg { width: 1.3rem; fill: none; stroke: var(--color-accent); stroke-width: 1.7; }
  .catalog-folders span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .catalog-folders i { color: var(--color-subtle-text); font-style: normal; }
  .catalog-books { display: grid; grid-template-columns: 1fr; gap: .8rem; }
  .catalog-book { display: grid; grid-template-columns: 4.2rem minmax(0, 1fr); gap: .85rem; align-items: start; padding: .8rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-md); background: var(--color-white-surface); }
  .remote-cover { display: grid; width: 4.2rem; height: 6rem; place-items: center; overflow: hidden; border-radius: .25rem; color: var(--color-accent); background: var(--color-active-highlight); font: 700 1.4rem var(--font-heading-family); }
  .remote-cover img { width: 100%; height: 100%; object-fit: cover; }
  .book-copy { min-width: 0; }
  .book-copy h2 { margin: .05rem 0 .25rem; color: var(--color-heading-text); font: 700 1rem/1.25 var(--font-heading-family); }
  .book-copy p { margin: 0; color: var(--color-accent); font-size: .72rem; font-weight: 700; }
  .book-copy small { display: -webkit-box; margin-top: .45rem; overflow: hidden; color: var(--color-subtle-text); font-size: .68rem; line-height: 1.4; line-clamp: 3; -webkit-box-orient: vertical; -webkit-line-clamp: 3; }
  .download { grid-column: 1 / -1; min-height: 2.45rem; padding: .5rem .75rem; border: 1px solid var(--color-accent); border-radius: var(--radius-sm); color: var(--color-accent); background: transparent; font-size: .74rem; font-weight: 780; cursor: pointer; }
  button:disabled { cursor: default; opacity: .55; }
  .loading, .empty { display: flex; align-items: center; gap: .65rem; min-height: 5rem; color: var(--color-subtle-text); font-size: .82rem; }
  .loading span { width: 1rem; height: 1rem; border: 2px solid var(--color-border-color); border-top-color: var(--color-accent); border-radius: 50%; animation: spin .8s linear infinite; }
  .error { padding: .75rem; border: 1px solid color-mix(in srgb, var(--color-danger-text) 35%, transparent); border-radius: var(--radius-sm); color: var(--color-danger-text); background: var(--color-danger-bg); font-size: .78rem; }
  .pagination { display: flex; justify-content: center; gap: .75rem; margin-top: 1.2rem; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (min-width: 38rem) { .sources, .catalog-books { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @media (min-width: 72rem) { .sources { grid-template-columns: repeat(3, minmax(0, 1fr)); } .catalog-books { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
</style>
