<script lang="ts">
  import { onMount } from "svelte";
  import type { BookDetail } from "../../lib/bindings";
  import type { BookCardData, DownloadState } from "../../lib/types";
  import { BROWSING_CONTENT_CONTRACT } from "../../lib/types";
  import { getThumbnailUrl } from "../../lib/thumbnail-cache";
  import { managedMenu } from "../../lib/keyboard-navigation";
  import { safeDescriptionHtml } from "../../lib/safe-description-html";
  import BookShelf from "./BookShelf.svelte";
  import BookTocTree from "./BookTocTree.svelte";

  let { detail, libraryId, openingBookId, downloadStates, onback, onread, onrelated, onauthor, onsubject, ondownload, onremove, ontrash, oncopy, onmove }:
    { detail: BookDetail; libraryId: string; openingBookId: string; downloadStates: Record<string, DownloadState>; onback: () => void; onread: (book: BookCardData, target?: string) => void | Promise<void>; onrelated: (book: BookCardData) => void | Promise<void>; onauthor: (id: string, name: string) => void | Promise<void>; onsubject: (path: string) => void | Promise<void>; ondownload: (book: BookCardData) => void | Promise<void>; onremove: (book: BookCardData) => void | Promise<void>; ontrash: (book: BookCardData) => void | Promise<void>; oncopy: (book: BookCardData) => void | Promise<void>; onmove: (book: BookCardData) => void | Promise<void> } = $props();

  let coverUrl = $state("");
  let tocExpanded = $state(false);
  const book = $derived(detail.book);
  const title = $derived(book.title || "Untitled");
  const progress = $derived(Math.min(100, Math.max(0, book.progress ?? 0)));
  const finished = $derived(progress >= BROWSING_CONTENT_CONTRACT.finished_progress_threshold);
  const descriptionHtml = $derived(safeDescriptionHtml(book.description));
  const subjects = $derived.by(() => {
    const values = detail.subject_paths.map((path) => {
      const parts = path.split(" / ").map((part) => part.trim()).filter(Boolean);
      return { path, leaf: parts.at(-1) || path, parent: parts.at(-2) || "" };
    });
    const leafCounts = new Map<string, number>();
    for (const value of values) leafCounts.set(value.leaf.toLocaleLowerCase(), (leafCounts.get(value.leaf.toLocaleLowerCase()) ?? 0) + 1);
    return values.map((value) => ({ ...value, label: (leafCounts.get(value.leaf.toLocaleLowerCase()) ?? 0) > 1 && value.parent ? `${value.leaf} · ${value.parent}` : value.leaf }))
      .sort((left, right) => left.label.localeCompare(right.label, undefined, { sensitivity: "base", numeric: true }));
  });

  onMount(() => { void getThumbnailUrl(libraryId, book.content_hash).then((url) => (coverUrl = url ?? "")); });
  function closeMenu(event: MouseEvent) { (event.currentTarget as HTMLElement).closest("details")?.removeAttribute("open"); }
</script>

<div class="mobile-detail-bar">
  <button type="button" aria-label="Back to library" onclick={onback}>←</button>
  <strong>{title}</strong>
  <details use:managedMenu><summary aria-label={`Actions for ${title}`}>⋮</summary><div class="menu" role="menu">
    <button type="button" role="menuitem" onclick={(e) => { closeMenu(e); return oncopy(book); }}>Add to another folder…</button>
    <button type="button" role="menuitem" onclick={(e) => { closeMenu(e); return onmove(book); }}>Move to folder…</button>
    {#if book.downloaded}<button type="button" role="menuitem" onclick={(e) => { closeMenu(e); return onremove(book); }}>Remove download</button>
    {:else}<button type="button" role="menuitem" onclick={(e) => { closeMenu(e); return ondownload(book); }}>Download for offline use</button>{/if}
    <button class="danger" type="button" role="menuitem" onclick={(e) => { closeMenu(e); return ontrash(book); }}>Move to Trash</button>
  </div></details>
</div>

<article class="book-detail">
  <div class="hero">
    <div class="cover-column">
      <button class="cover-button" class:has-cover={Boolean(coverUrl)} type="button" aria-label={`Open ${title}`} disabled={openingBookId === book.content_hash} onclick={() => onread(book)}>
        {#if coverUrl}<img src={coverUrl} alt={`Cover of ${title}`} />{:else}<span aria-hidden="true">⌑</span>{/if}
      </button>
      {#if progress > 0}
        <div class="reading-progress" role="progressbar" aria-label={finished ? "Finished" : `${Math.round(progress)}% read`} aria-valuenow={Math.round(progress)} aria-valuemin="0" aria-valuemax="100">
          <div><i style={`width:${finished ? 100 : progress}%`}></i></div><span>{finished ? "Finished" : `${Math.round(progress)}%`}</span>
        </div>
      {/if}
    </div>
    <div class="copy-column">
      <header class="title-row">
        <div><h1>{title}</h1><p class="authors">
          {#if detail.authors.length}{#each detail.authors as author, index (author.id)}{#if index}, {/if}<button type="button" onclick={() => onauthor(author.id, author.name)}>{author.name}</button>{/each}
          {:else}{book.author || "Unknown author"}{/if}
        </p></div>
        <details class="desktop-actions" use:managedMenu><summary aria-label={`Actions for ${title}`}>⋮</summary><div class="menu" role="menu">
          <button type="button" role="menuitem" onclick={(e) => { closeMenu(e); return oncopy(book); }}>Add to another folder…</button>
          <button type="button" role="menuitem" onclick={(e) => { closeMenu(e); return onmove(book); }}>Move to folder…</button>
          {#if book.downloaded}<button type="button" role="menuitem" onclick={(e) => { closeMenu(e); return onremove(book); }}>Remove download</button>
          {:else}<button type="button" role="menuitem" onclick={(e) => { closeMenu(e); return ondownload(book); }}>Download for offline use</button>{/if}
          <button class="danger" type="button" role="menuitem" onclick={(e) => { closeMenu(e); return ontrash(book); }}>Move to Trash</button>
        </div></details>
      </header>
      {#if subjects.length}<div class="subjects" aria-label="Subjects">{#each subjects as subject (subject.path)}<button type="button" aria-label={subject.path} title={subject.path} onclick={() => onsubject(subject.path)}>{subject.label}</button>{/each}</div>{/if}
    </div>
  </div>

  {#if descriptionHtml}<div class="description">{@html descriptionHtml}</div>{/if}

  <section class="toc">
    <button class="toc-heading" type="button" aria-expanded={tocExpanded} onclick={() => (tocExpanded = !tocExpanded)}><span aria-hidden="true">{tocExpanded ? "⌄" : "›"}</span><h2>Table of contents</h2></button>
    {#if tocExpanded}<div class="toc-body">{#if detail.toc.length}<BookTocTree entries={detail.toc} onopen={(target) => onread(book, target)} />{:else}<p>No table of contents available.</p>{/if}</div>{/if}
  </section>
  {#if detail.related.length}<BookShelf id={`related-${book.content_hash}`} title={detail.authors.length === 1 ? `More by ${detail.authors[0].name}` : "More by these authors"} books={detail.related} {libraryId} emptyMessage="" {openingBookId} {downloadStates} onopen={onrelated} {ondownload} {onremove} {ontrash} />{/if}
</article>

<style>
  .mobile-detail-bar { display: grid; grid-template-columns: 2.75rem minmax(0,1fr) 2.75rem; align-items: center; min-height: 3.5rem; margin: -1.1rem -1rem 1.25rem; padding: env(safe-area-inset-top) .45rem 0; border-bottom: 1px solid var(--color-border); background: var(--color-surface-bg); }
  .mobile-detail-bar > button, summary { display: grid; place-items: center; min-width: 2.75rem; min-height: 2.75rem; border: 0; color: inherit; background: transparent; cursor: pointer; font: 600 1.25rem var(--font-interface-family); list-style: none; }
  summary::-webkit-details-marker { display:none; } .mobile-detail-bar strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align:center; font-size:.82rem; }
  details { position: relative; } .menu { position:absolute; z-index:12; top:100%; right:0; display:grid; width:max-content; min-width:13rem; padding:.35rem; border:1px solid var(--color-border); border-radius: 0; background:var(--color-raised-bg); box-shadow:0 .8rem 2rem var(--color-box-shadow); }
  .menu button { min-height:2.75rem; padding:.55rem .75rem; border:0; border-radius: 0; color:var(--color-ink-text); background:transparent; text-align:left; cursor:pointer; font:650 .76rem var(--font-interface-family); } .menu button:hover { background-image:linear-gradient(var(--color-hover-overlay),var(--color-hover-overlay)); } .menu .danger { color:var(--color-danger-text); }
  .book-detail { width:100%; max-width:calc(var(--book-cover-width) + 2rem + 80ch); margin:0 auto; padding-bottom:3rem; }
  .hero { display:grid; grid-template-columns:minmax(7.5rem, 38vw) minmax(0,1fr); align-items:start; gap:1.25rem; }
  .cover-button { display:grid; place-items:center; width:100%; aspect-ratio:5/8; padding:0; overflow:hidden; border:0; border-radius: 0; color:var(--color-subtle-text); background:var(--color-raised-bg); box-shadow:0 .55rem 1.5rem var(--color-box-shadow); cursor:pointer; }
  .cover-button.has-cover { aspect-ratio:auto; background:transparent; }
  .cover-button img { display:block; width:100%; height:auto; } .cover-button span { font-size:2.5rem; } .cover-button:hover { outline:2px solid color-mix(in srgb,var(--color-accent) 45%,transparent); outline-offset:3px; } .cover-button:active { transform:scale(.99); }
  .reading-progress { display:grid; gap:.35rem; margin-top:.8rem; color:var(--color-subtle-text); font-size:.72rem; } .reading-progress > div { height:.22rem; overflow:hidden; border-radius: 0; background:var(--color-border); } .reading-progress i { display:block; height:100%; background:var(--color-accent); }
  .title-row { display:flex; align-items:flex-start; justify-content:space-between; gap:1rem; } h1 { margin:0; font:650 clamp(1.35rem,4.2vw,2.2rem)/1.08 var(--font-interface-family); letter-spacing:-.035em; } .authors { margin:.45rem 0 0; color:var(--color-subtle-text); } .authors button { padding:0; border:0; color:inherit; background:transparent; cursor:pointer; font:inherit; } .authors button:hover { color:var(--color-accent); text-decoration:underline; }
  .desktop-actions { display:none; } .description { margin-top:1.2rem; color:var(--color-ink-text); font:400 1rem/1.65 var(--font-reading-family, serif); text-align:justify; hyphens:auto; }
  .description :global(p) { margin-block: 1em; margin-inline: 0; }
  .description :global(h1), .description :global(h2), .description :global(h3), .description :global(h4), .description :global(h5), .description :global(h6) { margin: 1em 0 .4em; line-height: 1.25; }
  .description :global(ul), .description :global(ol) { margin: .55em 0; padding-left: 1.5em; }
  .description :global(:first-child) { margin-top: 0; }
  .description :global(:last-child) { margin-bottom: 0; }
  .description :global(blockquote) { margin: .65em 0 .65em 1.25rem; padding-left: .85rem; border-left: 2px solid var(--color-border); }
  .description :global(pre) { margin: .65em 0; overflow-x: auto; white-space: pre-wrap; text-align: left; }
  .subjects { display:flex; flex-wrap:wrap; gap:.45rem; margin-top:1.25rem; } .subjects button { min-height:2.2rem; padding:.35rem .7rem; border:1px solid var(--color-border); border-radius: 0; color:var(--color-subtle-text); background:var(--color-raised-bg); cursor:pointer; font:.68rem var(--font-interface-family); } .subjects button:hover { color:var(--color-accent); border-color:var(--color-accent); }
  .toc { margin-top:2.4rem; overflow:hidden; border:1px solid var(--color-border); border-radius: 0; } .toc-heading { display:flex; align-items:center; gap:.65rem; width:100%; min-height:3rem; padding:.55rem .85rem; border:0; color:inherit; background:transparent; cursor:pointer; text-align:left; } .toc-heading span { width:1rem; color:var(--color-subtle-text); } h2 { margin:0; font:600 1.05rem var(--font-interface-family); letter-spacing:-.02em; } .toc-body { padding:.45rem .65rem .75rem; border-top:1px solid var(--color-border); } .toc-body > p { color:var(--color-subtle-text); }
  @media (min-width:45rem) { .mobile-detail-bar { display:none; } .hero { grid-template-columns:var(--book-cover-width) minmax(0,80ch); gap:2rem; } .desktop-actions { display:block; flex:none; } .desktop-actions summary { border:1px solid var(--color-border); border-radius:50%; font-size:1.15rem; } .description { margin-top:1.5rem; } }
  @media (max-width:34rem) { .hero { grid-template-columns:minmax(7rem, 34vw) minmax(0,1fr); } }
  @media (prefers-reduced-motion:no-preference) { .cover-button { transition:outline-color .12s ease, transform .08s ease; } }
</style>
