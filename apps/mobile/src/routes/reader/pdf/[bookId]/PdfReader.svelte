<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    enableReaderImmersiveMode,
    enableVolumePageTurning,
    onVolumePageTurn,
  } from "../../../../lib/reader/volume-navigation";
  import { isNativeApp } from "../../../../lib/app/platform";
  import {
    closeNativePdfRangeSource,
    createNativePdfRangeTransport,
    isNativePdfRangeSource,
  } from "../../../../lib/reader/pdf-range-transport";
  import type {
    PdfAnnotationRect,
    ReaderAnnotation,
    ReaderAnnotationStyle,
    ReaderBook,
    ReaderBookmark,
  } from "../../../../lib/shared/types";
  import slidersHorizontal from "@icons/phosphor/sliders-horizontal.svg";
  import StatusState from "../../../../components/shared/StatusState.svelte";
  import ReaderHeader from "../../../../components/shared/readers/chrome/ReaderHeader.svelte";
  import PdfSelectionActions from "./components/PdfSelectionActions.svelte";
  import PdfReaderShell from "./components/PdfReaderShell.svelte";
  import PdfReaderSidebar from "./PdfReaderSidebar.svelte";
  import {
    initialExpandedKeys,
    tocPanelItems,
    toggleExpandedKey,
  } from "../../../../components/shared/readers/sidebar/toc-panel-items.ts";
  import PdfSearchPanel from "./components/PdfSearchPanel.svelte";
  import PdfSettingsPanel from "./components/PdfSettingsPanel.svelte";
  import PdfPage from "./components/PdfPage.svelte";
  import SharedAnnotationEditor from "../../../../components/shared/readers/annotations/AnnotationEditor.svelte";
  import {
    centerAnchor,
    placeAnnotationPopup,
  } from "../../../../components/shared/readers/annotations/annotation-placement";
  import FileIcon from "../../../../components/shared/FileIcon.svelte";
  import { createAnnotationController } from "../../../../components/shared/readers/annotations/annotation-controller.ts";
  import { createReaderPositionAutosave } from "../../../../components/shared/readers/state/reader-position-autosave.ts";
  import {
    clamp as clampValue,
    estimateRowLineHeight as estimateRowLineHeightForRows,
    layoutScale as pdfLayoutScale,
    lineOffsetsInDocument as getLineOffsetsInDocument,
    pageFromLocator,
    pageLocator,
    resolveLayout as resolvePdfLayout,
    rowStartForScrollTop as pdfRowStartForScrollTop,
    surfaceSize as getPdfSurfaceSize,
    widthFitZoom as pdfWidthFitZoom,
    PDF_COLUMN_GAP,
    PDF_MAX_PAGE_COLUMNS,
    PDF_MIN_ZOOM,
    PDF_SURFACE_INSET,
    PDF_VIEWPORT_SCROLL_OVERLAP_LINES,
  } from "./shared/pdf-reader-layout";
  import type {
    FitMode,
    OutlineItem,
    PdfPageState,
    PdfSelection,
    SearchResult,
  } from "./pdf-reader-types";

  let {
    book,
    onclose,
  }: {
    book: ReaderBook;
    onclose: () => void;
  } = $props();

  let viewport: HTMLDivElement;
  let loading = $state(true);
  let error = $state("");
  let pages = $state<PdfPageState[]>([]);
  let currentPage = $state(0);
  let fitMode = $state<FitMode>("width");
  /// Pixels per PDF point, which is also PDF.js's own scale unit.
  let zoom = $state(1);
  let pageColumns = $state(1);
  let sidebarOpen = $state(false);
  let sidebarTab = $state<"contents" | "highlights" | "bookmarks">("contents");
  let settingsOpen = $state(false);
  let searchOpen = $state(false);
  let outlineItems = $state<OutlineItem[]>([]);
  let expandedOutlineKeys = $state<Set<string>>(new Set());
  let searchQuery = $state("");
  let searchResults = $state<SearchResult[]>([]);
  let selectedSearchResult = $state(-1);
  let searching = $state(false);
  let documentTitle = $state("");
  let annotations = $state<ReaderAnnotation[]>([]);
  let bookmarks = $state<ReaderBookmark[]>([]);
  let selection = $state<PdfSelection | null>(null);
  let activeAnnotation = $state<ReaderAnnotation | null>(null);
  let annotationNote = $state("");
  let annotationError = $state("");
  let chromeVisible = $state(true);
  let tapStart: {
    pointerId: number;
    x: number;
    y: number;
    time: number;
    moved: boolean;
  } | null = null;

  let pdfDocument: any;
  let pdfjsApi: any;
  let intersectionObserver: IntersectionObserver | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let searchGeneration = 0;
  let pendingChromeToggle: ReturnType<typeof setTimeout> | undefined;
  let scrollFrame: number | undefined;
  let disposed = false;
  const annotationController = createAnnotationController({
    canUpsert: () => Boolean(book.upsertAnnotation),
    canDelete: () => Boolean(book.deleteAnnotation),
    getActiveAnnotation: () => activeAnnotation,
    setActiveAnnotation: (annotation) => {
      activeAnnotation = annotation;
    },
    getAnnotations: () => annotations,
    setAnnotations: (items) => {
      annotations = items;
    },
    upsertAnnotation: (annotation) => book.upsertAnnotation?.(annotation),
    deleteAnnotation: async (annotation) => {
      await book.deleteAnnotation?.(annotation.id, Date.now());
      annotations = annotations.filter((item) => item.id !== annotation.id);
    },
    onPersistError: (message) => {
      annotationError = message;
    },
    onDeleteError: (message) => {
      annotationError = message;
    },
  });
  const positionAutosave = createReaderPositionAutosave<{
    pageIndex: number;
    fullPagePosition: number;
    progress: number;
  }>({
    debounceMs: 500,
    areEqual: (left, right) =>
      left.pageIndex === right.pageIndex &&
      left.fullPagePosition === right.fullPagePosition &&
      left.progress === right.progress,
    persist: ({ pageIndex, fullPagePosition, progress }) => {
      if (book.savePdfPage) {
        return book.savePdfPage(pageIndex, fullPagePosition, progress);
      }
      return book.saveCfi(pageLocator(pageIndex), progress);
    },
  });
  let shellNodes: Array<HTMLElement | undefined> = [];
  let lastViewportSize = { width: 0, height: 0 };
  const renderTasks = new Map<number, any>();
  const pageObjects = new Map<number, any>();
  const atomicOffsets = new Map<number, number[]>();
  const textDivsByPage = new Map<number, HTMLElement[]>();
  const PAGE_GAP = 12;
  const PDF_VIEWER_PREFERENCES_KEY = "bokheim:pdf-viewer-preferences:v1";
  const outlinePanelItems = $derived(tocPanelItems(outlineItems, expandedOutlineKeys));

  function loadViewerPreferences() {
    try {
      const stored = JSON.parse(
        localStorage.getItem(PDF_VIEWER_PREFERENCES_KEY) ?? "null",
      ) as Partial<{ fitMode: FitMode; zoom: number; pageColumns: number }> | null;
      if (
        stored?.fitMode === "width" ||
        stored?.fitMode === "zoom" ||
        stored?.fitMode === "height"
      )
        fitMode = stored.fitMode;
      if (typeof stored?.zoom === "number" && Number.isFinite(stored.zoom))
        zoom = Math.max(PDF_MIN_ZOOM, stored.zoom);
      if (typeof stored?.pageColumns === "number" && Number.isFinite(stored.pageColumns))
        pageColumns = Math.max(
          1,
          Math.min(PDF_MAX_PAGE_COLUMNS, Math.round(stored.pageColumns)),
        );
    } catch {
      // Ignore malformed preferences and keep the reader defaults.
    }
  }

  function persistViewerPreferences() {
    try {
      localStorage.setItem(
        PDF_VIEWER_PREFERENCES_KEY,
        JSON.stringify({ fitMode, zoom, pageColumns }),
      );
    } catch {
      // Reading must keep working when storage is unavailable.
    }
  }

  function visibleLastPage() {
    return Math.min(pages.length, currentPage + visibleColumns());
  }

  function pageIndicator() {
    if (!pages.length) return "0/0";
    const last = visibleLastPage();
    return last === currentPage + 1
      ? `${last}/${pages.length}`
      : `${currentPage + 1}–${last}/${pages.length}`;
  }

  function progressLabel() {
    return pages.length
      ? `${Math.round((visibleLastPage() / pages.length) * 100)}%`
      : "0%";
  }

  function pdfAnchor(annotation: ReaderAnnotation) {
    return annotation.anchor.kind === "pdf" ? annotation.anchor : null;
  }

  function annotationsForPage(pageIndex: number) {
    return annotations.filter(
      (annotation) => pdfAnchor(annotation)?.page_index === pageIndex,
    );
  }

  function searchResultsForPage(pageIndex: number) {
    return searchResults
      .map((result, resultIndex) => ({ result, resultIndex }))
      .filter(({ result }) => result.pageIndex === pageIndex);
  }

  function bookmarkPage(cfi: string) {
    return pageFromLocator(cfi);
  }

  function clearPdfSelection() {
    window.getSelection()?.removeAllRanges();
    selection = null;
  }

  function capturePdfSelection() {
    const selected = window.getSelection();
    if (!selected?.rangeCount || selected.isCollapsed) {
      selection = null;
      return;
    }
    const range = selected.getRangeAt(0);
    const start =
      range.startContainer instanceof Element
        ? range.startContainer
        : range.startContainer.parentElement;
    const layer = start?.closest<HTMLElement>(".pdf-text-layer");
    const pageShell = layer?.closest<HTMLElement>(".pdf-page");
    const pageIndex = Number(pageShell?.dataset.pageIndex);
    const text = selected.toString().trim();
    if (!pageShell || !Number.isInteger(pageIndex) || !text) return;
    const bounds = pageShell.getBoundingClientRect();
    const clientRects = Array.from(range.getClientRects()).filter(
      (rect) =>
        rect.width > 0 &&
        rect.height > 0 &&
        rect.bottom > bounds.top &&
        rect.top < bounds.bottom &&
        rect.right > bounds.left &&
        rect.left < bounds.right,
    );
    const rects = clientRects
      .map((rect) => {
        const left = Math.max(bounds.left, rect.left);
        const top = Math.max(bounds.top, rect.top);
        const right = Math.min(bounds.right, rect.right);
        const bottom = Math.min(bounds.bottom, rect.bottom);
        return {
          left: (left - bounds.left) / bounds.width,
          top: (top - bounds.top) / bounds.height,
          width: (right - left) / bounds.width,
          height: (bottom - top) / bounds.height,
        };
      })
      .filter((rect) => rect.width > 0 && rect.height > 0);
    if (!rects.length) return;
    const left = Math.min(...clientRects.map((rect) => rect.left));
    const top = Math.min(...clientRects.map((rect) => rect.top));
    const right = Math.max(...clientRects.map((rect) => rect.right));
    const bottom = Math.max(...clientRects.map((rect) => rect.bottom));
    selection = {
      pageIndex,
      text,
      rects,
      viewportRect: new DOMRect(left, top, right - left, bottom - top),
    };
  }

  async function saveSelection(openEditor: boolean) {
    if (!selection || !book.bookId || !book.upsertAnnotation) return;
    annotationError = "";
    const now = Date.now();
    const selected = selection;
    const annotation: ReaderAnnotation = {
      id: crypto.randomUUID(),
      content_hash: book.bookId,
      anchor: {
        kind: "pdf",
        page_index: selected.pageIndex,
        rects: selected.rects,
        fallback_cfi: pageLocator(selected.pageIndex),
      },
      exact_text: selected.text,
      style: "highlight",
      color: "#f6c945",
      note: "",
      created_at: now,
      modified_at: now,
    };
    try {
      await book.upsertAnnotation(annotation);
      annotations = [annotation, ...annotations];
      clearPdfSelection();
      if (openEditor) openAnnotation(annotation);
    } catch (reason) {
      annotationError = reason instanceof Error ? reason.message : String(reason);
    }
  }

  function annotationViewportRect(annotation: ReaderAnnotation) {
    const anchor = pdfAnchor(annotation);
    const pageShell = anchor ? shellNodes[anchor.page_index] : undefined;
    if (!anchor?.rects.length || !pageShell) return null;
    const page = pageShell.getBoundingClientRect();
    const left = Math.min(...anchor.rects.map((rect) => rect.left));
    const top = Math.min(...anchor.rects.map((rect) => rect.top));
    const right = Math.max(...anchor.rects.map((rect) => rect.left + rect.width));
    const bottom = Math.max(...anchor.rects.map((rect) => rect.top + rect.height));
    return new DOMRect(
      page.left + left * page.width,
      page.top + top * page.height,
      (right - left) * page.width,
      (bottom - top) * page.height,
    );
  }

  function annotationPopupStyle() {
    if (!activeAnnotation) return "";
    const rect = annotationViewportRect(activeAnnotation);
    const width = Math.min(360, window.innerWidth - 24);
    const anchor = rect
      ? {
          left: rect.left,
          top: rect.top,
          right: rect.right,
          bottom: rect.bottom,
        }
      : centerAnchor({
          left: 0,
          top: 0,
          right: window.innerWidth,
          bottom: window.innerHeight,
        });
    const position = placeAnnotationPopup(
      anchor,
      { width, height: 300 },
      { width: window.innerWidth, height: window.innerHeight },
    );
    return `left:${position.x}px;top:${position.y}px;width:${width}px;`;
  }

  async function openAnnotation(annotation: ReaderAnnotation) {
    if (activeAnnotation && activeAnnotation.id !== annotation.id) {
      closeAnnotation();
      await tick();
    }
    if (sidebarOpen && window.matchMedia("(max-width: 720px)").matches) {
      sidebarOpen = false;
      await tick();
    }
    const anchor = pdfAnchor(annotation);
    if (anchor) goToPage(anchor.page_index);
    activeAnnotation = annotation;
    annotationNote = annotation.note;
    annotationController.beginEditing(annotation);
  }

  const handleOpenAnnotation = (annotation: ReaderAnnotation) => {
    void openAnnotation(annotation);
  };

  function closeAnnotation() {
    annotationController.closeActiveAnnotation();
  }

  function deleteActiveAnnotation() {
    annotationController.deleteActiveAnnotation();
  }

  function deleteAnnotation(annotation: ReaderAnnotation) {
    annotationController.deleteAnnotation(annotation);
  }

  function setPdfAnnotationNote(note: string) {
    annotationNote = note;
    annotationController.updateDraft(note);
  }

  function setActiveAnnotationStyle(nextStyle: ReaderAnnotationStyle) {
    annotationController.setActiveAnnotationStyle(nextStyle);
  }

  function setActiveAnnotationColor(nextColor: string) {
    annotationController.setActiveAnnotationColor(nextColor);
  }

  async function addBookmark() {
    if (!book.bookId || !book.createBookmark) return;
    const now = Date.now();
    const bookmark: ReaderBookmark = {
      id: crypto.randomUUID(),
      content_hash: book.bookId,
      cfi: `epubcfi(/6/${(currentPage + 1) * 2})`,
      label: `Page ${currentPage + 1}`,
      created_at: now,
      modified_at: now,
    };
    try {
      await book.createBookmark(bookmark);
      bookmarks = [bookmark, ...bookmarks];
    } catch (reason) {
      annotationError = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function deleteBookmark(bookmark: ReaderBookmark) {
    if (!book.deleteBookmark) return;
    try {
      await book.deleteBookmark(bookmark.id, Date.now());
      bookmarks = bookmarks.filter((item) => item.id !== bookmark.id);
    } catch (reason) {
      annotationError = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function openBookmark(bookmark: ReaderBookmark) {
    if (sidebarOpen && window.matchMedia("(max-width: 720px)").matches) {
      sidebarOpen = false;
      await tick();
    }
    goToPage(bookmarkPage(bookmark.cfi));
  }

  function sourceName() {
    if (typeof File !== "undefined" && book.file instanceof File) return book.file.name;
    return book.fileName ?? book.title;
  }

  /**
   * Set when the reader opens a native PDF range session, so a failed
   * background range read can abort the reader instead of leaving it hung
   * (see `createNativePdfRangeTransport`). Watched for the lifetime of the
   * component, not just during the initial open.
   */
  let pdfRangeFatal: Promise<never> | undefined;

  async function openSource(pdfjs: any) {
    pdfjs.GlobalWorkerOptions.workerSrc = new URL(
      "/pdfjs/pdf.worker.mjs",
      globalThis.location.href,
    ).toString();
    const options = {
      cMapUrl: new URL("/pdfjs/cmaps/", globalThis.location.href).toString(),
      standardFontDataUrl: new URL(
        "/pdfjs/standard_fonts/",
        globalThis.location.href,
      ).toString(),
      isEvalSupported: false,
    };
    if (isNativePdfRangeSource(book.file)) {
      // Read PDF bytes over Tauri IPC rather than the asset protocol: some
      // Android system WebViews can't give the PDF.js worker direct access to
      // Tauri's asset:// scheme for byte-range requests, which otherwise left
      // PDFs unable to open on Android.
      const { range, rangeChunkSize, fatal } = createNativePdfRangeTransport(
        pdfjs,
        book.file,
      );
      pdfRangeFatal = fatal;
      const task = pdfjs.getDocument({ ...options, range, rangeChunkSize });
      return await Promise.race([task.promise, fatal]);
    }
    if (typeof book.file === "string") {
      const direct = pdfjs.getDocument({ ...options, url: book.file });
      try {
        // Keep URL/range loading as the normal path. It avoids buffering the
        // entire document and lets PDF.js request only the bytes it needs.
        return await direct.promise;
      } catch (reason) {
        await direct.destroy?.().catch?.(() => undefined);
        if (!isNativeApp) throw reason;
        // Some Android WebViews cannot give the PDF.js worker direct access to
        // Tauri's asset protocol. Buffering is a compatibility fallback only.
        const response = await fetch(book.file);
        if (!response.ok) throw new Error(`Could not read the PDF (${response.status})`);
        return pdfjs.getDocument({ ...options, data: await response.arrayBuffer() })
          .promise;
      }
    }
    if (!(book.file instanceof File)) throw new Error("PDF source is not a local file");
    return pdfjs.getDocument({ ...options, data: await book.file.arrayBuffer() }).promise;
  }

  function currentLayout() {
    const { width, height } = getPdfSurfaceSize(viewport, PDF_SURFACE_INSET);
    return {
      ...resolvePdfLayout({
        pages,
        currentPage,
        fitMode,
        zoom,
        pageColumns,
        surfaceWidth: width,
        surfaceHeight: height,
        columnGap: PDF_COLUMN_GAP,
        maxPageColumns: PDF_MAX_PAGE_COLUMNS,
        minZoom: PDF_MIN_ZOOM,
      }),
      surfaceWidth: width,
      surfaceHeight: height,
    };
  }

  /// How many pages are actually side by side right now: chosen directly in
  /// fit-width, derived from the zoom otherwise.
  function visibleColumns() {
    return currentLayout().columns;
  }

  /// Every page is drawn at the resolved column width, so a document with
  /// mixed page sizes still lines up in the grid.
  function layoutScale(page: PdfPageState) {
    return pdfLayoutScale(page, currentLayout().columnWidth, PDF_MIN_ZOOM);
  }

  function updatePageLayout() {
    pages = pages.map((page, index) => {
      const scale = layoutScale(page);
      const displayWidth = page.width * scale;
      const displayHeight = page.height * scale;
      const textLayer = shellNodes[index]?.querySelector<HTMLElement>(".pdf-text-layer");
      if (textLayer) {
        textLayer.style.setProperty("--scale-factor", String(scale));
        textLayer.style.setProperty(
          "--user-unit",
          String(pageObjects.get(index)?.userUnit || 1),
        );
        textLayer.style.width = `${displayWidth}px`;
        textLayer.style.height = `${displayHeight}px`;
      }
      return { ...page, displayWidth, displayHeight };
    });
  }

  function shell(node: HTMLElement, index: number) {
    shellNodes[index] = node;
    intersectionObserver?.observe(node);
    return {
      destroy() {
        intersectionObserver?.unobserve(node);
        if (shellNodes[index] === node) shellNodes[index] = undefined;
      },
    };
  }

  async function pdfPage(index: number) {
    let page = pageObjects.get(index);
    if (!page) {
      page = await pdfDocument.getPage(index + 1);
      pageObjects.set(index, page);
    }
    return page;
  }

  function collectAtomicOffsets(
    index: number,
    textLayer: HTMLElement,
    pageShell: HTMLElement,
  ) {
    const shellRect = pageShell.getBoundingClientRect();
    const offsets = Array.from(textLayer.querySelectorAll<HTMLElement>("span"))
      .map((span) => span.getBoundingClientRect())
      .filter((rect) => rect.width > 0 && rect.height > 0)
      .map((rect) => Math.max(0, rect.top - shellRect.top))
      .sort((left, right) => left - right)
      .filter(
        (offset, position, values) =>
          position === 0 || Math.abs(offset - values[position - 1]) > 2,
      );
    atomicOffsets.set(index, offsets);
  }

  function refreshSearchRects(pageIndex: number) {
    const textDivs = textDivsByPage.get(pageIndex);
    const pageShell = shellNodes[pageIndex];
    if (!textDivs || !pageShell) return;
    const shellRect = pageShell.getBoundingClientRect();
    let changed = false;
    const next = searchResults.map((result) => {
      if (result.pageIndex !== pageIndex) return result;
      const startDiv = textDivs[result.startItem];
      const endDiv = textDivs[result.endItem];
      const startNode = startDiv?.firstChild;
      const endNode = endDiv?.firstChild;
      if (!startNode || !endNode) return result;
      try {
        const range = document.createRange();
        range.setStart(
          startNode,
          Math.min(result.startOffset, startNode.textContent?.length ?? 0),
        );
        range.setEnd(
          endNode,
          Math.min(result.endOffset, endNode.textContent?.length ?? 0),
        );
        const rects = Array.from(range.getClientRects())
          .filter((rect) => rect.width > 0 && rect.height > 0)
          .map((rect) => ({
            left: Math.max(0, (rect.left - shellRect.left) / shellRect.width),
            top: Math.max(0, (rect.top - shellRect.top) / shellRect.height),
            width: Math.min(1, rect.width / shellRect.width),
            height: Math.min(1, rect.height / shellRect.height),
          }));
        changed ||= JSON.stringify(rects) !== JSON.stringify(result.rects);
        return { ...result, rects };
      } catch {
        return result;
      }
    });
    if (changed) searchResults = next;
  }

  function refreshRenderedSearchRects() {
    for (const pageIndex of textDivsByPage.keys()) refreshSearchRects(pageIndex);
  }

  async function renderPage(index: number) {
    const state = pages[index];
    const pageShell = shellNodes[index];
    if (!state || !pageShell || state.rendering) return;
    const scale = state.displayWidth / Math.max(1, state.width);
    if (
      Math.abs(state.renderedScale - scale) < 0.001 &&
      pageShell.querySelector("canvas")
    )
      return;

    pages[index] = { ...state, rendering: true };
    pages = pages;
    try {
      const page = await pdfPage(index);
      if (disposed || shellNodes[index] !== pageShell) return;
      renderTasks.get(index)?.cancel?.();
      const canvas = pageShell.querySelector<HTMLCanvasElement>("canvas");
      const textLayer = pageShell.querySelector<HTMLElement>(".pdf-text-layer");
      if (!canvas || !textLayer) return;
      const pixelRatio = Math.min(window.devicePixelRatio || 1, 2);
      const renderViewport = page.getViewport({ scale: scale * pixelRatio });
      const displayViewport = page.getViewport({ scale });
      canvas.width = Math.max(1, Math.floor(renderViewport.width));
      canvas.height = Math.max(1, Math.floor(renderViewport.height));
      canvas.style.width = `${displayViewport.width}px`;
      canvas.style.height = `${displayViewport.height}px`;
      const context = canvas.getContext("2d", { alpha: false });
      if (!context) throw new Error("Canvas rendering is unavailable");
      const task = page.render({ canvasContext: context, viewport: renderViewport });
      renderTasks.set(index, task);
      await task.promise;
      if (disposed || shellNodes[index] !== pageShell) return;

      textLayer.replaceChildren();
      textLayer.style.width = `${displayViewport.width}px`;
      textLayer.style.height = `${displayViewport.height}px`;
      textLayer.style.setProperty("--scale-factor", String(scale));
      textLayer.style.setProperty("--user-unit", String(page.userUnit || 1));
      const layer = new pdfjsApi.TextLayer({
        textContentSource: await page.streamTextContent(),
        container: textLayer,
        viewport: displayViewport,
      });
      await layer.render();
      textDivsByPage.set(
        index,
        Array.from(
          (layer.textDivs as HTMLElement[] | null | undefined) ??
            textLayer.querySelectorAll<HTMLElement>("span"),
        ),
      );
      collectAtomicOffsets(index, textLayer, pageShell);
      refreshSearchRects(index);
      pages[index] = { ...pages[index], rendering: false, renderedScale: scale };
      pages = pages;
    } catch (reason) {
      if (
        (reason as { name?: string })?.name !== "RenderingCancelledException" &&
        !disposed
      ) {
        console.error(`Could not render PDF page ${index + 1}`, reason);
      }
      if (pages[index]) {
        pages[index] = { ...pages[index], rendering: false };
        pages = pages;
      }
    } finally {
      renderTasks.delete(index);
    }
  }

  function renderVisiblePages() {
    for (const [index, node] of shellNodes.entries()) {
      if (!node) continue;
      const bounds = node.getBoundingClientRect();
      const viewportBounds = viewport.getBoundingClientRect();
      if (
        bounds.bottom >= viewportBounds.top - viewportBounds.height &&
        bounds.top <= viewportBounds.bottom + viewportBounds.height
      )
        void renderPage(index);
    }
  }

  function releaseDistantPages() {
    for (const [index, node] of shellNodes.entries()) {
      if (!node || Math.abs(index - currentPage) <= 3) continue;
      renderTasks.get(index)?.cancel?.();
      const canvas = node.querySelector<HTMLCanvasElement>("canvas");
      if (canvas) {
        canvas.width = 1;
        canvas.height = 1;
      }
      node.querySelector(".pdf-text-layer")?.replaceChildren();
      atomicOffsets.delete(index);
      textDivsByPage.delete(index);
      if (pages[index]?.renderedScale) {
        pages[index] = { ...pages[index], renderedScale: 0, rendering: false };
        pages = pages;
      }
    }
  }

  function rowStartForScrollTop(scrollTop = viewport.scrollTop) {
    return pdfRowStartForScrollTop(scrollTop, pages, visibleColumns(), shellNodes);
  }

  function estimateRowLineHeight(rowStart: number, columns: number) {
    return estimateRowLineHeightForRows(rowStart, columns, pages, atomicOffsets);
  }

  function nearestPage() {
    return rowStartForScrollTop();
  }

  function schedulePositionSave() {
    positionAutosave.schedule({
      pageIndex: currentPage,
      fullPagePosition: pageFullPositionForPage(currentPage),
      progress: pages.length ? ((currentPage + 1) / pages.length) * 100 : 0,
    });
  }

  async function flushPosition() {
    await positionAutosave.flush();
  }

  function handleScroll() {
    if (scrollFrame !== undefined) return;
    scrollFrame = requestAnimationFrame(() => {
      scrollFrame = undefined;
      const next = nearestPage();
      if (next !== currentPage) {
        currentPage = next;
        schedulePositionSave();
        releaseDistantPages();
      }
      renderVisiblePages();
    });
  }

  function goToPage(index: number) {
    goToPageAt(index, 0);
  }

  function pageFullPageHeight(index: number): number {
    const state = pages[index];
    if (!state || !state.height) return 0;
    return state.height;
  }

  function pageDisplayHeightForPage(index: number): number {
    const state = pages[index];
    const shell = shellNodes[index];
    return state?.displayHeight ?? shell?.offsetHeight ?? 0;
  }

  function pageFullPositionForPage(index: number): number {
    const pageStateHeight = pageFullPageHeight(index);
    if (!pageStateHeight) return 0;
    const node = shellNodes[index];
    if (!node) return 0;
    const pageScrollY = viewport.scrollTop - node.offsetTop;
    const pageHeightForDisplay = Math.max(1, pageDisplayHeightForPage(index));
    const fullPosition =
      (Math.max(0, pageScrollY) / pageHeightForDisplay) * pageStateHeight;
    const clamped = Math.min(pageStateHeight, fullPosition);
    return Number.isFinite(clamped) ? clamped : 0;
  }

  function pageTopForFullPosition(index: number, fullPagePosition: number): number {
    const node = shellNodes[index];
    if (!node) return 0;
    const fullHeight = pageFullPageHeight(index);
    const displayHeight = Math.max(1, pageDisplayHeightForPage(index));
    if (!fullHeight) return node.offsetTop;
    const normalized = Math.max(0, fullPagePosition) / fullHeight;
    return (
      node.offsetTop + Math.min(displayHeight, Math.max(0, normalized * displayHeight))
    );
  }

  function goToPageAt(index: number, fullPagePosition = 0) {
    const target = Math.max(0, Math.min(pages.length - 1, index));
    const columns = visibleColumns();
    const rowStart = Math.floor(target / columns) * columns;
    const node = shellNodes[target];
    if (!node) return;
    currentPage = rowStart;
    viewport.scrollTo({
      top: pageTopForFullPosition(target, fullPagePosition),
      behavior: "auto",
    });
    void renderPage(target);
    if (rowStart !== target) void renderPage(rowStart);
    schedulePositionSave();
  }

  // A PDF outline is a tree. It is flattened for rendering but each entry keeps
  // its ancestry, so the panel can collapse branches like the EPUB contents do.
  function flattenOutline(
    items: any[] | null | undefined,
    depth = 0,
    parentKeys: string[] = [],
    result: OutlineItem[] = [],
  ) {
    if (!items) return result;
    for (const [index, item] of items.entries()) {
      const key = `${parentKeys.join("/") || "root"}/${index}-${item?.title ?? ""}`;
      if (item?.title)
        result.push({
          key,
          label: String(item.title),
          destination: item.dest,
          depth,
          parentKeys,
          hasChildren: Boolean(item?.items?.length),
          // Outline entries may exist only to group their children.
          disabled: !item.dest,
        });
      if (item?.items?.length)
        flattenOutline(item.items, depth + 1, [...parentKeys, key], result);
    }
    return result;
  }

  async function goToOutline(item: OutlineItem) {
    try {
      const destination =
        typeof item.destination === "string"
          ? await pdfDocument.getDestination(item.destination)
          : item.destination;
      if (!Array.isArray(destination) || !destination[0]) return;
      goToPage(await pdfDocument.getPageIndex(destination[0]));
      sidebarOpen = false;
    } catch (reason) {
      console.warn("Could not open PDF outline destination", reason);
    }
  }

  async function runSearch() {
    const query = searchQuery.trim().toLocaleLowerCase();
    const generation = ++searchGeneration;
    searchResults = [];
    selectedSearchResult = -1;
    if (query.length < 2 || !pdfDocument) {
      searching = false;
      return;
    }
    searching = true;
    try {
      const results: SearchResult[] = [];
      for (
        let index = 0;
        index < pages.length && generation === searchGeneration;
        index += 1
      ) {
        const content = await (await pdfPage(index)).getTextContent();
        const textItems = content.items.filter(
          (item: { str?: unknown }) => typeof item.str === "string",
        ) as Array<{ str: string }>;
        const segments: Array<{ start: number; end: number; item: number }> = [];
        let text = "";
        for (const [item, textItem] of textItems.entries()) {
          if (item) text += " ";
          const start = text.length;
          text += textItem.str;
          segments.push({ start, end: text.length, item });
        }
        const searchable = text.toLocaleLowerCase();
        let match = searchable.indexOf(query);
        while (match >= 0) {
          const matchEnd = match + query.length;
          const startSegment =
            segments.find((segment) => segment.end > match) ?? segments.at(-1);
          const endSegment =
            [...segments].reverse().find((segment) => segment.start < matchEnd) ??
            startSegment;
          if (startSegment && endSegment) {
            results.push({
              pageIndex: index,
              excerpt: text
                .slice(Math.max(0, match - 45), Math.min(text.length, matchEnd + 75))
                .replace(/\s+/g, " ")
                .trim(),
              startItem: startSegment.item,
              endItem: endSegment.item,
              startOffset: Math.max(0, match - startSegment.start),
              endOffset: Math.max(
                0,
                Math.min(endSegment.end, matchEnd) - endSegment.start,
              ),
              rects: [],
            });
          }
          match = searchable.indexOf(query, match + Math.max(1, query.length));
        }
        if (index % 8 === 0)
          await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      }
      if (generation !== searchGeneration) return;
      searchResults = results;
      refreshRenderedSearchRects();
      if (results.length) selectSearchResult(0);
    } catch (reason) {
      console.warn("Could not search PDF", reason);
    } finally {
      if (generation === searchGeneration) searching = false;
    }
  }

  function handleSearchInput(nextQuery = searchQuery) {
    searchQuery = nextQuery;
    if (searchTimer) clearTimeout(searchTimer);
    searchGeneration += 1;
    searchResults = [];
    selectedSearchResult = -1;
    searching = false;
    if (searchQuery.trim().length < 2) return;
    searchTimer = setTimeout(() => {
      searchTimer = undefined;
      void runSearch();
    }, 180);
  }

  function closeSearch() {
    searchGeneration += 1;
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = undefined;
    searchOpen = false;
    searchQuery = "";
    searchResults = [];
    selectedSearchResult = -1;
    searching = false;
  }

  function selectSearchResult(index: number) {
    if (!searchResults.length) return;
    selectedSearchResult = (index + searchResults.length) % searchResults.length;
    const result = searchResults[selectedSearchResult];
    goToPage(result.pageIndex);
    setTimeout(() => {
      refreshSearchRects(result.pageIndex);
      const current = searchResults[selectedSearchResult];
      const node = shellNodes[current?.pageIndex];
      const rect = current?.rects[0];
      if (node && rect)
        viewport.scrollTo({
          top: Math.max(0, node.offsetTop + rect.top * node.offsetHeight - 72),
          behavior: "auto",
        });
    });
  }

  function revealChromeAtTopEdge(event: PointerEvent) {
    if (!chromeVisible && event.pointerType === "mouse" && event.clientY <= 8)
      void toggleChrome(true);
  }

  function isWithinOpenPanel(target: EventTarget | null) {
    return (
      target instanceof Element &&
      target.closest(
        ".reader-sidebar,.pdf-search,.annotation-editor,.reader-topbar,.selection-actions",
      ) !== null
    );
  }

  async function toggleChrome(next = !chromeVisible) {
    if (!viewport) {
      chromeVisible = next;
      return;
    }
    const preservedBottom = viewport.scrollTop + viewport.clientHeight;
    chromeVisible = next;
    await tick();
    const maxTop = Math.max(0, viewport.scrollHeight - viewport.clientHeight);
    const nextTop = clamp(preservedBottom - viewport.clientHeight, 0, maxTop);
    viewport.scrollTo({ top: nextTop, behavior: "auto" });
  }

  function handleViewportPointerDown(event: PointerEvent) {
    if (activeAnnotation) {
      closeAnnotation();
      tapStart = null;
      return;
    }
    if (sidebarOpen || searchOpen || settingsOpen) {
      if (isWithinOpenPanel(event.target)) return;
      sidebarOpen = false;
      searchOpen = false;
      settingsOpen = false;
      tapStart = null;
      return;
    }
    tapStart =
      event.isPrimary && event.button === 0
        ? {
            pointerId: event.pointerId,
            x: event.clientX,
            y: event.clientY,
            time: event.timeStamp,
            moved: false,
          }
        : null;
  }

  function handleViewportPointerMove(event: PointerEvent) {
    if (
      tapStart?.pointerId === event.pointerId &&
      Math.hypot(event.clientX - tapStart.x, event.clientY - tapStart.y) > 12
    ) {
      tapStart.moved = true;
    }
    revealChromeAtTopEdge(event);
  }

  function handleViewportPointerUp(event: PointerEvent) {
    const start = tapStart;
    tapStart = null;
    setTimeout(capturePdfSelection);
    if (!start || start.pointerId !== event.pointerId) return;
    const movement = Math.hypot(event.clientX - start.x, event.clientY - start.y);
    const duration = event.timeStamp - start.time;
    const target = event.target as Element | null;
    if (
      start.moved ||
      movement > 12 ||
      duration > 500 ||
      target?.closest?.("a,button,input,textarea,select,img,svg") ||
      window.getSelection()?.type === "Range"
    )
      return;
    if (pendingChromeToggle) clearTimeout(pendingChromeToggle);
    pendingChromeToggle = setTimeout(() => {
      pendingChromeToggle = undefined;
      void toggleChrome();
    });
  }

  function togglePanel(panel: "sidebar" | "search" | "settings") {
    const next =
      panel === "sidebar"
        ? !sidebarOpen
        : panel === "search"
          ? !searchOpen
          : !settingsOpen;
    sidebarOpen = panel === "sidebar" && next;
    searchOpen = panel === "search" && next;
    settingsOpen = panel === "settings" && next;
  }

  function showSidebar(tab: "contents" | "highlights" | "bookmarks") {
    sidebarTab = tab;
    sidebarOpen = true;
    searchOpen = false;
    settingsOpen = false;
  }

  function lineOffsetsInDocument() {
    return getLineOffsetsInDocument(shellNodes, atomicOffsets);
  }

  function clamp(value: number, min: number, max: number) {
    return clampValue(value, min, max);
  }

  function moveAtomic(direction: -1 | 1) {
    const current = viewport.scrollTop;
    const offsets = lineOffsetsInDocument();
    const target =
      direction > 0
        ? offsets.find((offset) => offset > current + 2)
        : [...offsets].reverse().find((offset) => offset < current - 2);
    if (target !== undefined) viewport.scrollTo({ top: target, behavior: "auto" });
  }

  function moveViewport(direction: -1 | 1) {
    const current = viewport.scrollTop;
    const viewportHeight = Math.max(1, viewport.clientHeight);
    const columns = visibleColumns();
    const rowStart = rowStartForScrollTop(current);
    const rowLineHeight = estimateRowLineHeight(rowStart, columns);
    const overlap = Math.min(
      Math.max(0, viewportHeight - 1),
      Math.round(PDF_VIEWPORT_SCROLL_OVERLAP_LINES * rowLineHeight),
    );
    const intended = current + direction * Math.max(1, viewportHeight - overlap);
    const rowStartOffset = shellNodes[rowStart]?.offsetTop ?? 0;
    const rowStartNode = shellNodes[rowStart];
    const nextRowOffset = shellNodes[rowStart + columns]?.offsetTop;
    const prevRowOffset = shellNodes[rowStart - columns]?.offsetTop;
    const rowBottomScrollTop = Math.max(
      rowStartOffset,
      (nextRowOffset ?? rowStartOffset + (rowStartNode?.offsetHeight ?? viewportHeight)) -
        viewportHeight,
    );
    const offsets = lineOffsetsInDocument();
    const rowOffsets = offsets.filter(
      (offset) => offset > rowStartOffset && offset < rowBottomScrollTop,
    );
    const byLines =
      direction > 0
        ? rowOffsets.filter((offset) => offset <= intended).at(-1)
        : rowOffsets.filter((offset) => offset >= intended).at(0);

    if (byLines !== undefined) {
      viewport.scrollTo({
        top: clamp(byLines, rowStartOffset, rowBottomScrollTop),
        behavior: "auto",
      });
      return;
    }

    const clampTarget = clamp(intended, rowStartOffset, rowBottomScrollTop);
    if (direction > 0 ? clampTarget > current + 2 : clampTarget < current - 2) {
      viewport.scrollTo({ top: clampTarget, behavior: "auto" });
      return;
    }

    if (
      direction > 0 &&
      nextRowOffset !== undefined &&
      rowStartOffset !== nextRowOffset
    ) {
      viewport.scrollTo({ top: nextRowOffset, behavior: "auto" });
      return;
    }
    if (
      direction < 0 &&
      prevRowOffset !== undefined &&
      rowStartOffset !== prevRowOffset
    ) {
      viewport.scrollTo({ top: prevRowOffset, behavior: "auto" });
    }
  }

  function moveVolumePage(direction: -1 | 1) {
    const rowStarts = shellNodes
      .map((node, index) => ({ node, index }))
      .filter(({ node, index }) => node && index % visibleColumns() === 0) as Array<{
      node: HTMLElement;
      index: number;
    }>;
    if (!rowStarts.length) return;

    const scrollTop = viewport.scrollTop;
    const current =
      [...rowStarts].reverse().find(({ node }) => node.offsetTop <= scrollTop + 2) ??
      rowStarts[0];
    if (direction > 0) {
      const next = rowStarts.find(
        ({ node }) => node.offsetTop > current.node.offsetTop + 2,
      );
      if (next) goToPage(next.index);
      return;
    }

    // When part-way through a page, volume-up aligns its beginning first.
    // A subsequent press from that boundary moves to the previous page.
    if (scrollTop > current.node.offsetTop + 2) {
      goToPage(current.index);
      return;
    }
    const previous = [...rowStarts]
      .reverse()
      .find(({ node }) => node.offsetTop < current.node.offsetTop - 2);
    if (previous) goToPage(previous.index);
  }

  function handleKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      if (activeAnnotation) closeAnnotation();
      else if (selection) clearPdfSelection();
      else if (sidebarOpen || searchOpen || settingsOpen) {
        sidebarOpen = false;
        searchOpen = false;
        settingsOpen = false;
      } else onclose();
      event.preventDefault();
      return;
    }
    if (
      (event.target as Element | null)?.closest?.(
        "input,textarea,select,[contenteditable='true']",
      )
    )
      return;
    if (event.shiftKey && event.key.startsWith("Arrow")) {
      setTimeout(capturePdfSelection);
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveAtomic(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      moveAtomic(-1);
    } else if (event.key === "PageDown" || event.key === " ") {
      event.preventDefault();
      moveViewport(1);
    } else if (event.key === "PageUp") {
      event.preventDefault();
      moveViewport(-1);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      moveViewport(1);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      moveViewport(-1);
    }
  }

  async function relayout(nextMode = fitMode, nextZoom = zoom) {
    const anchor = currentPage;
    fitMode = nextMode;
    zoom = Math.max(
      PDF_MIN_ZOOM,
      Math.min(widthFitZoom(1), Math.round(nextZoom * 10) / 10),
    );
    persistViewerPreferences();
    for (const task of renderTasks.values()) task.cancel?.();
    pages = pages.map((page) => ({ ...page, renderedScale: 0, rendering: false }));
    updatePageLayout();
    await tick();
    goToPage(anchor);
    renderVisiblePages();
  }

  function adjustPageColumns(delta: number) {
    pageColumns = Math.max(
      1,
      Math.min(PDF_MAX_PAGE_COLUMNS, resolveLayout().columns + delta),
    );
    persistViewerPreferences();
    void relayout("width", zoom);
  }

  /// Nudging the zoom also switches to the mode where the zoom is what is held
  /// fixed and the page count follows.
  function adjustZoom(delta: number) {
    void relayout("zoom", resolveLayout().zoom + delta);
  }

  function resolveLayout() {
    return currentLayout();
  }

  function widthFitZoom(columns: number) {
    const layout = currentLayout();
    const page = pages[currentPage] ?? pages[0];
    return pdfWidthFitZoom({
      surfaceWidth: layout.surfaceWidth,
      columns,
      referenceWidth: Math.max(1, page?.width ?? 1),
      minZoom: PDF_MIN_ZOOM,
    });
  }

  const onTogglePanel = togglePanel;

  onMount(() => {
    disposed = false;
    loadViewerPreferences();
    documentTitle = book.title;
    if (window.BokheimAndroid) chromeVisible = false;
    enableReaderImmersiveMode(true);
    enableVolumePageTurning(true);
    const unlistenVolume = onVolumePageTurn((direction) =>
      moveVolumePage(direction === "next" ? 1 : -1),
    );
    const handleVisibilityChange = () => {
      if (document.visibilityState === "hidden") {
        void flushPosition();
        return;
      }
      enableVolumePageTurning(true);
      enableReaderImmersiveMode(true);
    };
    const handleWindowFocus = () => {
      enableVolumePageTurning(true);
      enableReaderImmersiveMode(true);
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("focus", handleWindowFocus);
    window.addEventListener("keydown", handleKey);
    window.addEventListener("keyup", capturePdfSelection);

    if (book.loadAnnotations) {
      void book
        .loadAnnotations()
        .then((loaded) => {
          if (!disposed)
            annotations = loaded.filter((annotation) => annotation.anchor.kind === "pdf");
        })
        .catch((reason) => {
          if (!disposed)
            annotationError = reason instanceof Error ? reason.message : String(reason);
        });
    }
    if (book.loadBookmarks) {
      void book
        .loadBookmarks()
        .then((loaded) => {
          if (!disposed) bookmarks = loaded;
        })
        .catch((reason) => {
          if (!disposed)
            annotationError = reason instanceof Error ? reason.message : String(reason);
        });
    }

    intersectionObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries)
          if (entry.isIntersecting) {
            const index = Number((entry.target as HTMLElement).dataset.pageIndex);
            if (Number.isInteger(index)) void renderPage(index);
          }
      },
      { root: viewport, rootMargin: "100% 0px" },
    );

    resizeObserver = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect;
      if (!rect) return;
      const width = Math.round(rect.width);
      const height = Math.round(rect.height);
      const widthChanged = width !== lastViewportSize.width;
      const heightChanged = height !== lastViewportSize.height;
      lastViewportSize = { width, height };
      if (widthChanged || (fitMode === "height" && heightChanged)) {
        void relayout(fitMode, zoom);
      } else if (heightChanged) {
        renderVisiblePages();
      }
    });
    resizeObserver.observe(viewport);

    let unlistenCloseRequested: (() => void) | undefined;
    if (isNativeApp) {
      void getCurrentWindow()
        .onCloseRequested((event) => {
          event.preventDefault();
          onclose();
        })
        .then((unlisten) => {
          if (disposed) unlisten();
          else unlistenCloseRequested = unlisten;
        });
    }

    void (async () => {
      try {
        pdfjsApi = await import("../../../../vendor/foliate-js/vendor/pdfjs/pdf.mjs");
        pdfDocument = await openSource(pdfjsApi);
        // Covers range-read failures for the rest of the document's lifetime,
        // not just the initial open (which `openSource` already races above).
        if (pdfRangeFatal)
          void pdfRangeFatal.catch((reason) => {
            if (disposed) return;
            error = reason instanceof Error ? reason.message : String(reason);
            loading = false;
          });
        if (disposed) return;
        const metadata = await pdfDocument.getMetadata().catch(() => null);
        documentTitle =
          metadata?.metadata?.get?.("dc:title") ||
          metadata?.info?.Title ||
          book.title ||
          sourceName();
        outlineItems = flattenOutline(await pdfDocument.getOutline().catch(() => null));
        expandedOutlineKeys = initialExpandedKeys(outlineItems);

        const first = await pdfDocument.getPage(1);
        pageObjects.set(0, first);
        const firstViewport = first.getViewport({ scale: 1 });
        pages = Array.from({ length: pdfDocument.numPages }, () => ({
          width: firstViewport.width,
          height: firstViewport.height,
          displayWidth: firstViewport.width,
          displayHeight: firstViewport.height,
          rendering: false,
          renderedScale: 0,
        }));
        updatePageLayout();
        loading = false;
        await tick();
        const initialPage = Math.min(
          pages.length - 1,
          Math.max(0, book.initialPdfPage ?? pageFromLocator(book.initialCfi)),
        );
        const initialFullPagePosition = Number.isFinite(
          book.initialPdfFullPagePosition ?? 0,
        )
          ? Math.max(0, Number(book.initialPdfFullPagePosition ?? 0))
          : 0;
        goToPageAt(initialPage, initialFullPagePosition);
        void renderPage(initialPage);

        // Learn real page dimensions without delaying the first visible page.
        for (let index = 1; index < pages.length && !disposed; index += 1) {
          const page = await pdfPage(index);
          const pageViewport = page.getViewport({ scale: 1 });
          const state = pages[index];
          const scale = layoutScale({
            ...state,
            width: pageViewport.width,
            height: pageViewport.height,
          });
          pages[index] = {
            ...state,
            width: pageViewport.width,
            height: pageViewport.height,
            displayWidth: pageViewport.width * scale,
            displayHeight: pageViewport.height * scale,
          };
          pages = pages;
          if (index % 8 === 0)
            await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
        }
      } catch (reason) {
        if (!disposed) error = reason instanceof Error ? reason.message : String(reason);
      } finally {
        if (!disposed) loading = false;
      }
    })();

    return () => {
      disposed = true;
      enableReaderImmersiveMode(false);
      enableVolumePageTurning(false);
      unlistenVolume();
      unlistenCloseRequested?.();
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      window.removeEventListener("focus", handleWindowFocus);
      window.removeEventListener("keydown", handleKey);
      window.removeEventListener("keyup", capturePdfSelection);
      if (pendingChromeToggle) clearTimeout(pendingChromeToggle);
      intersectionObserver?.disconnect();
      resizeObserver?.disconnect();
      for (const task of renderTasks.values()) task.cancel?.();
      renderTasks.clear();
      if (searchTimer) clearTimeout(searchTimer);
      if (scrollFrame !== undefined) cancelAnimationFrame(scrollFrame);
      void flushPosition();
      annotationController.dispose();
      positionAutosave.dispose();
      void pdfDocument?.destroy?.();
      if (isNativePdfRangeSource(book.file)) void closeNativePdfRangeSource(book.file);
    };
  });
</script>

<svelte:head>
  <title>{documentTitle} · Reader</title>
  <link
    rel="stylesheet"
    href="/pdfjs/text_layer_builder.css"
  />
</svelte:head>

<PdfReaderShell
  hiddenChrome={!chromeVisible}
  onPointerDown={handleViewportPointerDown}
  onPointerMove={handleViewportPointerMove}
  onPointerUp={handleViewportPointerUp}
>
  <ReaderHeader
    title={documentTitle}
    {sidebarOpen}
    {searchOpen}
    {settingsOpen}
    progressLabel={progressLabel()}
    hiddenChrome={!chromeVisible}
    {onclose}
    fixed
    onToggleSidebar={() => onTogglePanel("sidebar")}
    onToggleSearch={() => onTogglePanel("search")}
    onToggleSettings={() => onTogglePanel("settings")}
    searchLabel="Search PDF"
    settingsLabel="PDF appearance settings"
  >
    <svelte:fragment slot="settings-content">
      <FileIcon source={slidersHorizontal} />
    </svelte:fragment>
  </ReaderHeader>

  <PdfSelectionActions
    selectionText={selection?.text ?? ""}
    onHighlight={() => void saveSelection(false)}
    onNote={() => void saveSelection(true)}
    onDismiss={clearPdfSelection}
  />

  {#if sidebarOpen}
    <PdfReaderSidebar
      activeTab={sidebarTab}
      onTabChange={(tab) => showSidebar(tab)}
      contentItems={outlinePanelItems}
      onOpenContentItem={(item) => void goToOutline(item.data as OutlineItem)}
      onToggleContentItem={(item) =>
        (expandedOutlineKeys = toggleExpandedKey(
          expandedOutlineKeys,
          (item.data as OutlineItem).key,
        ))}
      {annotations}
      {bookmarks}
      {annotationError}
      bookmarkCfi={`epubcfi(/6/${(currentPage + 1) * 2})`}
      bookmarkLabel={`Page ${currentPage + 1}`}
      onActivateAnnotation={(annotation) => handleOpenAnnotation(annotation)}
      onOpenBookmark={(bookmark) => void openBookmark(bookmark)}
    />
  {/if}

  <PdfSearchPanel
    {searchOpen}
    {searchQuery}
    {searchResults}
    {selectedSearchResult}
    {searching}
    onQueryInput={handleSearchInput}
    onSubmitSearch={() => selectSearchResult(selectedSearchResult + 1)}
    onPrevious={() => selectSearchResult(selectedSearchResult - 1)}
    onNext={() => selectSearchResult(selectedSearchResult + 1)}
    onClose={closeSearch}
  />

  <PdfSettingsPanel
    {settingsOpen}
    {fitMode}
    {zoom}
    {resolveLayout}
    {widthFitZoom}
    {relayout}
    {adjustPageColumns}
    {adjustZoom}
    minZoom={PDF_MIN_ZOOM}
    maxColumns={PDF_MAX_PAGE_COLUMNS}
    onClose={() => (settingsOpen = false)}
  />

  <div
    class="pdf-viewport"
    bind:this={viewport}
    onscroll={handleScroll}
  >
    {#if loading}
      <StatusState
        kind="loading"
        variant="reader"
        title="Opening PDF…"
      />
    {:else if error}
      <StatusState
        kind="error"
        variant="reader"
        title="This PDF could not be opened."
        message={error}
      />
    {:else}
      <div
        class:fit-page={fitMode === "height"}
        class="page-stream"
        style={`--pdf-columns:${resolveLayout().columns}`}
      >
        {#each pages as page, index (index)}
          <PdfPage
            {page}
            {index}
            totalPages={pages.length}
            pageGap={PAGE_GAP}
            shellAction={shell}
            searchResults={searchResultsForPage(index)}
            {selectedSearchResult}
            annotations={annotationsForPage(index)}
            activeAnnotationId={activeAnnotation?.id ?? null}
            onOpenAnnotation={handleOpenAnnotation}
          />
        {/each}
      </div>
    {/if}
  </div>

  {#if activeAnnotation}
    <SharedAnnotationEditor
      annotation={activeAnnotation}
      note={annotationNote}
      style={annotationPopupStyle()}
      onNoteChange={setPdfAnnotationNote}
      onClose={closeAnnotation}
      onDelete={deleteActiveAnnotation}
      onStyleChange={(style) => setActiveAnnotationStyle(style)}
      onColorChange={(color) => setActiveAnnotationColor(color)}
      ariaLabel="PDF annotation"
      actionsClass="annotation-editor-actions"
      rootClass="annotation-editor"
      showDelete
    />
  {/if}
</PdfReaderShell>
