<script lang="ts">
  import { STORE_CONTRACT } from "../../lib/types";
  import type { CatalogPublication, CatalogPublicationAction } from "../../lib/bindings";
  import CatalogCover from "./CatalogCover.svelte";

  let { sourceId, publication, action, actionState = "idle", actionError = "", onaction }: {
    sourceId: string;
    publication: CatalogPublication;
    action: CatalogPublicationAction | undefined;
    actionState?: "idle" | "adding" | "added";
    actionError?: string;
    onaction: (action: CatalogPublicationAction) => void | Promise<void>;
  } = $props();
  const hasCover = $derived(Boolean(publication.image_url));
  const descriptionText = $derived(publication.description?.format === "plain_text" ? publication.description.text : publication.description?.plain_text ?? "");
</script>

{#snippet publicationAction()}
  {#if action}
    <button class="publication-action" class:external-action={action.kind === "external"} type="button" disabled={action.kind === "import" && actionState === "adding"} onclick={() => void onaction(action)}>
      {#if action.kind === "import" && actionState === "adding"}<span class="button-progress" aria-hidden="true"></span>{/if}
      {actionError ? STORE_CONTRACT.retry_label : action.kind === "import" ? actionState === "adding" ? STORE_CONTRACT.adding_label : STORE_CONTRACT.add_label : STORE_CONTRACT.open_label}
      {#if action.kind === "external"}<span aria-hidden="true">↗</span>{/if}
    </button>
  {/if}
{/snippet}

<article class="catalog-book" class:without-cover={!hasCover}>
  {#if hasCover}<div class="store-book-identity">
    <div class="remote-cover"><CatalogCover {sourceId} imageUrl={publication.image_url} title={publication.title} /></div>
    <h2>{publication.title}</h2>
    <p>{publication.authors.map((author) => author.name).join(", ") || "Unknown author"}</p>
  </div>{/if}
  <div class="store-book-detail">
    {#if !hasCover}<div class="metadata-only"><h2>{publication.title}</h2><p>{publication.authors.map((author) => author.name).join(", ") || "Unknown author"}</p></div>{/if}
    {#if descriptionText}<div class="book-description" aria-label={`Description of ${publication.title}`}>{descriptionText}</div>{/if}
    {#if actionError}<p class="publication-error" role="alert">{actionError}</p>{/if}
    {@render publicationAction()}
  </div>
</article>

<style>
  .catalog-book { display: flex; gap: 16px; min-width: var(--store-publication-min-width); height: var(--store-publication-height); overflow: hidden; border: 1px solid var(--color-border); border-radius: 0; background: var(--color-raised-bg); }
  .catalog-book.without-cover { gap: 0; }
  .store-book-identity { display: flex; width: var(--book-cover-width); min-width: 0; height: var(--store-publication-height); flex-direction: column; }
  .remote-cover { position: relative; display: grid; width: var(--book-cover-width); height: var(--book-cover-height); flex: none; place-items: center; overflow:hidden; border-radius: 0; color:var(--color-accent); background:var(--color-active-highlight); font:700 1.4rem var(--font-interface-family); }
  .store-book-identity h2, .store-book-identity p { width: var(--book-cover-width); overflow: hidden; text-overflow: ellipsis; }
  .store-book-identity h2 { display: -webkit-box; min-height: 2.3em; line-height: 1.15; white-space: normal; line-clamp: 2; -webkit-box-orient: vertical; -webkit-line-clamp: 2; }
  .store-book-identity p { white-space: nowrap; }
  .store-book-identity h2 { height: 42px; margin: 9px 0 0; padding: 0 2px; color: var(--color-ink-text); font: 600 17px/1.15 var(--font-interface-family); }
  .store-book-identity p { height: 22px; margin: 0; padding: 0 2px; color: var(--color-subtle-text); font: 400 15px/22px var(--font-interface-family); }
  .store-book-detail { display: flex; flex: 1; min-width: 0; min-height: 0; flex-direction: column; gap: 10px; padding: 16px 16px 16px 0; }
  .without-cover .store-book-detail { padding-left: 16px; }
  .metadata-only { display: grid; gap: 4px; padding-bottom: 10px; border-bottom: 1px solid var(--color-border); }
  .metadata-only h2, .metadata-only p { margin: 0; overflow: hidden; text-overflow: ellipsis; }
  .metadata-only h2 { display: -webkit-box; font: 600 17px/1.15 var(--font-interface-family); line-clamp: 2; -webkit-box-orient: vertical; -webkit-line-clamp: 2; }
  .metadata-only p { color: var(--color-subtle-text); font: 400 15px/22px var(--font-interface-family); white-space: nowrap; }
  .book-description { display: block; flex: 1; min-height: 0; padding-right: 4px; overflow-y: auto; overscroll-behavior: contain; scrollbar-color: var(--color-border) transparent; scrollbar-gutter: stable; scrollbar-width: thin; color: var(--color-subtle-text); font-size: 13px; line-height: 1.35; }
  .book-description::-webkit-scrollbar { width: var(--store-scrollbar-width); }
  .book-description::-webkit-scrollbar-track { background: transparent; }
  .book-description::-webkit-scrollbar-thumb { border: 2px solid transparent; border-radius: 0; background: var(--color-border); background-clip: padding-box; }
  .book-description :global(*) { max-width: 100%; }
  .book-description :global(p), .book-description :global(h1), .book-description :global(h2), .book-description :global(h3), .book-description :global(h4), .book-description :global(h5), .book-description :global(h6), .book-description :global(blockquote), .book-description :global(pre), .book-description :global(ul), .book-description :global(ol), .book-description :global(dl) { margin: 0; }
  .book-description :global(p + p), .book-description :global(div + div), .book-description :global(* + ul), .book-description :global(* + ol) { margin-top: .55em; }
  .book-description :global(ul), .book-description :global(ol) { padding-left: 1.15rem; }
  .publication-error { max-height: 48px; margin: 0; overflow-y: auto; color: var(--color-danger-text); font-size: 11px; line-height: 1.35; }
  .publication-action { display: inline-flex; flex: none; align-self: end; align-items: center; justify-content: center; gap: 6px; min-width: 0; height: 30px; margin-top: auto; padding: 0 9px; border: 1px solid var(--color-accent); border-radius: 0; color: var(--color-raised-bg); background: var(--color-accent); font-size: 12px; font-weight: 750; cursor: pointer; }
  .publication-action.external-action { color: var(--color-accent); background: transparent; }
  .button-progress { width: 14px; height: 14px; border: 2px solid color-mix(in srgb, currentColor 35%, transparent); border-top-color: currentColor; border-radius: 50%; animation: spin .8s linear infinite; }
  button:disabled { cursor: default; opacity: .55; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @container (max-width: 719px) { .catalog-book { width: 100%; min-width: 0; } }
</style>
