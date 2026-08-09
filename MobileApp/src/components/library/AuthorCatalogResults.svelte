<script lang="ts">
  import { onMount } from "svelte";

  import { commands } from "../../lib/commands";
  import { openExternalUrl } from "../../lib/external-links";
  import CatalogCover from "../store/CatalogCover.svelte";
  import type { CatalogAuthorDiscoveryMatch, CatalogAuthorDiscoveryResult, CatalogPublicationAction } from "../../lib/bindings";

  let { authorId, authorName, activeLibraryId, onimported }: { authorId: string; authorName: string; activeLibraryId: string; onimported: () => void | Promise<void> } = $props();

  let result = $state<CatalogAuthorDiscoveryResult | null>(null);
  let loading = $state(true);
  let error = $state("");
  let busy = $state<Record<string, boolean>>({});
  let imported = $state<Record<string, boolean>>({});

  function message(reason: unknown) {
    return reason instanceof Error ? reason.message : String(reason);
  }

  function matchKey(match: CatalogAuthorDiscoveryMatch) {
    return `${match.source.id}:${match.publication.identity.edition_id}:${match.publication.identity.work_id ?? ""}`;
  }

  function evidenceLabel(match: CatalogAuthorDiscoveryMatch) {
    if (match.evidence.kind === "author_name") return "Name match";
    return `${match.evidence.identifier.scheme.toUpperCase()} match`;
  }

  function primaryAction(match: CatalogAuthorDiscoveryMatch) {
    return match.publication.actions.find((action) => action.kind === "import") ?? match.publication.actions.find((action) => action.kind === "external") ?? null;
  }

  async function runAction(match: CatalogAuthorDiscoveryMatch, action: CatalogPublicationAction) {
    const key = matchKey(match);
    if (busy[key]) return;
    busy = { ...busy, [key]: true };
    error = "";
    try {
      if (action.kind === "import") {
        await commands.importCatalogBook({ library_id: activeLibraryId, directory_id: "__root__" }, action.request, null, null);
        imported = { ...imported, [key]: true };
        await onimported();
      } else {
        await openExternalUrl(action.url);
      }
    } catch (reason) {
      error = message(reason);
    } finally {
      busy = { ...busy, [key]: false };
    }
  }

  async function load() {
    loading = true;
    error = "";
    try {
      result = await commands.discoverCatalogAuthorBooks(activeLibraryId, authorId);
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  onMount(() => void load());
</script>

<section class="catalog-results" aria-labelledby="catalog-author-results-title">
  <header>
    <div><p>Online catalogues</p><h3 id="catalog-author-results-title">More books by {authorName}</h3></div>
    {#if result?.matches.length}<small>{result.matches.length} {result.matches.length === 1 ? "result" : "results"}</small>{/if}
  </header>

  {#if loading}
    <p class="status">Matching local book identifiers to catalogue authors…</p>
  {:else if result?.matches.length}
    <p class="summary">Identifier-backed matches are shown first. Name matches may include another author with the same name.</p>
    <div class="results">
      {#each result.matches as match (matchKey(match))}
        {@const action = primaryAction(match)}
        {@const key = matchKey(match)}
        <article>
          <div class="cover"><CatalogCover sourceId={match.source.id} imageUrl={match.publication.image_url} title={match.publication.title} /></div>
          <div class="copy">
            <span class:strong-evidence={match.evidence.kind === "book_identifier"} class="evidence">{evidenceLabel(match)}</span>
            <h4>{match.publication.title}</h4>
            <p>{match.publication.authors.map((author) => author.name).join(", ") || "Unknown author"}</p>
            <small>{match.source.title}</small>
          </div>
          {#if action}
            <button class="action" type="button" disabled={busy[key] || imported[key]} onclick={() => void runAction(match, action)}>
              {imported[key] ? "Added" : busy[key] ? "Working…" : action.kind === "import" ? "Add" : "Open ↗"}
            </button>
          {/if}
        </article>
      {/each}
    </div>
  {:else if !error}
    <p class="status">No catalogue books were found for this author.</p>
  {/if}

  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if error && !loading}<button class="retry" type="button" onclick={() => void load()}>Try again</button>{/if}
</section>

<style>
  .catalog-results { display: grid; gap: .7rem; margin-top: 1.5rem; padding-top: 1.25rem; border-top: 1px solid var(--color-border-color); }
  header { display: flex; align-items: end; justify-content: space-between; gap: .75rem; }
  header p, h3, h4, p { margin: 0; }
  header p { color: var(--color-accent); font-size: .62rem; font-weight: 800; letter-spacing: .1em; text-transform: uppercase; }
  header h3 { margin-top: .15rem; color: var(--color-heading-text); font: 600 1.05rem var(--font-heading-family); }
  header small, .summary, .status { color: var(--color-subtle-text); font-size: .7rem; line-height: 1.5; }
  .results { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(23rem, 100%), 1fr)); gap: .6rem; }
  article { display: grid; grid-template-columns: 3.4rem minmax(0, 1fr) auto; align-items: center; gap: .75rem; min-height: 5.2rem; padding: .65rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); background: var(--color-white-surface); }
  .cover { width: 3.4rem; height: 4.8rem; overflow: hidden; border-radius: .25rem; background: var(--color-active-highlight); }
  .copy { display: grid; min-width: 0; gap: .12rem; }
  .copy h4 { overflow: hidden; color: var(--color-heading-text); font: 650 .86rem var(--font-heading-family); text-overflow: ellipsis; white-space: nowrap; }
  .copy p, .copy small { overflow: hidden; color: var(--color-subtle-text); font-size: .68rem; text-overflow: ellipsis; white-space: nowrap; }
  .evidence { width: max-content; max-width: 100%; color: var(--color-subtle-text); font-size: .58rem; font-weight: 750; text-transform: uppercase; }
  .evidence.strong-evidence { color: var(--color-accent); }
  .action, .retry { min-height: 2.35rem; padding: .45rem .7rem; border: 1px solid var(--color-accent); border-radius: var(--radius-sm); color: var(--color-white-surface); background: var(--color-accent); font: inherit; font-size: .7rem; font-weight: 750; cursor: pointer; }
  button:disabled { opacity: .55; cursor: default; }
  .error { color: var(--color-danger-text); font-size: .72rem; }
  .retry { width: max-content; }
</style>
