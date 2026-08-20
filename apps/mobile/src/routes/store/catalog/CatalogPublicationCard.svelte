<script lang="ts">
  import { STORE_CONTRACT } from "../../../lib/shared/types";
  import type { CatalogImportAction } from "./actions";
  import type { CatalogPublication } from "../../../lib/bindings";
  import CatalogCover from "./CatalogCover.svelte";
  import PrimaryButton from "../../../components/shared/buttons/PrimaryButton.svelte";

  let {
    sourceId,
    publication,
    action,
    actionState = "idle",
    actionError = "",
    onaction,
  }: {
    sourceId: string;
    publication: CatalogPublication;
    action: CatalogImportAction | undefined;
    actionState?: "idle" | "adding" | "added";
    actionError?: string;
    onaction: (action: CatalogImportAction) => void | Promise<void>;
  } = $props();
  const DESCRIPTION_MIN_CHARACTER_COLUMNS = 30;
  const DESCRIPTION_MAX_CHARACTER_COLUMNS = 80;
  const DESCRIPTION_PAGE_LINES = 12;
  let descriptionPage = $state(0);
  let descriptionCharacterLimit = $state(
    DESCRIPTION_MIN_CHARACTER_COLUMNS * DESCRIPTION_PAGE_LINES,
  );
  const hasCover = $derived(Boolean(publication.image_url));
  const descriptionText = $derived(
    publication.description?.format === "plain_text"
      ? publication.description.text
      : (publication.description?.plain_text ?? ""),
  );
  const descriptionPages = $derived(
    paginateDescription(descriptionText, descriptionCharacterLimit),
  );
  const currentDescriptionPage = $derived(
    Math.min(descriptionPage, Math.max(0, descriptionPages.length - 1)),
  );
  const coverActionLabel = $derived(
    action?.kind === "import" ? `Add ${publication.title}` : `Open ${publication.title}`,
  );

  function runCoverAction() {
    if (!action || actionState === "adding") return;
    return onaction(action);
  }

  function paginateDescription(text: string, characterLimit: number) {
    const pages: string[] = [];
    let page = "";
    for (const word of text.trim().split(/\s+/).filter(Boolean)) {
      if (page && [...page, " ", word].join("").length > characterLimit) {
        pages.push(page);
        page = "";
      }
      page += `${page ? " " : ""}${word}`;
    }
    if (page) pages.push(page);
    return pages;
  }

  function measureDescription(node: HTMLElement) {
    function updatePageSize() {
      const style = getComputedStyle(node);
      const fontSize = Number.parseFloat(style.fontSize) || 16;
      const lineHeight = Number.parseFloat(style.lineHeight) || fontSize * 1.35;
      const columns = Math.max(
        8,
        Math.min(
          DESCRIPTION_MAX_CHARACTER_COLUMNS,
          Math.floor(node.clientWidth / (fontSize * 0.55)),
        ),
      );
      const lines = Math.max(
        1,
        Math.min(DESCRIPTION_PAGE_LINES, Math.floor(node.clientHeight / lineHeight)),
      );
      descriptionCharacterLimit = columns * lines;
    }
    const observer = new ResizeObserver(updatePageSize);
    observer.observe(node);
    updatePageSize();
    return { destroy: () => observer.disconnect() };
  }
</script>

{#snippet publicationAction(
  action: CatalogImportAction | undefined,
  actionState: "idle" | "adding" | "added",
  actionError: string,
)}
  {#if action?.kind === "import"}
    {@const label = actionError
      ? STORE_CONTRACT.retry_label
      : action.kind === "import"
        ? actionState === "adding"
          ? STORE_CONTRACT.adding_label
          : STORE_CONTRACT.add_label
        : STORE_CONTRACT.open_label}
    <PrimaryButton
      disabled={actionState === "adding"}
      onclick={() => void onaction(action)}
    >
      {#if actionState === "adding"}<span
          class="button-progress"
          aria-hidden="true"
        ></span>{/if}
      {label}
    </PrimaryButton>
  {/if}
{/snippet}

<article
  class="catalog-book"
  class:without-cover={!hasCover}
  class:without-description={!descriptionPages.length}
>
  {#if hasCover}<div class="store-book-identity">
      <div class="remote-cover">
        <CatalogCover
          {sourceId}
          imageUrl={publication.image_url}
          title={publication.title}
          disabled={!action || actionState === "adding"}
          ariaLabel={coverActionLabel}
          onclick={runCoverAction}
        />
        {#if !descriptionPages.length}
          <div class="identity-controls">
            {@render publicationAction(action, actionState, actionError)}
          </div>
          {#if actionError}<div
              class="publication-error identity-error"
              role="alert"
            >
              {actionError}
            </div>{/if}
        {/if}
      </div>
      <h2>{publication.title}</h2>
      <p>
        {publication.authors.map((author) => author.name).join(", ") || "Unknown author"}
      </p>
    </div>{/if}
  {#if !hasCover || descriptionPages.length}<div class="store-book-detail">
      {#if !hasCover}<div class="metadata-only">
          <h2>{publication.title}</h2>
          <p>
            {publication.authors.map((author) => author.name).join(", ") ||
              "Unknown author"}
          </p>
        </div>{/if}
      {#if descriptionPages.length}<div
          class="book-description"
          aria-label={`Description of ${publication.title}`}
          use:measureDescription
        >
          {descriptionPages[currentDescriptionPage]}
        </div>{/if}
      {#if actionError}<p
          class="publication-error"
          role="alert"
        >
          {actionError}
        </p>{/if}
      {#if descriptionPages.length > 1 || action}
        <div class="publication-controls">
          {#if descriptionPages.length > 1}
            {#if currentDescriptionPage > 0}<button
                class="description-page"
                type="button"
                aria-label="Previous description page"
                onclick={() =>
                  (descriptionPage = Math.max(0, currentDescriptionPage - 1))}>‹</button
              >{/if}
            {#if currentDescriptionPage + 1 < descriptionPages.length}<button
                class="description-page"
                type="button"
                aria-label="Next description page"
                onclick={() =>
                  (descriptionPage = Math.min(
                    descriptionPages.length - 1,
                    currentDescriptionPage + 1,
                  ))}>›</button
              >{/if}
          {/if}
          {@render publicationAction(action, actionState, actionError)}
        </div>
      {/if}
    </div>{/if}
</article>

<style>
  .catalog-book {
    display: flex;
    gap: 16px;
    width: min(100%, calc(var(--book-cover-width) + 80ch + 48px));
    min-width: min(
      100%,
      max(
        var(--store-publication-min-width),
        calc(var(--book-cover-width) + 20rem + 48px)
      )
    );
    height: calc(var(--book-cover-height) + 3.81875rem + 15px);
    overflow: hidden;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-surface-bg);
  }
  .catalog-book.without-description {
    width: var(--book-cover-width);
    min-width: var(--book-cover-width);
  }
  .catalog-book.without-cover {
    gap: 0;
  }
  .store-book-identity {
    display: flex;
    width: var(--book-cover-width);
    min-width: 0;
    height: 100%;
    flex-direction: column;
  }
  .remote-cover {
    position: relative;
    display: grid;
    width: var(--book-cover-width);
    height: var(--book-cover-height);
    flex: none;
    place-items: center;
    overflow: hidden;
    border-radius: 0;
    color: var(--color-accent);
    background: var(--color-active-highlight);
    font: 700 1.4rem var(--font-interface-family);
  }
  .store-book-identity h2,
  .store-book-identity p {
    width: var(--book-cover-width);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .store-book-identity h2 {
    display: -webkit-box;
    height: 2.3em;
    line-height: 1.15;
    white-space: normal;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
  }
  .store-book-identity p {
    white-space: nowrap;
  }
  .store-book-identity h2 {
    margin: 9px 0 0;
    padding: 0 2px;
    color: var(--color-ink-text);
    font-size: 1.0625rem;
    font-weight: 600;
  }
  .store-book-identity p {
    margin: 2px 0 0;
    padding: 0 2px;
    color: var(--color-subtle-text);
    font-size: 0.9375rem;
    line-height: 1.375rem;
  }
  .identity-controls {
    position: absolute;
    right: 4px;
    bottom: 4px;
    display: flex;
    justify-content: flex-end;
  }
  .identity-error {
    position: absolute;
    right: 4px;
    bottom: calc(2.125rem + 8px);
    left: 4px;
    padding: 3px;
    background: color-mix(in srgb, var(--color-raised-bg) 92%, transparent);
  }
  .store-book-detail {
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
    flex-direction: column;
    gap: 0;
    padding: 16px 16px 4px 0;
  }
  .without-cover .store-book-detail {
    padding-left: 16px;
  }
  .metadata-only {
    display: grid;
    gap: 2px;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--color-border);
  }
  .metadata-only h2,
  .metadata-only p {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .metadata-only h2 {
    display: -webkit-box;
    font-size: 1.0625rem;
    font-weight: 600;
    line-height: 1.15;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
  }
  .metadata-only p {
    color: var(--color-subtle-text);
    font-size: 0.9375rem;
    line-height: 1.375rem;
    white-space: nowrap;
  }
  .book-description {
    display: block;
    flex: 1;
    min-height: 0;
    max-width: 80ch;
    overflow: hidden;
    color: var(--color-subtle-text);
    font-size: 1rem;
    line-height: 1.35;
    text-align: justify;
    text-align-last: left;
    hyphens: auto;
  }
  .book-description :global(*) {
    max-width: 100%;
  }
  .book-description :global(p),
  .book-description :global(h1),
  .book-description :global(h2),
  .book-description :global(h3),
  .book-description :global(h4),
  .book-description :global(h5),
  .book-description :global(h6),
  .book-description :global(blockquote),
  .book-description :global(pre),
  .book-description :global(ul),
  .book-description :global(ol),
  .book-description :global(dl) {
    margin: 0;
  }
  .book-description :global(p + p),
  .book-description :global(div + div),
  .book-description :global(* + ul),
  .book-description :global(* + ol) {
    margin-top: 0.55em;
  }
  .book-description :global(ul),
  .book-description :global(ol) {
    padding-left: 1.15rem;
  }
  .publication-error {
    max-height: 48px;
    margin: 0;
    overflow-y: auto;
    color: var(--color-danger-text);
    font-size: 11px;
    line-height: 1.35;
  }
  .publication-controls {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    width: 100%;
    margin-top: auto;
  }
  .description-page {
    display: inline-grid;
    width: 2.125rem;
    height: 2.125rem;
    padding: 0;
    place-items: center;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: var(--color-accent);
    background: var(--color-raised-bg);
    font-size: 1.35rem;
    line-height: 1;
    cursor: pointer;
  }
  .button-progress {
    width: 14px;
    height: 14px;
    border: 2px solid color-mix(in srgb, currentColor 35%, transparent);
    border-top-color: currentColor;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @container (max-width: 719px) {
    .catalog-book {
      width: 100%;
      min-width: 0;
    }
  }
</style>
