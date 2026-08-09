<script lang="ts">
  import { onMount } from "svelte";

  import { commands } from "../../lib/commands";
  import { modalFocus } from "../../lib/keyboard-navigation";
  import { createBookLink } from "../../lib/semantic-links";
  import { BROWSING_CONTENT_CONTRACT, type BookCardData, type BookLink } from "../../lib/types";

  let { libraryId, book, onclose }: { libraryId: string; book: BookCardData; onclose: () => void } = $props();

  let links = $state<BookLink[]>([]);
  let targetQuery = $state("");
  let targetResults = $state<BookCardData[]>([]);
  let targetBookId = $state("");
  let relation = $state(BROWSING_CONTENT_CONTRACT.semantic_link_relations[0] ?? "related to");
  let note = $state("");
  let busy = $state(false);
  let error = $state("");

  async function loadLinks() {
    busy = true;
    error = "";
    try {
      links = await commands.getBookLinks(libraryId, book.content_hash);
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      busy = false;
    }
  }

  async function searchTargets() {
    error = "";
    if (targetQuery.trim().length < BROWSING_CONTENT_CONTRACT.minimum_search_characters) {
      error = `Type at least ${BROWSING_CONTENT_CONTRACT.minimum_search_characters} characters to find a book.`;
      return;
    }
    busy = true;
    try {
      targetResults = (await commands.searchLibrary(libraryId, targetQuery.trim(), 20)).filter((candidate) => candidate.content_hash !== book.content_hash);
      if (targetResults.length === 1) targetBookId = targetResults[0].content_hash;
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      busy = false;
    }
  }

  async function addRelationship(event: SubmitEvent) {
    event.preventDefault();
    if (!targetBookId) {
      error = "Choose a target book.";
      return;
    }
    busy = true;
    error = "";
    try {
      await commands.upsertBookLink(libraryId, createBookLink(book.content_hash, targetBookId, relation, note));
      targetQuery = "";
      targetResults = [];
      targetBookId = "";
      note = "";
      await loadLinks();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
      busy = false;
    }
  }

  async function removeRelationship(link: BookLink) {
    busy = true;
    error = "";
    try {
      await commands.deleteBookLink(libraryId, link.id, Date.now());
      links = links.filter((candidate) => candidate.id !== link.id);
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      busy = false;
    }
  }

  onMount(() => void loadLinks());
</script>

<div class="scrim" role="presentation" onclick={onclose}>
  <dialog class="dialog" open aria-modal="true" aria-labelledby="relationships-title" use:modalFocus={{ onClose: onclose, initialFocus: ".target-search input" }} onclick={(event) => event.stopPropagation()}>
    <header>
      <div><small>Relationships</small><h2 id="relationships-title">{book.title || "Untitled"}</h2></div>
      <button class="close" type="button" aria-label="Close relationships" onclick={onclose}>×</button>
    </header>

    <section aria-labelledby="existing-relationships">
      <h3 id="existing-relationships">Existing relationships</h3>
      {#if links.length}
        <div class="relationship-list">
          {#each links as link (link.id)}
            <div class="relationship-row">
              <span><strong>{link.relation}</strong><small>{link.source_content_hash === book.content_hash ? `To book ${link.target_content_hash}` : `From book ${link.source_content_hash}`}{#if link.note} · {link.note}{/if}</small></span>
              <button type="button" disabled={busy} onclick={() => void removeRelationship(link)}>Remove</button>
            </div>
          {/each}
        </div>
      {:else if busy}
        <p class="empty">Loading relationships…</p>
      {:else}
        <p class="empty">No relationships yet</p>
      {/if}
    </section>

    <section aria-labelledby="add-relationship">
      <h3 id="add-relationship">Add relationship</h3>
      <form onsubmit={addRelationship}>
        <div class="target-search">
          <input bind:value={targetQuery} aria-label="Find target book" placeholder="Target book title or author" />
          <button type="button" disabled={busy} onclick={() => void searchTargets()}>Search</button>
        </div>
        {#if targetResults.length}
          <select bind:value={targetBookId} aria-label="Target book" required>
            <option value="">Choose a target book</option>
            {#each targetResults as candidate (candidate.content_hash)}<option value={candidate.content_hash}>{candidate.title || "Untitled"} · {candidate.author || "Unknown author"}</option>{/each}
          </select>
        {/if}
        <select bind:value={relation} aria-label="Relationship">
          {#each BROWSING_CONTENT_CONTRACT.semantic_link_relations as option}<option value={option}>{option}</option>{/each}
        </select>
        <input bind:value={note} aria-label="Relationship note" placeholder="Optional note" />
        <button class="primary" type="submit" disabled={busy || !targetBookId}>Add relationship</button>
      </form>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    </section>
  </dialog>
</div>

<style>
  .scrim { position: fixed; z-index: 90; inset: 0; display: grid; place-items: end center; padding: 1rem; background: var(--color-overlay-scrim); }
  .dialog { width: min(100%, 34rem); max-height: min(90dvh, 48rem); margin: 0; padding: 1rem; overflow-y: auto; border: 1px solid var(--color-border-color); border-radius: var(--radius-lg); color: var(--color-ink-text); background: var(--color-card-bg); box-shadow: 0 1.5rem 4rem var(--color-box-shadow); }
  header, .relationship-row, .target-search { display: flex; align-items: center; justify-content: space-between; gap: .75rem; }
  header { align-items: start; margin-bottom: 1.1rem; }
  header small { color: var(--color-accent); font-size: .68rem; font-weight: 800; }
  h2, h3, p { margin: 0; }
  h2 { margin-top: .15rem; font: 500 1.3rem var(--font-heading-family); }
  h3 { margin-bottom: .55rem; color: var(--color-subtle-text); font-size: .72rem; }
  section + section { margin-top: 1rem; padding-top: 1rem; border-top: 1px solid var(--color-border-color); }
  button, input, select { font: inherit; }
  .close { width: 2.3rem; height: 2.3rem; border: 0; border-radius: var(--radius-sm); color: var(--color-subtle-text); background: transparent; font-size: 1.4rem; cursor: pointer; }
  .relationship-list, form { display: grid; gap: .55rem; }
  .relationship-row { min-height: 3.4rem; padding: .65rem .75rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); background: var(--color-surface-bg); }
  .relationship-row span { display: grid; min-width: 0; gap: .15rem; }
  .relationship-row strong { font-size: .8rem; }
  .relationship-row small, .empty { color: var(--color-subtle-text); font-size: .7rem; overflow-wrap: anywhere; }
  .relationship-row button { border: 0; color: var(--color-danger-text); background: transparent; font-size: .72rem; font-weight: 750; cursor: pointer; }
  input, select { width: 100%; min-height: 2.7rem; padding: .6rem .7rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); color: inherit; background: var(--color-surface-bg); }
  .target-search input { min-width: 0; }
  .target-search button, .primary { min-height: 2.7rem; padding: .6rem .85rem; border: 0; border-radius: var(--radius-sm); color: var(--color-white-surface); background: var(--color-accent); font-weight: 750; cursor: pointer; }
  button:disabled { opacity: .55; cursor: default; }
  .error { margin-top: .65rem; color: var(--color-danger-text); font-size: .72rem; }
  @media (min-width: 45rem) { .scrim { place-items: center; } }
</style>
