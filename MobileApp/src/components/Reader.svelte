<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import designTokens from "../../../FrontendContract/design_tokens.json";

  // Foliate is vendored as a pinned submodule. Importing view.js registers the
  // <foliate-view> custom element and keeps the unstable API boundary here.
  import "../vendor/foliate-js/view.js";
  import { DEFAULT_READER_PREFERENCES, READER_PREFERENCES_CONTRACT } from "../lib/types";
  import { enableVolumePageTurning, onVolumePageTurn } from "../lib/volume-navigation";
  import type {
    FoliateRelocateDetail,
    FoliateRenderer,
    FoliateSearchGroup,
    FoliateSearchMatch,
    FoliateTocItem,
    FoliateView,
    ReaderBook,
    ReaderFlow,
    ReaderFontFamily,
    ReaderPreferencePatch,
    ReaderPreferences,
    ReaderTextAlignment,
    ReaderTheme,
  } from "../lib/types";

  let {
    book,
    preferences,
    onpreferenceschange,
    onclose,
  }: {
    book: ReaderBook;
    preferences: ReaderPreferences;
    onpreferenceschange: (patch: ReaderPreferencePatch) => void | Promise<void>;
    onclose: () => void;
  } = $props();

  let host: HTMLDivElement;
  let view: FoliateView | null = null;
  let loading = $state(true);
  let error = $state("");
  let settings = $state<ReaderPreferences>({ ...DEFAULT_READER_PREFERENCES });
  let settingsOpen = $state(false);
  let tocOpen = $state(false);
  let searchOpen = $state(false);
  let footnoteOpen = $state(false);
  let footnoteHost = $state<HTMLDivElement>();
  let footnoteLabel = $state("Note");
  let footnoteView: FoliateView | null = null;
  let tocItems = $state<FlatTocItem[]>([]);
  let activeTocHref = $state("");
  let searchQuery = $state("");
  let searchResults = $state<SearchResult[]>([]);
  let searchProgress = $state(0);
  let searching = $state(false);
  let searchError = $state("");
  let saveState = $state<"idle" | "saving" | "saved" | "error">("idle");

  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  const preferenceConstraints = READER_PREFERENCES_CONTRACT.constraints;
  let lastCfi = "";
  let lastPersistedCfi = "";
  let positionSaveQueue = Promise.resolve();
  let searchGeneration = 0;
  let footnoteHandler: {
    handle: (book: unknown, event: Event) => void | Promise<void>;
    addEventListener: EventTarget["addEventListener"];
    removeEventListener: EventTarget["removeEventListener"];
  } | null = null;

  interface FlatTocItem {
    label: string;
    href?: string;
    depth: number;
  }

  interface SearchResult extends FoliateSearchMatch {
    section: string;
  }

  function flattenToc(items: FoliateTocItem[] = [], depth = 0, result: FlatTocItem[] = []) {
    for (const item of items) {
      if (item.label) result.push({ label: item.label, href: item.href || undefined, depth });
      if (item.subitems?.length) flattenToc(item.subitems, depth + 1, result);
    }
    return result;
  }

  $effect(() => {
    const next = { ...preferences };
    settings = next;
    applyReaderPreferences(next);
  });

  function readerStyles(readerPreferences: ReaderPreferences) {
    const colors = designTokens.reader_themes[readerPreferences.theme];
    const family = designTokens.font_families[readerPreferences.font_family];
    const familyRule = family ? `font-family: ${family} !important;` : "";
    const alignmentRule = readerPreferences.text_alignment === "publisher" ? "" : `text-align: ${readerPreferences.text_alignment} !important;`;
    const hyphens = readerPreferences.hyphenation ? "auto" : "manual";
    return `
      :root { color-scheme: ${readerPreferences.theme === "dark" ? "dark" : "light"}; font-size: ${readerPreferences.font_size_px}px !important; }
      html, body { background: ${colors.background} !important; color: ${colors.foreground} !important; ${familyRule} }
      p, li, blockquote, dd { line-height: ${readerPreferences.line_height} !important; ${alignmentRule} -webkit-hyphens: ${hyphens}; hyphens: ${hyphens}; }
      a { color: ${colors.link} !important; }
      img, svg { max-width: 100% !important; max-height: 95vh !important; object-fit: contain; }
    `;
  }

  function applyPreferencesToRenderer(renderer: FoliateRenderer | undefined, readerPreferences: ReaderPreferences) {
    if (!renderer) return;
    renderer.setAttribute("flow", readerPreferences.flow);
    renderer.setAttribute("max-inline-size", `${readerPreferences.max_line_width_px}px`);
    renderer.setAttribute("max-column-count", String(readerPreferences.max_column_count));
    renderer.setAttribute("zoom", String(readerPreferences.zoom));
    renderer.setStyles?.(readerStyles(readerPreferences));
  }

  function applyReaderPreferences(readerPreferences = settings) {
    applyPreferencesToRenderer(view?.renderer, readerPreferences);
    applyPreferencesToRenderer(footnoteView?.renderer, readerPreferences);
  }

  function persistCfi(cfi: string) {
    if (!cfi || cfi === lastPersistedCfi) return positionSaveQueue;
    saveState = "saving";
    positionSaveQueue = positionSaveQueue.then(async () => {
      try {
        await book.saveCfi(cfi);
        lastPersistedCfi = cfi;
        saveState = "saved";
      } catch (reason) {
        console.error("Could not save reading position", reason);
        saveState = "error";
      }
    });
    return positionSaveQueue;
  }

  function scheduleSave(cfi: string) {
    lastCfi = cfi;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => void persistCfi(cfi), 600);
  }

  async function flushPosition() {
    if (saveTimer) {
      clearTimeout(saveTimer);
      saveTimer = undefined;
    }
    if (lastCfi && lastCfi !== lastPersistedCfi) await persistCfi(lastCfi);
    else await positionSaveQueue;
  }

  function handleRelocate(event: Event) {
    const detail = (event as CustomEvent<FoliateRelocateDetail>).detail;
    activeTocHref = detail.tocItem?.href || "";
    if (detail.cfi) scheduleSave(detail.cfi);
  }

  function updatePreference<K extends keyof ReaderPreferences>(key: K, value: ReaderPreferences[K]) {
    settings = { ...settings, [key]: value };
    applyReaderPreferences();
    void onpreferenceschange({ preference: key, value } as ReaderPreferencePatch);
  }

  function changeFont(delta: number) {
    const [minimum, maximum] = READER_PREFERENCES_CONTRACT.constraints.font_size_px;
    updatePreference("font_size_px", Math.max(minimum, Math.min(maximum, settings.font_size_px + delta)));
  }

  function closePanels() {
    settingsOpen = false;
    tocOpen = false;
    searchOpen = false;
  }

  function togglePanel(panel: "toc" | "search" | "settings") {
    const next = panel === "toc" ? !tocOpen : panel === "search" ? !searchOpen : !settingsOpen;
    closePanels();
    if (panel === "toc") tocOpen = next;
    if (panel === "search") searchOpen = next;
    if (panel === "settings") settingsOpen = next;
  }

  async function goToTocItem(item: FlatTocItem) {
    if (!item.href || !view) return;
    await view.goTo(item.href);
    closePanels();
  }

  async function runSearch(event?: SubmitEvent) {
    event?.preventDefault();
    const query = searchQuery.trim();
    const currentView = view;
    const generation = ++searchGeneration;
    searchResults = [];
    searchProgress = 0;
    searchError = "";
    currentView?.clearSearch();
    if (!query || !currentView) return;

    searching = true;
    try {
      for await (const result of currentView.search({ query })) {
        if (generation !== searchGeneration) return;
        if (typeof result === "string") continue;
        if ("progress" in result) {
          searchProgress = result.progress;
          continue;
        }
        const group = result as FoliateSearchGroup;
        searchResults = [
          ...searchResults,
          ...group.subitems.map((match) => ({ ...match, section: group.label || "Book" })),
        ];
      }
    } catch (reason) {
      searchError = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (generation === searchGeneration) {
        searching = false;
        searchProgress = 1;
      }
    }
  }

  async function selectSearchResult(result: SearchResult) {
    await view?.select(result.cfi);
    searchOpen = false;
  }

  function clearSearch() {
    searchGeneration += 1;
    searching = false;
    searchResults = [];
    searchProgress = 0;
    searchError = "";
    view?.clearSearch();
  }

  async function openExternalLink(event: Event) {
    event.preventDefault();
    const href = (event as CustomEvent<{ href_?: string }>).detail?.href_;
    if (!href) return;
    try {
      await openUrl(href);
    } catch (reason) {
      console.warn("Could not open link with Tauri; falling back to the browser", reason);
      window.open(href, "_blank", "noopener,noreferrer");
    }
  }

  function handleInternalLink(event: Event) {
    const handler = footnoteHandler;
    const foliateBook = view?.book;
    if (!handler || !foliateBook) return;
    const handled = handler.handle(foliateBook, event);
    if (handled) Promise.resolve(handled).catch((reason) => console.warn("Could not open footnote", reason));
  }

  async function showFootnote(event: Event) {
    const detail = (event as CustomEvent<{ view: FoliateView; type?: string }>).detail;
    closeFootnote();
    footnoteView = detail.view;
    footnoteView.classList.add("footnote-view");
    footnoteLabel = detail.type ? detail.type.replace(/^./, (letter) => letter.toUpperCase()) : "Note";
    footnoteOpen = true;
    await tick();
    footnoteHost?.replaceChildren(footnoteView);
    applyPreferencesToRenderer(footnoteView.renderer, settings);
  }

  function prepareFootnote(event: Event) {
    const detail = (event as CustomEvent<{ view: FoliateView }>).detail;
    applyPreferencesToRenderer(detail.view.renderer, settings);
  }

  function closeFootnote() {
    footnoteView?.close();
    footnoteView?.remove();
    footnoteView = null;
    footnoteOpen = false;
  }

  async function requestClose() {
    if (footnoteOpen) {
      closeFootnote();
      return;
    }
    if (settingsOpen || tocOpen || searchOpen) {
      closePanels();
      return;
    }
    await flushPosition();
    onclose();
  }

  onMount(() => {
    let disposed = false;
    let unlistenCloseRequested: (() => void) | undefined;
    lastPersistedCfi = book.initialCfi || "";
    const element = document.createElement("foliate-view") as FoliateView;
    element.className = "foliate-view";
    element.addEventListener("relocate", handleRelocate);
    element.addEventListener("link", handleInternalLink);
    element.addEventListener("external-link", openExternalLink);
    host.append(element);
    view = element;

    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "ArrowLeft") void view?.goLeft();
      if (event.key === "ArrowRight") void view?.goRight();
      if (event.key === "Escape") void requestClose();
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
        event.preventDefault();
        togglePanel("search");
      }
    };
    const unlistenVolumePageTurn = onVolumePageTurn((direction) => {
      if (direction === "next") void view?.goRight();
      else void view?.goLeft();
    });
    enableVolumePageTurning(true);
    const handleVisibilityChange = () => {
      if (document.visibilityState === "hidden") void flushPosition();
    };
    const handlePageHide = () => void flushPosition();
    window.addEventListener("keydown", handleKey);
    window.addEventListener("pagehide", handlePageHide);
    document.addEventListener("visibilitychange", handleVisibilityChange);

    void getCurrentWindow()
      .onCloseRequested((event) => {
        event.preventDefault();
        void requestClose();
      })
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenCloseRequested = unlisten;
      })
      .catch(() => {
        // Browser-only development has no native window lifecycle.
      });

    void (async () => {
      try {
        await element.open(book.file);
        if (disposed) return;
        if (element.isFixedLayout) {
          const message = "Fixed-layout EPUBs are not supported yet.";
          console.warn(message, { title: book.title, file: book.file.name });
          throw new Error(message);
        }
        tocItems = flattenToc(element.book?.toc ?? []);
        const { FootnoteHandler } = await import("../vendor/foliate-js/footnotes.js");
        if (disposed) return;
        footnoteHandler = new FootnoteHandler();
        footnoteHandler.addEventListener("before-render", prepareFootnote);
        footnoteHandler.addEventListener("render", showFootnote);
        applyReaderPreferences();
        await element.init({
          lastLocation: book.initialCfi || undefined,
          showTextStart: !book.initialCfi,
        });
      } catch (reason) {
        if (!disposed) {
          error = reason instanceof Error ? reason.message : String(reason);
        }
      } finally {
        if (!disposed) loading = false;
      }
    })();

    return () => {
      disposed = true;
      enableVolumePageTurning(false);
      unlistenVolumePageTurn();
      unlistenCloseRequested?.();
      window.removeEventListener("keydown", handleKey);
      window.removeEventListener("pagehide", handlePageHide);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      element.removeEventListener("relocate", handleRelocate);
      element.removeEventListener("link", handleInternalLink);
      element.removeEventListener("external-link", openExternalLink);
      footnoteHandler?.removeEventListener("before-render", prepareFootnote);
      footnoteHandler?.removeEventListener("render", showFootnote);
      footnoteHandler = null;
      clearSearch();
      closeFootnote();
      if (saveTimer) clearTimeout(saveTimer);
      void flushPosition();
      element.close();
      element.remove();
      view = null;
    };
  });
</script>

<svelte:head>
  <title>{book.title} · Reader</title>
</svelte:head>

<main class="reader-shell" data-theme={settings.theme}>
  <header class="reader-header">
    <button class="icon-button" type="button" aria-label="Close reader" onclick={() => void requestClose()}>←</button>
    <div class="book-heading">
      <strong>{book.title}</strong>
      {#if book.author}<span>{book.author}</span>{/if}
    </div>
    <div class="text-controls" aria-label="Reading appearance">
      <button class:active={tocOpen} type="button" aria-label="Table of contents" onclick={() => togglePanel("toc")}>☰</button>
      <button class:active={searchOpen} type="button" aria-label="Search book" onclick={() => togglePanel("search")}>⌕</button>
      <button class:active={settingsOpen} type="button" aria-label="Reading settings" onclick={() => togglePanel("settings")}>Aa</button>
    </div>
  </header>

  {#if tocOpen}
    <section class="reader-panel toc-panel" aria-label="Table of contents">
      <div class="panel-heading">
        <strong>Contents</strong>
        <button type="button" aria-label="Close table of contents" onclick={() => (tocOpen = false)}>×</button>
      </div>
      {#if tocItems.length}
        <nav class="toc-list">
          {#each tocItems as item}
            <button
              type="button"
              class:current={item.href === activeTocHref}
              disabled={!item.href}
              style={`--toc-depth: ${item.depth}`}
              onclick={() => void goToTocItem(item)}
            >{item.label}</button>
          {/each}
        </nav>
      {:else}
        <p class="panel-empty">This book has no table of contents.</p>
      {/if}
    </section>
  {/if}

  {#if searchOpen}
    <section class="reader-panel search-panel" aria-label="Search book">
      <div class="panel-heading">
        <strong>Search</strong>
        <button type="button" aria-label="Close search" onclick={() => (searchOpen = false)}>×</button>
      </div>
      <form class="search-form" onsubmit={runSearch}>
        <input bind:value={searchQuery} type="search" enterkeyhint="search" placeholder="Search this book" aria-label="Search text" />
        <button type="submit" disabled={!searchQuery.trim() || searching}>{searching ? "…" : "Find"}</button>
      </form>
      {#if searching}
        <progress max="1" value={searchProgress} aria-label="Search progress"></progress>
      {/if}
      {#if searchError}
        <p class="panel-error">{searchError}</p>
      {:else if searchResults.length}
        <div class="search-results" aria-live="polite">
          {#each searchResults as result}
            <button type="button" onclick={() => void selectSearchResult(result)}>
              <strong>{result.section}</strong>
              <span>{result.excerpt.pre}<mark>{result.excerpt.match}</mark>{result.excerpt.post}</span>
            </button>
          {/each}
        </div>
      {:else if searchQuery.trim() && !searching && searchProgress === 1}
        <p class="panel-empty">No matches found.</p>
      {/if}
      {#if searchResults.length || searchProgress > 0}
        <button class="clear-search" type="button" onclick={clearSearch}>Clear results</button>
      {/if}
    </section>
  {/if}

  {#if settingsOpen}
    <section class="reader-panel settings-panel" aria-label="Reading settings">
      <div class="panel-heading">
        <strong>Appearance</strong>
        <button type="button" aria-label="Close reading settings" onclick={() => (settingsOpen = false)}>×</button>
      </div>
      <label>
        <span>Theme</span>
        <select value={settings.theme} onchange={(event) => updatePreference("theme", event.currentTarget.value as ReaderTheme)}>
          {#each READER_PREFERENCES_CONTRACT.options.themes as option}
            <option value={option.value}>{option.label}</option>
          {/each}
        </select>
      </label>
      <label>
        <span>Layout</span>
        <select value={settings.flow} onchange={(event) => updatePreference("flow", event.currentTarget.value as ReaderFlow)}>
          {#each READER_PREFERENCES_CONTRACT.options.flows as option}
            <option value={option.value}>{option.label}</option>
          {/each}
        </select>
      </label>
      <label>
        <span>Typeface</span>
        <select value={settings.font_family} onchange={(event) => updatePreference("font_family", event.currentTarget.value as ReaderFontFamily)}>
          {#each READER_PREFERENCES_CONTRACT.options.font_families as option}
            <option value={option.value}>{option.label}</option>
          {/each}
        </select>
      </label>
      <label>
        <span>Alignment</span>
        <select value={settings.text_alignment} onchange={(event) => updatePreference("text_alignment", event.currentTarget.value as ReaderTextAlignment)}>
          {#each READER_PREFERENCES_CONTRACT.options.text_alignments as option}
            <option value={option.value}>{option.label}</option>
          {/each}
        </select>
      </label>

      <div class="setting-stepper">
        <span>Font size</span>
        <button type="button" onclick={() => changeFont(-1)}>−</button>
        <output>{settings.font_size_px.toFixed(0)}px</output>
        <button type="button" onclick={() => changeFont(1)}>+</button>
      </div>
      <div class="setting-stepper">
        <span>Line height</span>
        <button type="button" onclick={() => updatePreference("line_height", Math.max(preferenceConstraints.line_height[0], Number((settings.line_height - 0.1).toFixed(1))))}>−</button>
        <output>{settings.line_height.toFixed(1)}</output>
        <button type="button" onclick={() => updatePreference("line_height", Math.min(preferenceConstraints.line_height[1], Number((settings.line_height + 0.1).toFixed(1))))}>+</button>
      </div>
      <div class="setting-stepper tablet-setting">
        <span>Line width</span>
        <button type="button" onclick={() => updatePreference("max_line_width_px", Math.max(preferenceConstraints.max_line_width_px[0], settings.max_line_width_px - 40))}>−</button>
        <output>{settings.max_line_width_px.toFixed(0)}px</output>
        <button type="button" onclick={() => updatePreference("max_line_width_px", Math.min(preferenceConstraints.max_line_width_px[1], settings.max_line_width_px + 40))}>+</button>
      </div>
      <div class="setting-stepper tablet-setting">
        <span>Columns</span>
        <button type="button" onclick={() => updatePreference("max_column_count", Math.max(preferenceConstraints.max_column_count[0], settings.max_column_count - 1))}>−</button>
        <output>{settings.max_column_count}</output>
        <button type="button" onclick={() => updatePreference("max_column_count", Math.min(preferenceConstraints.max_column_count[1], settings.max_column_count + 1))}>+</button>
      </div>
      <div class="setting-stepper">
        <span>Fixed zoom</span>
        <button type="button" onclick={() => updatePreference("zoom", Math.max(preferenceConstraints.zoom[0], Number((settings.zoom - 0.1).toFixed(1))))}>−</button>
        <output>{settings.zoom.toFixed(1)}×</output>
        <button type="button" onclick={() => updatePreference("zoom", Math.min(preferenceConstraints.zoom[1], Number((settings.zoom + 0.1).toFixed(1))))}>+</button>
      </div>
      <label class="check-setting">
        <span>Hyphenation</span>
        <input type="checkbox" checked={settings.hyphenation} onchange={(event) => updatePreference("hyphenation", event.currentTarget.checked)} />
      </label>
    </section>
  {/if}

  <section class="viewport" aria-label="Book content">
    <div class="view-host" bind:this={host}></div>
    {#if loading}
      <div class="reader-message">Opening book…</div>
    {:else if error}
      <div class="reader-message error">
        <strong>This book could not be opened.</strong>
        <span>{error}</span>
      </div>
    {/if}
  </section>

  {#if footnoteOpen}
    <dialog
      open
      class="modal-backdrop"
      aria-label={footnoteLabel}
      onclick={(event) => {
        if (event.target === event.currentTarget) closeFootnote();
      }}
    >
      <section class="footnote-sheet">
        <div class="panel-heading">
          <strong>{footnoteLabel}</strong>
          <button type="button" aria-label="Close note" onclick={closeFootnote}>×</button>
        </div>
        <div class="footnote-host" bind:this={footnoteHost}></div>
      </section>
    </dialog>
  {/if}

</main>

<style>
  :global(html), :global(body), :global(#app) {
    height: 100%;
    overflow: hidden;
  }

  :global(body) {
    margin: 0;
  }

  .reader-shell {
    --chrome: var(--color-surface-bg);
    --chrome-strong: var(--color-border-color);
    --ink: var(--color-ink-text);
    --accent: var(--color-accent);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    width: 100%;
    height: 100dvh;
    color: var(--ink);
    background: var(--reader-sepia-background);
    position: relative;
  }

  .reader-shell[data-theme="light"] {
    background: var(--reader-light-background);
  }

  .reader-shell[data-theme="dark"] {
    background: var(--reader-dark-background);
  }

  .reader-header {
    position: relative;
    z-index: 2;
    display: flex;
    align-items: center;
    gap: 0.55rem;
    box-sizing: border-box;
    background: var(--chrome);
  }

  .reader-header {
    min-height: calc(3.5rem + env(safe-area-inset-top));
    padding: calc(0.45rem + env(safe-area-inset-top)) max(0.65rem, env(safe-area-inset-right)) 0.45rem max(0.65rem, env(safe-area-inset-left));
    border-bottom: 1px solid var(--chrome-strong);
  }

  button {
    min-width: 2.55rem;
    min-height: 2.55rem;
    padding: 0 0.55rem;
    border: 0;
    border-radius: 0.7rem;
    color: inherit;
    background: transparent;
    font: inherit;
    font-weight: 700;
  }

  button:active {
    background: var(--chrome-strong);
  }

  button.active {
    background: var(--chrome-strong);
  }

  .icon-button {
    font-size: 1.7rem;
    line-height: 1;
  }

  .book-heading {
    min-width: 0;
    flex: 1;
    display: grid;
  }

  .book-heading strong,
  .book-heading span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .book-heading strong {
    font-family: var(--font-heading-family);
    font-size: 0.92rem;
  }

  .book-heading span {
    font-size: 0.75rem;
    opacity: 0.65;
  }

  .text-controls {
    display: flex;
  }

  .text-controls button {
    min-width: 2.25rem;
    min-height: 2.25rem;
    padding: 0 0.35rem;
    font-size: 0.78rem;
  }

  .viewport {
    position: relative;
    min-height: 0;
  }

  .reader-panel {
    position: absolute;
    z-index: 6;
    top: calc(3.5rem + env(safe-area-inset-top));
    right: max(0.65rem, env(safe-area-inset-right));
    width: min(28rem, calc(100% - 1.3rem - env(safe-area-inset-left) - env(safe-area-inset-right)));
    max-height: calc(100dvh - 8rem - env(safe-area-inset-top) - env(safe-area-inset-bottom));
    box-sizing: border-box;
    overflow: auto;
    padding: 1rem;
    border: 1px solid var(--chrome-strong);
    border-radius: 0 0 1rem 1rem;
    color: var(--ink);
    background: var(--chrome);
    box-shadow: 0 1rem 3rem color-mix(in srgb, var(--color-ink-text) 24%, transparent);
  }

  .toc-panel {
    right: auto;
    left: max(0.65rem, env(safe-area-inset-left));
    width: min(24rem, calc(100% - 1.3rem - env(safe-area-inset-left) - env(safe-area-inset-right)));
  }

  .settings-panel {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.8rem;
    width: min(25rem, calc(100% - 1.3rem - env(safe-area-inset-left) - env(safe-area-inset-right)));
  }

  .panel-heading {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 2.5rem;
  }

  .panel-heading strong {
    font-family: var(--font-heading-family);
  }

  .panel-heading button {
    min-width: 2.4rem;
    min-height: 2.4rem;
    font-size: 1.45rem;
    line-height: 1;
  }

  .toc-list,
  .search-results {
    display: grid;
    gap: 0.2rem;
  }

  .toc-list button {
    justify-content: flex-start;
    min-height: 2.75rem;
    padding: 0.55rem 0.7rem 0.55rem calc(0.7rem + var(--toc-depth) * 1rem);
    text-align: start;
    font-weight: 600;
  }

  .toc-list button.current {
    background: var(--chrome-strong);
  }

  .toc-list button:disabled {
    opacity: 0.55;
  }

  .search-panel {
    display: grid;
    gap: 0.75rem;
  }

  .search-form {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 0.5rem;
  }

  .search-form input {
    min-width: 0;
    min-height: 2.65rem;
    padding: 0 0.75rem;
    border: 1px solid var(--chrome-strong);
    border-radius: 0.65rem;
    color: inherit;
    background: color-mix(in srgb, var(--chrome) 84%, white);
    font: inherit;
  }

  .reader-shell[data-theme="dark"] .search-form input {
    background: var(--reader-dark-background);
  }

  .search-form button,
  .clear-search {
    border: 1px solid var(--chrome-strong);
    background: var(--chrome-strong);
  }

  .search-panel progress {
    width: 100%;
    accent-color: var(--accent);
  }

  .search-results {
    max-height: min(55dvh, 34rem);
    overflow: auto;
  }

  .search-results button {
    display: grid;
    gap: 0.25rem;
    min-height: 3.5rem;
    padding: 0.65rem 0.75rem;
    text-align: start;
    font-weight: 500;
  }

  .search-results button strong {
    font-size: 0.72rem;
    opacity: 0.65;
  }

  .search-results button span {
    line-height: 1.35;
  }

  .search-results mark {
    color: inherit;
    background: color-mix(in srgb, var(--accent) 52%, transparent);
  }

  .panel-empty,
  .panel-error {
    margin: 0.75rem 0;
    font-size: 0.88rem;
    opacity: 0.72;
  }

  .panel-error {
    color: var(--color-danger-text);
    opacity: 1;
  }

  .settings-panel label {
    display: grid;
    gap: 0.3rem;
    font-size: 0.75rem;
    font-weight: 700;
  }

  .settings-panel select {
    min-width: 0;
    height: 2.4rem;
    padding: 0 0.55rem;
    border: 1px solid var(--chrome-strong);
    border-radius: 0.55rem;
    color: inherit;
    background: var(--chrome);
    font: inherit;
  }

  .setting-stepper {
    grid-column: 1 / -1;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 2.5rem 4.5rem 2.5rem;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.78rem;
    font-weight: 700;
  }

  .setting-stepper output {
    text-align: center;
    font-variant-numeric: tabular-nums;
  }

  .setting-stepper button {
    min-width: 2.35rem;
    min-height: 2.35rem;
    border: 1px solid var(--chrome-strong);
  }

  .check-setting {
    grid-column: 1 / -1;
    grid-template-columns: 1fr auto !important;
    align-items: center;
  }

  .check-setting input {
    width: 1.3rem;
    height: 1.3rem;
    accent-color: var(--accent);
  }

  .view-host,
  :global(.foliate-view) {
    display: block;
    width: 100%;
    height: 100%;
  }

  .reader-message {
    position: absolute;
    inset: 0;
    display: grid;
    place-content: center;
    gap: 0.6rem;
    padding: 2rem;
    text-align: center;
    background: inherit;
  }

  .reader-message.error span {
    max-width: 28rem;
    font-size: 0.85rem;
    opacity: 0.7;
  }

  .modal-backdrop {
    position: absolute;
    z-index: 10;
    inset: 0;
    width: 100%;
    max-width: none;
    height: 100%;
    max-height: none;
    margin: 0;
    border: 0;
    display: grid;
    align-items: end;
    padding: max(1rem, env(safe-area-inset-top)) max(0.75rem, env(safe-area-inset-right)) calc(0.75rem + env(safe-area-inset-bottom)) max(0.75rem, env(safe-area-inset-left));
    box-sizing: border-box;
    background: var(--color-overlay-scrim);
  }

  .footnote-sheet {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    width: min(42rem, 100%);
    max-height: min(65dvh, 34rem);
    margin: 0 auto;
    overflow: hidden;
    border: 1px solid var(--chrome-strong);
    border-radius: 1rem;
    color: var(--ink);
    background: var(--chrome);
    box-shadow: 0 1.5rem 4rem color-mix(in srgb, var(--color-ink-text) 32%, transparent);
  }

  .footnote-sheet .panel-heading {
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--chrome-strong);
  }

  .footnote-host {
    min-height: 8rem;
    height: min(48dvh, 26rem);
  }

  :global(.footnote-view) {
    display: block;
    width: 100%;
    height: 100%;
  }

  @media (max-width: 420px) {
    .reader-panel {
      left: 0;
      right: 0;
      width: 100%;
      border-inline: 0;
      border-radius: 0;
    }

    .reader-header {
      gap: 0.25rem;
    }

    .text-controls button {
      min-width: 2.1rem;
    }

  }
</style>
