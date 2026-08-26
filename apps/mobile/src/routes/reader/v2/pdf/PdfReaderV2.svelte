<script>
  import { onMount, tick } from "svelte";
  import { page } from "$app/state";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { commands } from "../../../../lib/app/commands";
  import { isWebApp } from "../../../../lib/app/platform";
  import TopBar from "../shared/TopBar.svelte";
  import Sidebar from "../shared/Sidebar.svelte";
  import TocPanel from "../shared/TocPanel.svelte";
  import AnnotationsPanel from "../shared/AnnotationsPanel.svelte";
  import BookmarkPanel from "../shared/BookmarkPanel.svelte";
  import AnnotationEditor from "../../../../components/shared/readers/annotations/AnnotationEditor.svelte";
  import PdfPageV2 from "./PdfPageV2.svelte";
  import { createPdfAnnotations } from "./pdf-annotations.svelte";
  import { createBookmarks } from "../shared/bookmarks.svelte";
  import { pageLocator, pageFromLocator } from "./pdf-locator";
  import { placeAnnotationPopup, rangeRect } from "../shared/placement";
  import { flatten } from "../shared/toc";
  import { attachSession } from "../shared/session";
  import { goto } from "$app/navigation";
  import { hasOwnHistory } from "../../../../lib/app/navigate";
  import { libraryRootHref } from "../../../../lib/app/navigation";

  let libraryId = page.url.searchParams.get("library") ?? "";
  let bookId = page.params.bookId ?? "";

  /// Shared rather than a fresh `[]` per page per render: most pages have no
  /// annotations, and a stable reference means their `PdfPageV2` props don't
  /// look "changed" just because the parent re-rendered.
  const NO_ANNOTATIONS = [];

  let pdfjsApi;
  let pdfDocument;
  let title = $state("");
  let error = $state("");

  let viewport = $state();
  let sidebarOpen = $state(false);
  let tab = $state("contents");
  let chromeHidden = $state(false);

  /// One entry per page, in untransformed PDF points.
  let pages = $state([]);
  /// Set on open and on resize; every page fits to this width, so their
  /// display size is derived rather than stored and re-synced by hand.
  let viewportWidth = $state(0);
  let displayPages = $derived(
    pages.map((state) => ({
      ...state,
      displayWidth: viewportWidth,
      displayHeight: viewportWidth ? state.height * (viewportWidth / state.width) : 0,
    })),
  );
  let canvases = $state([]);
  let textLayers = $state([]);
  let pageShells = $state([]);
  let renderedScale = [];

  let currentPage = $state(0);
  let outlineItems = $state([]);

  let annotations = createPdfAnnotations(libraryId, bookId);
  let bookmarks = createBookmarks(libraryId, bookId);

  /// Grouped once per annotation-list change rather than re-filtered per
  /// page on every render, so editing one annotation doesn't cost O(pages).
  let annotationsByPage = $derived.by(function () {
    let map = new Map();
    for (let item of annotations.items) {
      if (item.anchor.kind !== "pdf") continue;
      let list = map.get(item.anchor.page_index);
      if (list) list.push(item);
      else map.set(item.anchor.page_index, [item]);
    }
    return map;
  });

  /// The last position written, so scroll-driven updates don't rewrite it.
  let storedPage = -1;

  async function store(index) {
    if (index === storedPage) return;
    storedPage = index;
    try {
      await commands.updateReadingPosition(
        libraryId,
        bookId,
        `pdfpage(${Math.max(0, Math.floor(index))})`,
        null,
      );
    } catch {
      // A lost write costs the reader their place, not the reading session.
    }
  }

  /// Which page is topmost in the viewport, which is what "current page"
  /// means for both the position write and the outline's current entry.
  let observer;

  function observePages() {
    observer?.disconnect();
    observer = new IntersectionObserver(
      function (entries) {
        for (let entry of entries) {
          let index = Number(entry.target.dataset.pageIndex);
          if (entry.isIntersecting) renderPage(index);
        }
        let visible = entries
          .filter((entry) => entry.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0];
        if (visible) {
          let index = Number(visible.target.dataset.pageIndex);
          currentPage = index;
          store(index);
        }
      },
      { root: viewport, rootMargin: "50% 0px", threshold: 0 },
    );
    for (let shell of pageShells) if (shell) observer.observe(shell);
  }

  async function renderPage(index) {
    let state = displayPages[index];
    let canvas = canvases[index];
    let textLayer = textLayers[index];
    if (!state || !canvas || !textLayer) return;
    let scale = state.displayWidth / state.width;
    if (Math.abs((renderedScale[index] ?? 0) - scale) < 0.001 && canvas.width) return;
    renderedScale[index] = scale;
    try {
      let pdfPage = await pdfDocument.getPage(index + 1);
      let pixelRatio = Math.min(window.devicePixelRatio || 1, 2);
      let renderViewport = pdfPage.getViewport({ scale: scale * pixelRatio });
      let displayViewport = pdfPage.getViewport({ scale });
      canvas.width = Math.max(1, Math.floor(renderViewport.width));
      canvas.height = Math.max(1, Math.floor(renderViewport.height));
      canvas.style.width = `${displayViewport.width}px`;
      canvas.style.height = `${displayViewport.height}px`;
      let context = canvas.getContext("2d", { alpha: false });
      await pdfPage.render({ canvasContext: context, viewport: renderViewport }).promise;

      textLayer.replaceChildren();
      textLayer.style.width = `${displayViewport.width}px`;
      textLayer.style.height = `${displayViewport.height}px`;
      textLayer.style.setProperty("--scale-factor", String(scale));
      textLayer.style.setProperty("--user-unit", String(pdfPage.userUnit || 1));
      await new pdfjsApi.TextLayer({
        textContentSource: await pdfPage.streamTextContent(),
        container: textLayer,
        viewport: displayViewport,
      }).render();
    } catch (reason) {
      if (reason?.name !== "RenderingCancelledException")
        console.warn("Could not render PDF page", reason);
    }
  }

  /// Every page fits to the viewport's width. Columns and zoom modes are the
  /// kind of thing a fuller reader would add; a basic one just fills the
  /// width — `displayPages` is what turns this into actual page sizes.
  function updateViewportWidth() {
    if (viewport) viewportWidth = viewport.clientWidth;
  }

  async function openOutline() {
    try {
      let outline = await pdfDocument.getOutline().catch(() => null);
      outlineItems = flatten(await resolveOutline(outline));
    } catch {
      outlineItems = [];
    }
  }

  /// Turns pdf.js's outline (nodes carrying a `dest` to resolve) into the
  /// same `{label, href, subitems}` shape `flatten` already knows how to
  /// flatten for the EPUB table of contents, so the row-shaping logic isn't
  /// duplicated per format. Siblings resolve concurrently since none of them
  /// depend on another's destination lookup.
  async function resolveOutline(items) {
    if (!items) return [];
    return Promise.all(
      items.map(async (item) => {
        let href = "";
        try {
          let destination =
            typeof item.dest === "string"
              ? await pdfDocument.getDestination(item.dest)
              : item.dest;
          if (Array.isArray(destination) && destination[0])
            href = String(await pdfDocument.getPageIndex(destination[0]));
        } catch {
          // A destination that can't resolve is still worth listing, just unopenable.
        }
        return {
          label: item?.title ? String(item.title) : "",
          href,
          subitems: await resolveOutline(item?.items),
        };
      }),
    );
  }

  function goToPage(index) {
    let target = Math.max(0, Math.min(pages.length - 1, index));
    pageShells[target]?.scrollIntoView({ block: "start" });
    sidebarOpen = false;
  }

  function goToOutline(href) {
    if (href) goToPage(Number(href));
  }

  function goToBookmark(bookmark) {
    goToPage(pageFromLocator(bookmark.cfi));
  }

  function addBookmark() {
    bookmarks.add(pageLocator(currentPage), `Page ${currentPage + 1}`);
  }

  /// What is selected right now, captured where it can still be read.
  let selection = $state(null);

  function captureSelection() {
    let selected = window.getSelection();
    if (!selected?.rangeCount || selected.isCollapsed) {
      selection = null;
      return;
    }
    let range = selected.getRangeAt(0);
    let start =
      range.startContainer instanceof Element
        ? range.startContainer
        : range.startContainer.parentElement;
    let layer = start?.closest(".pdf-text-layer");
    let shell = layer?.closest(".pdf-page");
    let index = Number(shell?.dataset.pageIndex);
    let text = selected.toString().trim();
    if (!shell || !Number.isInteger(index) || !text) {
      selection = null;
      return;
    }
    let bounds = shell.getBoundingClientRect();
    let clientRects = Array.from(range.getClientRects()).filter(
      (rect) => rect.width > 0 && rect.height > 0,
    );
    let rects = clientRects
      .map((rect) => ({
        left: (Math.max(bounds.left, rect.left) - bounds.left) / bounds.width,
        top: (Math.max(bounds.top, rect.top) - bounds.top) / bounds.height,
        width:
          (Math.min(bounds.right, rect.right) - Math.max(bounds.left, rect.left)) /
          bounds.width,
        height:
          (Math.min(bounds.bottom, rect.bottom) - Math.max(bounds.top, rect.top)) /
          bounds.height,
      }))
      .filter((rect) => rect.width > 0 && rect.height > 0);
    selection = rects.length
      ? { pageIndex: index, text, rects, rect: rangeRect(range, document) }
      : null;
  }

  /// The annotation being edited, and where its popup sits.
  let editing = $state(null);
  let anchor = $state(null);
  let popup = $state(null);
  let popupElement = null;

  async function noteMounted(node) {
    popupElement = node?.closest(".reader-annotation-editor") ?? null;
    await tick();
    if (!popupElement || !anchor) return;
    popup = placeAnnotationPopup(anchor, popupElement.getBoundingClientRect(), {
      width: window.innerWidth,
      height: window.innerHeight,
    });
  }

  async function highlightSelection() {
    if (!selection) return;
    anchor = selection.rect;
    popup = null;
    let saved = await annotations.save(
      annotations.draft(selection.pageIndex, selection.rects, selection.text),
    );
    selection = null;
    window.getSelection()?.removeAllRanges();
    editing = saved;
  }

  function openAnnotation(annotation) {
    anchor = null;
    popup = null;
    editing = annotation;
  }

  async function editAnnotation(changes) {
    if (!editing) return;
    editing = await annotations.save({ ...editing, ...changes });
  }

  async function deleteEditing() {
    if (!editing) return;
    await annotations.remove(editing);
    closeEditor();
  }

  function closeEditor() {
    editing = null;
    anchor = null;
    popup = null;
    popupElement = null;
  }

  function tapped(event) {
    if (window.getSelection()?.type === "Range") return;
    if (event.target.closest(".pdf-annotation-layer button")) return;
    if (editing) {
      closeEditor();
      return;
    }
    chromeHidden = !chromeHidden;
  }

  function showPanel(next) {
    if (sidebarOpen && tab === next) {
      sidebarOpen = false;
      return;
    }
    tab = next;
    sidebarOpen = true;
  }

  function close() {
    if (hasOwnHistory()) {
      history.back();
      return;
    }
    void goto(libraryId ? libraryRootHref(libraryId) : "/library", {
      replaceState: true,
    });
  }

  function volumeTurn(direction) {
    viewport?.scrollBy({
      top: direction === "next" ? viewport.clientHeight : -viewport.clientHeight,
      behavior: "smooth",
    });
  }

  async function openSource() {
    pdfjsApi.GlobalWorkerOptions.workerSrc = new URL(
      "/pdfjs/pdf.worker.mjs",
      location.href,
    ).toString();
    let options = {
      cMapUrl: new URL("/pdfjs/cmaps/", location.href).toString(),
      standardFontDataUrl: new URL("/pdfjs/standard_fonts/", location.href).toString(),
      isEvalSupported: false,
    };
    let path = await commands.openBook(libraryId, bookId);
    let url = isWebApp ? path : convertFileSrc(path);
    let task = pdfjsApi.getDocument({ ...options, url });
    try {
      return await task.promise;
    } catch (reason) {
      await task.destroy?.().catch?.(() => undefined);
      // Some Android WebViews cannot give the pdf.js worker direct access to
      // Tauri's asset protocol; buffering the whole file is a fallback only.
      let response = await fetch(url);
      if (!response.ok) throw reason;
      return pdfjsApi.getDocument({ ...options, data: await response.arrayBuffer() })
        .promise;
    }
  }

  async function show() {
    try {
      pdfjsApi = await import("../../../../vendor/foliate-js/vendor/pdfjs/pdf.mjs");
      pdfDocument = await openSource();
      let metadata = await pdfDocument.getMetadata().catch(() => null);
      title =
        metadata?.info?.Title ||
        (await commands.getBookFileName(libraryId, bookId).catch(() => ""));

      // Independent lookups: fetched together rather than one page at a time
      // so an N-page book pays one round trip, not N sequential ones.
      pages = await Promise.all(
        Array.from({ length: pdfDocument.numPages }, async (_, index) => {
          let pdfPage = await pdfDocument.getPage(index + 1);
          let { width, height } = pdfPage.getViewport({ scale: 1 });
          return { width, height };
        }),
      );
      await tick();
      updateViewportWidth();

      let initial =
        (await commands.getPdfReadingPosition(libraryId, bookId).catch(() => null)) ??
        null;
      storedPage = initial?.[0] ?? 0;
      currentPage = storedPage;
      await tick();
      pageShells[storedPage]?.scrollIntoView({ block: "start" });

      observePages();
      openOutline();
      annotations.restore();
      bookmarks.restore();
      viewport?.addEventListener("click", tapped);
    } catch (reason) {
      error = String(reason);
    }
  }

  onMount(function () {
    document.addEventListener("pointerup", captureSelection);
    window.addEventListener("resize", updateViewportWidth);
    let detachSession = attachSession(volumeTurn);
    show();

    return function () {
      document.removeEventListener("pointerup", captureSelection);
      window.removeEventListener("resize", updateViewportWidth);
      viewport?.removeEventListener("click", tapped);
      observer?.disconnect();
      detachSession();
    };
  });
</script>

<div class="reader">
  <TopBar
    {title}
    panel={sidebarOpen ? tab : ""}
    onClose={close}
    onContents={() => showPanel("contents")}
    hidden={chromeHidden}
  />

  <div
    class="reader-body"
    class:has-sidebar={sidebarOpen}
  >
    <Sidebar
      {tab}
      onTab={(next) => (tab = next)}
      hidden={!sidebarOpen}
    >
      {#snippet contents()}
        <TocPanel
          items={outlineItems}
          activeHref={String(currentPage)}
          onSelect={goToOutline}
        />
      {/snippet}

      {#snippet annotationList()}
        <AnnotationsPanel
          items={annotations.items}
          onSelect={(item) =>
            goToPage(item.anchor.kind === "pdf" ? item.anchor.page_index : 0)}
          onDelete={annotations.remove}
        />
      {/snippet}

      {#snippet bookmarkList()}
        <BookmarkPanel
          {bookmarks}
          bookmarked={bookmarks.items.some(
            (item) => pageFromLocator(item.cfi) === currentPage,
          )}
          onAdd={addBookmark}
          onSelect={goToBookmark}
        />
      {/snippet}
    </Sidebar>

    <div
      class="pdf-viewport"
      bind:this={viewport}
    >
      {#each displayPages as state, index (index)}
        <div
          bind:this={pageShells[index]}
          data-page-index={index}
        >
          <PdfPageV2
            {index}
            width={state.displayWidth}
            height={state.displayHeight}
            annotations={annotationsByPage.get(index) ?? NO_ANNOTATIONS}
            activeAnnotationId={editing?.id ?? null}
            onOpenAnnotation={openAnnotation}
            bind:canvas={canvases[index]}
            bind:textLayer={textLayers[index]}
          />
        </div>
      {/each}
    </div>
  </div>

  {#if selection?.rect}
    <button
      class="highlight-selection"
      style="left: {(selection.rect.left + selection.rect.right) / 2}px; top: {selection
        .rect.bottom}px"
      onclick={highlightSelection}>Highlight</button
    >
  {/if}

  {#if editing}
    {#key editing.id}
      <AnnotationEditor
        annotation={editing}
        note={editing.note}
        position={popup}
        placement={popup?.placement}
        onNoteChange={(note) => editAnnotation({ note })}
        onStyleChange={(next) => editAnnotation({ style: next })}
        onColorChange={(next) => editAnnotation({ color: next })}
        onDelete={deleteEditing}
        onClose={closeEditor}
        onTextareaMount={noteMounted}
        showDelete
        showClose
      />
    {/key}
  {/if}

  {#if error}
    <p class="reader-error">{error}</p>
  {/if}
</div>

<svelte:head>
  <link
    rel="stylesheet"
    href="/pdfjs/text_layer_builder.css"
  />
</svelte:head>

<style>
  .reader {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .reader-body {
    position: relative;
    display: flex;
    flex: 1;
    min-height: 0;
  }

  @media (min-width: 721px) {
    .reader-body.has-sidebar .pdf-viewport {
      margin-inline-start: min(380px, 40vw);
    }
  }

  .pdf-viewport {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: center;
    min-width: 0;
    min-height: 0;
    padding: 1rem;
    overflow-y: auto;
    background: var(--color-surface-bg);
  }

  .reader-error {
    flex: none;
    padding: 1rem;
  }

  .highlight-selection {
    position: fixed;
    z-index: 8;
    translate: -50% 4px;
    padding: 6px 10px;
    background: var(--color-raised-bg);
    border: 1px solid var(--color-border);
    color: inherit;
    cursor: pointer;
  }
</style>
