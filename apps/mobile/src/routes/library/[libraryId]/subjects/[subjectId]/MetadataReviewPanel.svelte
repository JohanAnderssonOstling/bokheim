<script lang="ts">
  import SquareButton from "../../../../../components/shared/buttons/SquareButton.svelte";
  import FormButton from "../../../../../components/shared/buttons/FormButton.svelte";
  import PrimaryButton from "../../../../../components/shared/buttons/PrimaryButton.svelte";
  import SecondaryButton from "../../../../../components/shared/buttons/SecondaryButton.svelte";
  import { commands } from "../../../../../lib/app/commands";
  import type {
    EditionIdentityReview,
    EditionIdentityReviewCandidate,
    EditionIdentityReviewStatus,
  } from "../../../../../lib/bindings";
  import { BROWSING_LAYOUT_CONTRACT } from "../../../../../lib/shared/types";
  import { getThumbnailUrl } from "../../../../../lib/library/thumbnail-cache";

  const ITEM_HEIGHT = BROWSING_LAYOUT_CONTRACT.metadata_review_item_height_px;
  const ROW_STRIDE = ITEM_HEIGHT + 16;
  const OVERSCAN = 3;
  const tabs: { status: EditionIdentityReviewStatus; label: string }[] = [
    { status: "ambiguous", label: "Ambiguous" },
    { status: "no_match", label: "No match" },
    { status: "failed", label: "Failed" },
  ];

  let {
    libraryId,
    onchanged,
  }: { libraryId: string; onchanged: () => void | Promise<void> } = $props();
  let reviews = $state<EditionIdentityReview[]>([]);
  let activeStatus = $state<EditionIdentityReviewStatus>("ambiguous");
  let loading = $state(true);
  let error = $state("");
  let busyHash = $state("");
  let candidateSelections = $state<Record<string, number>>({});
  let manualIsbns = $state<Record<string, string>>({});
  let coverUrls = $state<Record<string, string>>({});
  let viewport: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportHeight = $state(632);
  let loadGeneration = 0;

  const filteredReviews = $derived(
    reviews.filter((review) => review.status === activeStatus),
  );
  const startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_STRIDE) - OVERSCAN));
  const endIndex = $derived(
    Math.min(
      filteredReviews.length,
      Math.ceil((scrollTop + viewportHeight) / ROW_STRIDE) + OVERSCAN,
    ),
  );
  const visibleReviews = $derived(filteredReviews.slice(startIndex, endIndex));
  const totalHeight = $derived(filteredReviews.length * ROW_STRIDE);

  function message(reason: unknown): string {
    return reason instanceof Error ? reason.message : String(reason);
  }
  function count(status: EditionIdentityReviewStatus): number {
    return reviews.filter((review) => review.status === status).length;
  }
  function selection(review: EditionIdentityReview): number {
    return Math.min(
      candidateSelections[review.content_hash] ?? 0,
      Math.max(0, review.candidates.length - 1),
    );
  }
  function selectedCandidate(
    review: EditionIdentityReview,
  ): EditionIdentityReviewCandidate | undefined {
    return review.candidates[selection(review)];
  }
  function selectPrevious(review: EditionIdentityReview) {
    const current = selection(review);
    candidateSelections = {
      ...candidateSelections,
      [review.content_hash]: current === 0 ? review.candidates.length - 1 : current - 1,
    };
  }
  function selectNext(review: EditionIdentityReview) {
    candidateSelections = {
      ...candidateSelections,
      [review.content_hash]: (selection(review) + 1) % review.candidates.length,
    };
  }
  function measureViewport() {
    if (viewport) viewportHeight = viewport.clientHeight;
  }
  function handleScroll() {
    if (!viewport) return;
    scrollTop = viewport.scrollTop;
    viewportHeight = viewport.clientHeight;
  }
  function show(status: EditionIdentityReviewStatus) {
    activeStatus = status;
    scrollTop = 0;
    if (viewport) viewport.scrollTop = 0;
  }

  async function loadReviews() {
    const generation = ++loadGeneration;
    loading = true;
    error = "";
    try {
      const loaded = await commands.getEditionIdentityReviews(libraryId);
      if (generation === loadGeneration) {
        reviews = loaded;
        for (const review of loaded) {
          if (coverUrls[review.content_hash] !== undefined) continue;
          void getThumbnailUrl(libraryId, review.content_hash).then((url) => {
            if (generation === loadGeneration)
              coverUrls = { ...coverUrls, [review.content_hash]: url ?? "" };
          });
        }
      }
    } catch (reason) {
      if (generation === loadGeneration) error = message(reason);
    } finally {
      if (generation === loadGeneration) loading = false;
    }
  }

  async function run(review: EditionIdentityReview, action: () => Promise<unknown>) {
    if (busyHash) return;
    busyHash = review.content_hash;
    error = "";
    try {
      await action();
      await loadReviews();
      await onchanged();
    } catch (reason) {
      error = message(reason);
    } finally {
      busyHash = "";
    }
  }

  function applyCandidate(review: EditionIdentityReview) {
    const candidate = selectedCandidate(review);
    if (!candidate) return;
    return run(review, () =>
      commands.selectEditionIdentityCandidate(
        libraryId,
        review.content_hash,
        candidate.edition_id,
        candidate.isbn13,
      ),
    );
  }
  function applyManualIsbn(review: EditionIdentityReview) {
    const isbn = (manualIsbns[review.content_hash] ?? "").trim();
    if (!isbn) {
      error = `Enter an ISBN for “${review.title}”.`;
      return;
    }
    return run(review, () =>
      commands.applyManualEditionIdentity(libraryId, review.content_hash, isbn),
    );
  }
  function rejectCandidates(review: EditionIdentityReview) {
    return run(review, () =>
      commands.rejectEditionIdentityCandidates(libraryId, review.content_hash),
    );
  }
  function retry(review: EditionIdentityReview) {
    return run(review, () =>
      commands.retryEditionIdentity(libraryId, review.content_hash),
    );
  }

  $effect(() => {
    libraryId;
    coverUrls = {};
    void loadReviews();
  });
</script>

<svelte:window onresize={measureViewport} />

<section
  class="metadata-review"
  aria-labelledby="metadata-review-title"
>
  <header>
    <div>
      <p class="eyebrow">Uncategorized books</p>
      <h2 id="metadata-review-title">ISBN metadata review</h2>
      <p class="description">
        Resolve uncertain Open Library editions, enter a known ISBN, or retry a previous
        lookup.
      </p>
    </div>
    <SecondaryButton
      onclick={() => void loadReviews()}
      disabled={loading || !!busyHash}>Refresh</SecondaryButton
    >
  </header>

  <div
    class="tabs"
    role="tablist"
    aria-label="ISBN review status"
  >
    {#each tabs as tab}
      <button
        type="button"
        role="tab"
        aria-selected={activeStatus === tab.status}
        class:active={activeStatus === tab.status}
        onclick={() => show(tab.status)}
      >
        <span>{tab.label}</span><strong>{count(tab.status)}</strong>
      </button>
    {/each}
  </div>

  {#if error}<div
      class="review-error"
      role="alert"
    >
      <span>{error}</span><SquareButton
        label="Dismiss error"
        onclick={() => (error = "")}>×</SquareButton
      >
    </div>{/if}
  {#if loading}
    <div
      class="review-state"
      aria-live="polite"
    >
      Loading ISBN reviews…
    </div>
  {:else if !filteredReviews.length}
    <div class="review-state">
      No {tabs.find((tab) => tab.status === activeStatus)?.label.toLocaleLowerCase()} ISBN decisions
      need review.
    </div>
  {:else}
    <div
      class="review-viewport"
      role="tabpanel"
      aria-label={`${tabs.find((tab) => tab.status === activeStatus)?.label} ISBN reviews`}
      bind:this={viewport}
      onscroll={handleScroll}
    >
      <div
        class="review-canvas"
        style={`height: ${totalHeight}px`}
      >
        {#each visibleReviews as review, offset (review.content_hash)}
          {@const candidate = selectedCandidate(review)}
          <article
            class="review-item"
            aria-posinset={startIndex + offset + 1}
            aria-setsize={filteredReviews.length}
            style={`height:${ITEM_HEIGHT}px; transform: translateY(${(startIndex + offset) * ROW_STRIDE}px)`}
          >
            {#if review.status === "ambiguous" && review.candidates.length}
              <div class="comparison">
                <section class="comparison-column local-book">
                  <h4>Your book</h4>
                  <div class="local-evidence">
                    <div class="review-cover">
                      {#if coverUrls[review.content_hash]}<img
                          src={coverUrls[review.content_hash]}
                          alt=""
                        />{:else}<span>No cover</span>{/if}
                    </div>
                    <div class="metadata-values">
                      <strong>{review.title}</strong>
                      {#if review.authors.length}<span>{review.authors.join(", ")}</span
                        >{/if}
                      {#if review.publishers.length}<span
                          >{review.publishers.join(", ")}</span
                        >{/if}
                      {#if review.publication_year}<span>{review.publication_year}</span
                        >{/if}
                    </div>
                  </div>
                </section>
                <section class="comparison-column external-book">
                  <div class="comparison-heading">
                    <h4>External book</h4>
                    {#if review.candidates.length > 1}<div class="candidate-controls">
                        <SecondaryButton
                          onclick={() => selectPrevious(review)}
                          disabled={!!busyHash}>Previous</SecondaryButton
                        ><span>{selection(review) + 1} of {review.candidates.length}</span
                        ><SecondaryButton
                          onclick={() => selectNext(review)}
                          disabled={!!busyHash}>Next</SecondaryButton
                        >
                      </div>{/if}
                  </div>
                  {#if candidate}<div class="metadata-values external-values">
                      <strong>{candidate.title}</strong>
                      {#if candidate.authors.length}<span
                          >{candidate.authors.join(", ")}</span
                        >{/if}
                      {#if candidate.publishers.length}<span
                          >{candidate.publishers.join(", ")}</span
                        >{/if}
                      {#if candidate.publication_year}<span
                          >{candidate.publication_year}</span
                        >{/if}
                      <span class="classifications"
                        >{candidate.classifications.length
                          ? candidate.classifications.join("\n")
                          : "No classifications"}</span
                      >
                    </div>{/if}
                  <div class="candidate-actions">
                    <PrimaryButton
                      onclick={() => void applyCandidate(review)}
                      disabled={!!busyHash}
                      >{busyHash === review.content_hash
                        ? "Saving…"
                        : "Choose candidate"}</PrimaryButton
                    ><SecondaryButton
                      onclick={() => void rejectCandidates(review)}
                      disabled={!!busyHash}>No suitable edition</SecondaryButton
                    >
                  </div>
                </section>
              </div>
            {:else}
              <div class="simple-review">
                <div class="review-cover">
                  {#if coverUrls[review.content_hash]}<img
                      src={coverUrls[review.content_hash]}
                      alt=""
                    />{:else}<span>No cover</span>{/if}
                </div>
                <div class="simple-copy">
                  <div class="book-heading">
                    <div>
                      <span class={`status ${review.status}`}
                        >{review.status === "no_match" ? "No match" : review.status}</span
                      >
                      <h3>{review.title}</h3>
                    </div>
                    <code title={review.content_hash}
                      >{review.content_hash.slice(0, 10)}</code
                    >
                  </div>
                  <p class="detail">
                    {review.detail ||
                      (review.status === "failed"
                        ? "The metadata request did not complete."
                        : "The snapshot contained no supported edition match.")}
                  </p>
                  <div class="actions">
                    {#if review.status === "failed"}<SecondaryButton
                        onclick={() => void retry(review)}
                        disabled={!!busyHash}
                        >{busyHash === review.content_hash
                          ? "Retrying…"
                          : "Retry lookup"}</SecondaryButton
                      >{/if}
                    {#if review.status === "no_match"}<form
                        onsubmit={(event) => {
                          event.preventDefault();
                          void applyManualIsbn(review);
                        }}
                      >
                        <label
                          ><span>Known ISBN</span><input
                            inputmode="numeric"
                            autocomplete="off"
                            placeholder="ISBN-10 or ISBN-13"
                            value={manualIsbns[review.content_hash] ?? ""}
                            oninput={(event) =>
                              (manualIsbns = {
                                ...manualIsbns,
                                [review.content_hash]: event.currentTarget.value,
                              })}
                          /></label
                        ><FormButton disabled={!!busyHash}>Save ISBN</FormButton>
                      </form>{/if}
                  </div>
                </div>
              </div>
            {/if}
          </article>
        {/each}
      </div>
    </div>
  {/if}
</section>

<style>
  .metadata-review {
    display: grid;
    gap: 0.85rem;
    margin: 0 0 1.35rem;
    padding: 1rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-raised-bg);
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
  }
  h2,
  h3,
  p {
    margin: 0;
  }
  h2 {
    margin-top: 0.12rem;
    font: 680 1.15rem var(--font-interface-family);
  }
  h3 {
    overflow: hidden;
    margin-top: 0.3rem;
    color: var(--color-ink-text);
    font: 680 1rem var(--font-interface-family);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .eyebrow {
    color: var(--color-accent);
    font-size: 0.65rem;
    font-weight: 800;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }
  .description {
    margin-top: 0.28rem;
    color: var(--color-subtle-text);
    font-size: 0.76rem;
  }
  button,
  input {
    font: inherit;
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    cursor: wait;
    opacity: 0.55;
  }
  .tabs {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    border: 1px solid var(--color-border);
    border-radius: 0;
    overflow: hidden;
  }
  .tabs button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.45rem;
    min-height: 2.45rem;
    border: 0;
    border-right: 1px solid var(--color-border);
    color: var(--color-subtle-text);
    background: transparent;
    font-size: 0.72rem;
    font-weight: 720;
  }
  .tabs button:last-child {
    border-right: 0;
  }
  .tabs button.active {
    color: var(--color-ink-text);
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
  }
  .tabs strong {
    display: grid;
    min-width: 1.4rem;
    min-height: 1.4rem;
    padding: 0 0.3rem;
    place-content: center;
    border-radius: 0;
    color: var(--color-ink-text);
    background: var(--color-page-bg);
    font-size: 0.65rem;
  }
  .review-error {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0.65rem 0.75rem;
    border-radius: 0;
    color: var(--color-danger-text);
    background: var(--color-danger-bg);
    font-size: 0.72rem;
  }
  .review-state {
    display: grid;
    min-height: 7rem;
    place-content: center;
    color: var(--color-subtle-text);
    font-size: 0.78rem;
    text-align: center;
  }
  .review-viewport {
    height: min(784px, 68vh);
    min-height: 376px;
    overflow-y: auto;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
  }
  .review-canvas {
    position: relative;
    min-width: 0;
  }
  .review-item {
    position: absolute;
    inset: 0 0 auto;
    display: block;
    padding: 0.5625rem 0.625rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-page-bg);
    overflow: hidden;
    contain: strict;
  }
  .book-heading {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 0.75rem;
    align-items: start;
  }
  .book-heading code {
    max-width: 7rem;
    overflow: hidden;
    color: var(--color-subtle-text);
    font-size: 0.64rem;
    text-overflow: ellipsis;
  }
  .status {
    display: inline-flex;
    padding: 0.18rem 0.42rem;
    border-radius: 0;
    color: var(--color-subtle-text);
    background: var(--color-raised-bg);
    font-size: 0.58rem;
    font-weight: 820;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }
  .status.ambiguous {
    color: var(--color-warning-text, var(--color-accent));
  }
  .status.failed {
    color: var(--color-danger-text);
  }
  input {
    min-width: 0;
    min-height: 2.25rem;
    width: 9rem;
    padding: 0.35rem 0.55rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: var(--color-ink-text);
    background: var(--color-raised-bg);
  }
  .comparison {
    display: flex;
    align-items: start;
    gap: 0.5rem;
    width: 100%;
    height: 100%;
  }
  .comparison-column {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    height: 100%;
    overflow: hidden;
  }
  .comparison-column h4 {
    display: flex;
    align-items: center;
    flex: none;
    height: 2rem;
    margin: 0;
    font: 680 0.78rem var(--font-interface-family);
  }
  .comparison-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    height: 2rem;
    flex: none;
  }
  .candidate-controls {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    color: var(--color-subtle-text);
    font-size: 0.64rem;
    white-space: nowrap;
  }
  .local-evidence {
    display: flex;
    align-items: start;
    gap: 0.625rem;
    width: 100%;
    height: 20rem;
    min-width: 0;
    overflow: hidden;
  }
  .review-cover {
    display: grid;
    place-items: center;
    width: 12.5rem;
    height: 20rem;
    flex: none;
    overflow: hidden;
    color: var(--color-subtle-text);
    background: var(--color-raised-bg);
    font-size: 0.65rem;
  }
  .review-cover img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    object-position: center top;
  }
  .local-book .review-cover {
    max-width: 46%;
  }
  .metadata-values {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 0.3125rem;
    min-width: 0;
    height: 100%;
    overflow: hidden;
  }
  .metadata-values > strong,
  .metadata-values > span {
    display: -webkit-box;
    width: 100%;
    padding: 0.3125rem 0.4375rem;
    overflow: hidden;
    border: 1px solid var(--color-border);
    color: var(--color-subtle-text);
    font-size: 0.68rem;
    line-height: 1.25;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
  }
  .metadata-values > strong {
    color: var(--color-ink-text);
    font-weight: 680;
  }
  .metadata-values > .classifications {
    color: var(--color-accent);
    white-space: pre-line;
    line-clamp: 3;
    -webkit-line-clamp: 3;
  }
  .external-values {
    height: 17.5rem;
    flex: none;
  }
  .candidate-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    height: 2.5rem;
    padding-top: 0.5rem;
    flex: none;
  }
  .simple-review {
    display: flex;
    align-items: start;
    gap: 0.625rem;
    width: 100%;
    height: 100%;
  }
  .simple-copy {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 0.375rem;
    min-width: 0;
    height: 100%;
  }
  .detail {
    min-height: 2rem;
    overflow: hidden;
    color: var(--color-subtle-text);
    font-size: 0.7rem;
    line-height: 1.45;
  }
  .actions {
    display: flex;
    align-items: end;
    gap: 0.5rem;
    margin-top: auto;
  }
  form {
    display: flex;
    align-items: end;
    gap: 0.4rem;
    margin-left: auto;
  }
  form label {
    display: grid;
    gap: 0.2rem;
    color: var(--color-subtle-text);
    font-size: 0.6rem;
    font-weight: 750;
  }
  @media (max-width: 42rem) {
    .metadata-review {
      padding: 0.8rem;
    }
    .description {
      max-width: 20rem;
    }
    .simple-review > .review-cover {
      width: min(42%, 12.5rem);
    }
    .actions {
      flex-wrap: wrap;
      align-items: center;
    }
    form {
      width: 100%;
      margin-left: 0;
    }
    form label {
      flex: 1;
    }
    input {
      width: 100%;
    }
  }
</style>
