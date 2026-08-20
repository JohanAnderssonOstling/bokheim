<script lang="ts">
  import { onMount, setContext, tick } from "svelte";
  // Foliate is vendored as a pinned submodule. Importing view.js registers the
  // <foliate-view> custom element and keeps the unstable API boundary here.
  import {
    READER_PREFERENCES_CONTRACT,
    READER_SETTINGS_PRESENTATION_CONTRACT,
  } from "../../../../lib/shared/types";
  import { openExternalUrl } from "../../../../lib/app/external-links";
  import {
    mountFoliateView,
    openPublication,
    releaseEpubSource,
  } from "./reader-foliate-view";
  import { attachReaderSession } from "./reader-session";
  import type {
    FoliateView,
    ReaderBook,
    ReaderAnnotation,
    ApplicationTheme,
    ReaderPreferencePatch,
    ReaderPreferences,
  } from "../../../../lib/shared/types";
  import {
    createReaderPreferences,
    readerPreferenceConstraints,
  } from "./reader-preferences.svelte";
  import { annotationOverlay, flattenToc } from "./reader-helpers";
  import { matchReaderKey } from "./reader-keyboard";
  import { attachSectionInput } from "./reader-document-input";
  import { createReaderSearch } from "./reader-search.svelte";
  import { createReaderFootnotes } from "./reader-footnotes.svelte";
  import { createReaderBookmarks } from "./reader-bookmarks.svelte";
  import { createReaderPosition } from "./reader-position.svelte";
  import { createReaderChrome } from "./reader-chrome.svelte";
  import { createReaderSelection } from "./reader-selection.svelte";
  import {
    createReaderAnnotationStore,
    drawAnnotationOverlay,
  } from "./reader-annotation-store.svelte";
  import { installReaderMarks } from "../../../../lib/reader/marks";
  import { createReaderAnnotationEditor } from "./reader-annotation-editor.svelte";
  import { rangeViewportRect } from "./annotation-placement";
  import { createReaderNavigation } from "./reader-navigation.svelte";
  import type { TocItem } from "./reader-types";
  import type { ReaderTocPanelItem } from "../../../../components/shared/readers/sidebar/toc-panel-items.ts";
  import ReaderHeader from "../../../../components/shared/readers/chrome/ReaderHeader.svelte";
  import EpubReaderSidebar from "./EpubReaderSidebar.svelte";
  import SearchBar from "./SearchBar.svelte";
  import SettingsPanel from "./SettingsPanel.svelte";
  import SelectionContextMenu from "./SelectionContextMenu.svelte";
  import SharedAnnotationEditor from "../../../../components/shared/readers/annotations/AnnotationEditor.svelte";
  import FootnoteSheet from "./FootnoteSheet.svelte";
  import ImageViewer from "./ImageViewer.svelte";
  import EpubReaderShell from "./EpubReaderShell.svelte";
  import EpubContentSurface from "./EpubContentSurface.svelte";

  let {
    book,
    preferences,
    applicationTheme,
    applicationAppearance,
    onpreferenceschange,
    onflushpreferences,
    onclose,
  }: {
    book: ReaderBook;
    preferences: ReaderPreferences;
    applicationTheme: ApplicationTheme;
    applicationAppearance: "light" | "dark";
    onpreferenceschange: (patch: ReaderPreferencePatch) => void | Promise<void>;
    onflushpreferences?: () => void;
    onclose: () => void;
  } = $props();

  let host: HTMLDivElement;
  let view = $state<FoliateView | null>(null);
  let loading = $state(true);
  let error = $state("");
  let readerState = $state({ error: "" });
  let imageViewerSrc = $state("");
  let readerLocation = $state({ activeHref: "" });

  let readerLayoutFrame: number | undefined;

  const selection = createReaderSelection({
    getCfi: (index, range) => view?.getCFI(index, range) || "",
    getAttribution: () => [book.title, book.author].filter(Boolean).join(" — "),
  });

  const annotationStore = createReaderAnnotationStore({
    getView: () => view,
    getBookId: () => book.bookId,
    canUpsert: () => Boolean(book.upsertAnnotation),
    canDelete: () => Boolean(book.deleteAnnotation),
    canLoad: () => Boolean(book.loadAnnotations),
    upsertAnnotation: (annotation) => book.upsertAnnotation?.(annotation),
    deleteAnnotation: (id, at) => book.deleteAnnotation?.(id, at),
    loadAnnotations: () => book.loadAnnotations?.() ?? Promise.resolve([]),
    readerState,
  });

  const annotationEditor = createReaderAnnotationEditor({
    canUpsert: () => Boolean(book.upsertAnnotation),
    canDelete: () => Boolean(book.deleteAnnotation),
    getAnnotations: () => annotationStore.items,
    setAnnotations: (items) => (annotationStore.items = items),
    upsertAnnotation: (annotation) => book.upsertAnnotation?.(annotation),
    deleteAnnotation: (annotation) => annotationStore.remove(annotation),
    replaceOverlay: (previous, next) => {
      void annotationStore.removeOverlay(previous);
      void annotationStore.addOverlay(next);
    },
    getAnchorRect: () => selection.anchorRect,
    clearAnchorRect: () => (selection.anchorRect = null),
    closeContextMenu: () => selection.closeContextMenu(),
    readerState,
  });

  const chrome = createReaderChrome();

  const nav = createReaderNavigation({
    getActiveTocHref: () => position.activeTocHref,
    loadAnnotationsEnabled: () => Boolean(book.loadAnnotations),
  });

  const bookmarkList = createReaderBookmarks({
    getBookId: () => book.bookId,
    getCurrentCfi: () => position.lastCfi,
    getCurrentLabel: () =>
      nav.tocItems.find((item) => item.href === position.activeTocHref)?.label ||
      `Location ${position.location + 1}`,
    canCreate: () => Boolean(book.createBookmark),
    canDelete: () => Boolean(book.deleteBookmark),
    canLoad: () => Boolean(book.loadBookmarks),
    createBookmark: (bookmark) => book.createBookmark?.(bookmark),
    deleteBookmark: (id, at) => book.deleteBookmark?.(id, at),
    loadBookmarks: () => book.loadBookmarks?.() ?? Promise.resolve([]),
    readerState,
  });

  const footnotes = createReaderFootnotes({
    applyPreferences: (renderer) => prefs.applyToRenderer(renderer),
  });

  const search = createReaderSearch({
    getView: () => view,
    getCurrentSectionLabel: () =>
      (position.activeTocHref
        ? nav.tocItems.find((item) => item.href === position.activeTocHref)?.label
        : "") || "Current section",
  });

  const position = createReaderPosition({
    getView: () => view,
    canSave: () => Boolean(book.saveCfi),
    saveCfi: (cfi, fraction) => book.saveCfi?.(cfi, fraction),
    getLineDistance: () =>
      Math.max(1, prefs.settings.font_size_px * prefs.settings.line_height),
    onRelocate: (href) => nav.expandActiveTocAncestors(href),
  });

  const prefs = createReaderPreferences({
    getPreferences: () => preferences,
    getApplicationTheme: () => applicationTheme,
    getApplicationAppearance: () => applicationAppearance,
    getViewRenderer: () => view?.renderer,
    getFootnoteRenderer: () => footnotes.renderer,
    onChange: (patch) => onpreferenceschange(patch),
  });
  setContext("epub-reader", {
    readerLocation,
    readerState,
    navigation: nav,
    preferences: prefs,
    openContentItem: (item: ReaderTocPanelItem) => goToTocItem(item.data as TocItem),
    toggleContentItem: (item: ReaderTocPanelItem) => nav.toggleTocItem(item.data as TocItem),
  });

  const disposeMarks = installReaderMarks({
    annotations: () => annotationStore.items,
    bookmarks: () => bookmarkList.items,
    activateAnnotation: (id) => {
      const annotation = annotationStore.items.find((item) => item.id === id);
      if (annotation) return activateStoredAnnotation(annotation);
    },
    deleteAnnotation: (id) => {
      const annotation = annotationStore.items.find((item) => item.id === id);
      if (annotation) return annotationStore.remove(annotation);
    },
    addBookmark: () => bookmarkList.add(),
    canAddBookmark: () => Boolean(position.lastCfi),
    openBookmark: async (id) => {
      const bookmark = bookmarkList.items.find((item) => item.id === id);
      if (bookmark) await view?.goTo(bookmark.cfi);
    },
    deleteBookmark: (id) => {
      const bookmark = bookmarkList.items.find((item) => item.id === id);
      if (bookmark) return bookmarkList.remove(bookmark);
    },
  });

  function scheduleReaderLayout() {
    if (readerLayoutFrame !== undefined) cancelAnimationFrame(readerLayoutFrame);
    readerLayoutFrame = requestAnimationFrame(() => {
      readerLayoutFrame = requestAnimationFrame(() => {
        readerLayoutFrame = undefined;
        view?.renderer?.render?.();
      });
    });
  }

  async function flushPosition() {
    await position.flush();
  }

  function handleRelocate(event: Event) {
    selection.closeContextMenu();
    readerLocation.activeHref = (event as CustomEvent<{ href?: string }>).detail?.href ?? "";
    position.handleRelocate(event);
  }

  function handleSectionLoad(event: Event) {
    const { doc, index } = (event as CustomEvent<{ doc: Document; index: number }>)
      .detail;
    attachSectionInput(doc, index, {
      onSelectionChange: selection.capture,
      onPointerDown: () => {
        selection.closeContextMenu();
        if (annotationEditor.active) {
          annotationEditor.close();
          return false;
        }
        if (nav.settingsOpen) {
          nav.settingsOpen = false;
          return false;
        }
        return true;
      },
      onPointerMove: chrome.revealAtTopEdge,
      onPageTap: chrome.toggleAfterHitTesting,
      onContextMenu: (position) => (selection.contextMenu = position),
      onImageActivate: (src) => (imageViewerSrc = src),
      onKeyDown: handleReaderKey,
      shouldHandleTap: () =>
        !nav.settingsOpen &&
        !nav.sidebarOpen &&
        !nav.searchOpen &&
        !footnotes.open,
    });
  }

  function handleShowAnnotation(event: Event) {
    chrome.cancelPendingToggle();
    const { value: cfi, range } = (event as CustomEvent<{ value: string; range?: Range }>)
      .detail;
    const annotation = annotationStore.find(cfi);
    if (!annotation) return;
    selection.adoptAnnotation(cfi, annotation.exact_text, annotation.note);
    const doc = range?.startContainer.ownerDocument;
    if (range && doc) selection.anchorRect = rangeViewportRect(range, doc);
    void annotationEditor.open(annotation);
  }

  async function showStoredAnnotation(annotation: ReaderAnnotation) {
    await annotationStore.addOverlay(annotation);
    const overlay = annotationOverlay(annotation);
    if (overlay) await view?.showAnnotation(overlay);
    // showAnnotation emits the precise range and opens the editor. Keep a
    // fallback for renderers that do not emit that event.
    if (annotationEditor.active?.id !== annotation.id)
      await annotationEditor.open(annotation);
  }

  async function activateStoredAnnotation(annotation: ReaderAnnotation) {
    // In the compact layout the annotations panel covers the publication, so
    // dismiss it before navigation and popup positioning. Wide layouts retain
    // the list alongside the document for quick annotation-to-annotation work.
    if (window.matchMedia("(max-width: 720px)").matches) {
      nav.hideSidebar();
      await tick();
    }
    await showStoredAnnotation(annotation);
  }

  async function saveSelectionAndOpenAnnotation() {
    const current = selection.current;
    if (!current) return;
    selection.closeContextMenu();
    const annotation = await annotationStore.saveSelection(current, selection.note);
    if (!annotation) return;
    selection.clear();
    await annotationEditor.open(annotation);
  }

  async function goToTocItem(item: TocItem) {
    if (!item.href || !view) return;
    await view.goTo(item.href);
    nav.closePanels();
  }

  function handleReaderKey(event: KeyboardEvent) {
    const match = matchReaderKey(event);
    if (!match) return;
    if (match.preventDefault) event.preventDefault();
    const { intent } = match;
    if (intent.kind === "navigate") position.navigate(intent.action);
    if (intent.kind === "adjust_font_size") prefs.adjust("font_size_px", intent.delta);
    if (intent.kind === "toggle_search") nav.togglePanel("search");
    if (intent.kind === "cycle_search") search.cycle(intent.offset);
    if (intent.kind === "close") void requestClose();
  }

  async function openExternalLink(event: Event) {
    event.preventDefault();
    const href = (event as CustomEvent<{ href_?: string }>).detail?.href_;
    if (!href) return;
    try {
      await openExternalUrl(href);
    } catch (reason) {
      console.warn("Could not open external link", reason);
    }
  }

  function handleFootnoteLink(event: Event) {
    footnotes.handleInternalLink(view?.book, event);
  }

  async function requestClose() {
    if (annotationEditor.active) {
      annotationEditor.close();
      return;
    }
    if (footnotes.open) {
      footnotes.close();
      return;
    }
    if (nav.settingsOpen || nav.sidebarOpen || nav.searchOpen) {
      nav.closePanels();
      return;
    }
    // Closing the reader must not wait for network persistence. The save queue
    // continues in the background and the component cleanup flushes again.
    void flushPosition();
    onflushpreferences?.();
    onclose();
  }

  onMount(() => {
    nav.restoreTocState();

    let disposed = false;
    const { element, dispose: disposeView } = mountFoliateView(host, {
      relocate: handleRelocate,
      link: handleFootnoteLink,
      externalLink: openExternalLink,
      load: handleSectionLoad,
      drawAnnotation: drawAnnotationOverlay,
      showAnnotation: handleShowAnnotation,
      createOverlay: annotationStore.redrawOverlays,
    });
    view = element;

    const flushReaderState = () => {
      void flushPosition();
      onflushpreferences?.();
    };

    const disposeSession = attachReaderSession({
      flush: flushReaderState,
      onPageTurn: (direction) => {
        if (direction === "next") void view?.goRight();
        else void view?.goLeft();
      },
      onCloseRequested: () => void requestClose(),
    });

    const closeSelectionContextMenu = (event?: Event) => {
      const target = event?.target as Element | null;
      if (target?.closest?.(".selection-context-menu")) return;
      selection.closeContextMenu();
    };
    const dismissAnnotationFromOutside = (event: Event) => {
      const target = event.target as Element | null;
      if (!annotationEditor.active || !annotationEditor.isOutside(target)) return;
      annotationEditor.close();
    };
    const handleWindowResize = () => {
      closeSelectionContextMenu();
      annotationEditor.reposition();
      scheduleReaderLayout();
    };
    window.addEventListener("keydown", handleReaderKey);
    window.addEventListener("pointerdown", closeSelectionContextMenu, true);
    window.addEventListener("pointerdown", dismissAnnotationFromOutside, true);
    window.addEventListener("resize", handleWindowResize);

    void (async () => {
      try {
        await openPublication(element, {
          file: book.file,
          initialCfi: book.initialCfi,
          initialTarget: book.initialTarget,
          isDisposed: () => disposed,
          onBookReady: async (opened) => {
            nav.setToc(flattenToc(opened.book?.toc ?? []));
            await footnotes.attach();
            prefs.applyAll();
          },
        });
        if (disposed) return;
        // WebKit can finish the initial pagination before the Tauri window's
        // final grid width is committed. Re-render after two paint frames so
        // both columns use the settled viewport instead of clipping the right.
        scheduleReaderLayout();

        // Annotations and bookmarks enrich an already usable reader. Loading
        // them must not keep the first rendered page behind the opening veil.
        void annotationStore.restore();
        void bookmarkList.restore(() => disposed);
      } catch (reason) {
        if (!disposed) error = reason instanceof Error ? reason.message : String(reason);
      } finally {
        if (!disposed) loading = false;
      }
    })();

    return () => {
      disposed = true;
      disposeSession();
      window.removeEventListener("keydown", handleReaderKey);
      window.removeEventListener("pointerdown", closeSelectionContextMenu, true);
      window.removeEventListener("pointerdown", dismissAnnotationFromOutside, true);
      window.removeEventListener("resize", handleWindowResize);
      search.clear();
      footnotes.dispose();
      chrome.dispose();
      if (annotationEditor.active) annotationEditor.close();
      annotationEditor.dispose();
      disposeMarks();
      if (readerLayoutFrame !== undefined) cancelAnimationFrame(readerLayoutFrame);
      position.dispose();
      flushReaderState();
      disposeView();
      releaseEpubSource(book.file);
      view = null;
    };
  });
</script>

<svelte:head>
  <title>{book.title} · Reader</title>
</svelte:head>

<EpubReaderShell
  hiddenChrome={chrome.hidden}
  hasSidebar={nav.sidebarOpen}
  searchActive={nav.searchOpen}
  onPointerMove={chrome.revealAtTopEdge}
  onPointerDown={nav.dismissSettingsFromOutside}
>
  <ReaderHeader
    title={book.title}
    {onclose}
    hiddenChrome={chrome.hidden}
    onToggleSidebar={() => nav.toggleReaderSidebar()}
    onToggleSearch={() => nav.togglePanel("search")}
    onToggleSettings={() => nav.togglePanel("settings")}
    searchLabel="Search in book"
    sidebarLabel="Contents and annotations"
    settingsLabel="Reader settings"
    sidebarOpen={nav.sidebarOpen}
    searchOpen={nav.searchOpen}
    settingsOpen={nav.settingsOpen}
  >
    <span slot="settings-content">
      {READER_SETTINGS_PRESENTATION_CONTRACT.trigger_label}
    </span>
  </ReaderHeader>

    <EpubReaderSidebar />

  {#if nav.searchOpen}
    <SearchBar
      query={search.query}
      searchResultsLength={search.results.length}
      selectedSearchIndex={search.selectedIndex}
      searchError={search.error}
      onQueryChange={(value) => {
        search.query = value;
        void search.run();
      }}
      onSubmitSearch={(event) => {
        event.preventDefault();
        void search.run();
      }}
      onCycleSearch={(offset) => search.cycle(offset)}
    />
  {/if}

  {#if nav.settingsOpen}
    <SettingsPanel />
  {/if}

  <EpubContentSurface
    {loading}
    {error}
  >
    <div
      class="view-host"
      bind:this={host}
    ></div>
  </EpubContentSurface>

  <SelectionContextMenu
    selectionText={selection.text}
    position={selection.contextMenu}
    onHighlight={() => void saveSelectionAndOpenAnnotation()}
    onCopyCitation={() => void selection.copyCitation()}
  />

  {#if annotationEditor.active}
    <SharedAnnotationEditor
      annotation={annotationEditor.active}
      note={annotationEditor.draft}
      position={annotationEditor.position}
      placement={annotationEditor.placement}
      onNoteChange={(note) => annotationEditor.updateDraft(note)}
      onStyleChange={(nextStyle) => annotationEditor.setStyle(nextStyle)}
      onColorChange={(nextColor) => annotationEditor.setColor(nextColor)}
      onDelete={() => annotationEditor.deleteActive()}
      onTextareaMount={(node) => annotationEditor.setNoteInput(node)}
      showDelete
    />
  {/if}

  <FootnoteSheet
    open={footnotes.open}
    label={footnotes.label}
    onClose={() => footnotes.close()}
    onHostMount={(node) => footnotes.setHost(node)}
  />

  <ImageViewer
    imageSrc={imageViewerSrc}
    onClose={() => (imageViewerSrc = "")}
  />
</EpubReaderShell>
