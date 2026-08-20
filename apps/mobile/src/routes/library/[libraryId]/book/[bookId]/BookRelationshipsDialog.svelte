<script lang="ts">
  import SquareButton from "../../../../../components/shared/buttons/SquareButton.svelte";
  import FormButton from "../../../../../components/shared/buttons/FormButton.svelte";
  import DangerButton from "../../../../../components/shared/buttons/DangerButton.svelte";
  import PrimaryButton from "../../../../../components/shared/buttons/PrimaryButton.svelte";
  import { onMount } from "svelte";

  import { commands } from "../../../../../lib/app/commands";
  import { libraryApi } from "../../../../../lib/library/api";
  import { modalFocus } from "../../../../../lib/app/keyboard-navigation";
  import { createBookLink } from "../../../../../lib/shared/semantic-links";
  import {
    BROWSING_CONTENT_CONTRACT,
    type BookCardData,
    type BookLink,
  } from "../../../../../lib/shared/types";

  let {
    libraryId,
    book,
    onclose,
  }: { libraryId: string; book: BookCardData; onclose: () => void } = $props();

  let links = $state<BookLink[]>([]);
  let targetQuery = $state("");
  let targetResults = $state<BookCardData[]>([]);
  let targetBookId = $state("");
  let relation = $state(
    BROWSING_CONTENT_CONTRACT.semantic_link_relations[0] ?? "related to",
  );
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
      const result = await libraryApi.search(libraryId, targetQuery.trim(), 20);
      targetResults = [...result.title_matches, ...result.author_matches].filter(
        (candidate) => candidate.content_hash !== book.content_hash,
      );
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
      await commands.upsertBookLink(
        libraryId,
        createBookLink(book.content_hash, targetBookId, relation, note),
      );
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

<div
  class="scrim"
  role="presentation"
  onclick={onclose}
>
  <dialog
    class="dialog"
    open
    aria-modal="true"
    aria-labelledby="relationships-title"
    use:modalFocus={{ onClose: onclose, initialFocus: ".target-search input" }}
    onclick={(event) => event.stopPropagation()}
  >
    <header>
      <div>
        <small>Relationships</small>
        <h2 id="relationships-title">{book.title || "Untitled"}</h2>
      </div>
      <SquareButton
        label="Close relationships"
        onclick={onclose}>×</SquareButton
      >
    </header>

    <section aria-labelledby="existing-relationships">
      <h3 id="existing-relationships">Existing relationships</h3>
      {#if links.length}
        <div class="relationship-list">
          {#each links as link (link.id)}
            <div class="relationship-row">
              <span
                ><strong>{link.relation}</strong><small
                  >{link.source_content_hash === book.content_hash
                    ? `To book ${link.target_content_hash}`
                    : `From book ${link.source_content_hash}`}{#if link.note}
                    · {link.note}{/if}</small
                ></span
              >
              <DangerButton
                disabled={busy}
                onclick={() => void removeRelationship(link)}>Remove</DangerButton
              >
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
          <input
            bind:value={targetQuery}
            aria-label="Find target book"
            placeholder="Target book title or author"
          />
          <PrimaryButton
            disabled={busy}
            onclick={() => void searchTargets()}>Search</PrimaryButton
          >
        </div>
        {#if targetResults.length}
          <select
            bind:value={targetBookId}
            aria-label="Target book"
            required
          >
            <option value="">Choose a target book</option>
            {#each targetResults as candidate (candidate.content_hash)}<option
                value={candidate.content_hash}
                >{candidate.title || "Untitled"} · {candidate.author ||
                  "Unknown author"}</option
              >{/each}
          </select>
        {/if}
        <select
          bind:value={relation}
          aria-label="Relationship"
        >
          {#each BROWSING_CONTENT_CONTRACT.semantic_link_relations as option}<option
              value={option}>{option}</option
            >{/each}
        </select>
        <input
          bind:value={note}
          aria-label="Relationship note"
          placeholder="Optional note"
        />
        <FormButton disabled={busy || !targetBookId}>Add relationship</FormButton>
      </form>
      {#if error}<p
          class="error"
          role="alert"
        >
          {error}
        </p>{/if}
    </section>
  </dialog>
</div>

<style>
  .scrim {
    position: fixed;
    z-index: 90;
    inset: 0;
    display: grid;
    place-items: end center;
    padding: 1rem;
    background: var(--color-overlay-scrim);
  }
  .dialog {
    width: min(100%, 34rem);
    max-height: min(90dvh, 48rem);
    margin: 0;
    padding: 1rem;
    overflow-y: auto;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: var(--color-ink-text);
    background: var(--color-raised-bg);
    box-shadow: 0 1.5rem 4rem var(--color-box-shadow);
  }
  header,
  .relationship-row,
  .target-search {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
  }
  header {
    align-items: start;
    margin-bottom: 1.1rem;
  }
  header small {
    color: var(--color-accent);
    font-size: 0.68rem;
    font-weight: 800;
  }
  h2,
  h3,
  p {
    margin: 0;
  }
  h2 {
    margin-top: 0.15rem;
    font: 500 1.3rem var(--font-interface-family);
  }
  h3 {
    margin-bottom: 0.55rem;
    color: var(--color-subtle-text);
    font-size: 0.72rem;
  }
  section + section {
    margin-top: 1rem;
    padding-top: 1rem;
    border-top: 1px solid var(--color-border);
  }
  input,
  select {
    font: inherit;
  }
  .relationship-list,
  form {
    display: grid;
    gap: 0.55rem;
  }
  .relationship-row {
    min-height: 3.4rem;
    padding: 0.65rem 0.75rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-surface-bg);
  }
  .relationship-row span {
    display: grid;
    min-width: 0;
    gap: 0.15rem;
  }
  .relationship-row strong {
    font-size: 0.8rem;
  }
  .relationship-row small,
  .empty {
    color: var(--color-subtle-text);
    font-size: 0.7rem;
    overflow-wrap: anywhere;
  }
  input,
  select {
    width: 100%;
    min-height: 2.7rem;
    padding: 0.6rem 0.7rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: inherit;
    background: var(--color-surface-bg);
  }
  .target-search input {
    min-width: 0;
  }
  .error {
    margin-top: 0.65rem;
    color: var(--color-danger-text);
    font-size: 0.72rem;
  }
  @media (min-width: 45rem) {
    .scrim {
      place-items: center;
    }
  }
</style>
