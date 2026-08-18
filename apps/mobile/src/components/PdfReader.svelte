<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { enableReaderImmersiveMode, enableVolumePageTurning, onVolumePageTurn } from "../lib/volume-navigation";
  import { isNativeApp } from "../lib/platform";
  import type { PdfAnnotationRect, ReaderAnnotation, ReaderAnnotationStyle, ReaderBook, ReaderBookmark } from "../lib/types";

  /// Which end of the layout is held fixed. The other two values are
  /// derived from it, exactly as in the desktop reader.
  type FitMode = "width" | "zoom" | "height";
  type PdfPageState = {
    width: number;
    height: number;
    displayWidth: number;
    displayHeight: number;
    rendering: boolean;
    renderedScale: number;
  };
  type OutlineItem = { label: string; destination: unknown; depth: number };
  type SearchResult = {
    pageIndex: number;
    excerpt: string;
    startItem: number;
    endItem: number;
    startOffset: number;
    endOffset: number;
    rects: PdfAnnotationRect[];
  };
  type PdfSelection = { pageIndex: number; text: string; rects: PdfAnnotationRect[]; viewportRect: DOMRect };

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
  let sidebarTab = $state<"contents" | "annotations">("contents");
  let marksTab = $state<"highlights" | "bookmarks">("highlights");
  let settingsOpen = $state(false);
  let searchOpen = $state(false);
  let outlineItems = $state<OutlineItem[]>([]);
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

  let pdfDocument: any;
  let pdfjsApi: any;
  let intersectionObserver: IntersectionObserver | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let searchGeneration = 0;
  let annotationSaveQueue = Promise.resolve();
  let scrollFrame: number | undefined;
  let disposed = false;
  let shellNodes: Array<HTMLElement | undefined> = [];
  const renderTasks = new Map<number, any>();
  const pageObjects = new Map<number, any>();
  const atomicOffsets = new Map<number, number[]>();
  const textDivsByPage = new Map<number, HTMLElement[]>();
  const PAGE_GAP = 12;
  const COLUMN_GAP = 1;
  const SURFACE_INSET = 12;
  const MAX_PAGE_COLUMNS = 6;
  const MIN_ZOOM = 0.05;
  const PDF_VIEWER_PREFERENCES_KEY = "bokheim:pdf-viewer-preferences:v1";

  function loadViewerPreferences() {
    try {
      const stored = JSON.parse(localStorage.getItem(PDF_VIEWER_PREFERENCES_KEY) ?? "null") as Partial<{ fitMode: FitMode; zoom: number; pageColumns: number }> | null;
      if (stored?.fitMode === "width" || stored?.fitMode === "zoom" || stored?.fitMode === "height") fitMode = stored.fitMode;
      if (typeof stored?.zoom === "number" && Number.isFinite(stored.zoom)) zoom = Math.max(MIN_ZOOM, stored.zoom);
      if (typeof stored?.pageColumns === "number" && Number.isFinite(stored.pageColumns)) pageColumns = Math.max(1, Math.min(MAX_PAGE_COLUMNS, Math.round(stored.pageColumns)));
    } catch {
      // Ignore malformed preferences and keep the reader defaults.
    }
  }

  function persistViewerPreferences() {
    try {
      localStorage.setItem(PDF_VIEWER_PREFERENCES_KEY, JSON.stringify({ fitMode, zoom, pageColumns }));
    } catch {
      // Reading must keep working when storage is unavailable.
    }
  }

  function pageLocator(index: number) {
    return `pdf-page:${Math.max(0, index)}`;
  }

  function visibleLastPage() {
    return Math.min(pages.length, currentPage + visibleColumns());
  }

  function pageIndicator() {
    if (!pages.length) return "0/0";
    const last = visibleLastPage();
    return last === currentPage + 1 ? `${last}/${pages.length}` : `${currentPage + 1}–${last}/${pages.length}`;
  }

  function progressLabel() {
    return pages.length ? `${Math.round((visibleLastPage() / pages.length) * 100)}%` : "0%";
  }

  function pdfAnchor(annotation: ReaderAnnotation) {
    return annotation.anchor.kind === "pdf" ? annotation.anchor : null;
  }

  function annotationsForPage(pageIndex: number) {
    return annotations.filter((annotation) => pdfAnchor(annotation)?.page_index === pageIndex);
  }

  function searchResultsForPage(pageIndex: number) {
    return searchResults.map((result, resultIndex) => ({ result, resultIndex })).filter(({ result }) => result.pageIndex === pageIndex);
  }

  function bookmarkPage(cfi: string) {
    const direct = /^pdf-page:(\d+)$/.exec(cfi);
    if (direct) return Number(direct[1]);
    const legacy = /\/6\/(\d+)/.exec(cfi);
    return legacy ? Math.max(0, Math.floor(Number(legacy[1]) / 2) - 1) : 0;
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
    const start = range.startContainer instanceof Element ? range.startContainer : range.startContainer.parentElement;
    const layer = start?.closest<HTMLElement>(".pdf-text-layer");
    const pageShell = layer?.closest<HTMLElement>(".pdf-page");
    const pageIndex = Number(pageShell?.dataset.pageIndex);
    const text = selected.toString().trim();
    if (!pageShell || !Number.isInteger(pageIndex) || !text) return;
    const bounds = pageShell.getBoundingClientRect();
    const clientRects = Array.from(range.getClientRects()).filter((rect) => rect.width > 0 && rect.height > 0 && rect.bottom > bounds.top && rect.top < bounds.bottom && rect.right > bounds.left && rect.left < bounds.right);
    const rects = clientRects.map((rect) => {
      const left = Math.max(bounds.left, rect.left);
      const top = Math.max(bounds.top, rect.top);
      const right = Math.min(bounds.right, rect.right);
      const bottom = Math.min(bounds.bottom, rect.bottom);
      return { left: (left - bounds.left) / bounds.width, top: (top - bounds.top) / bounds.height, width: (right - left) / bounds.width, height: (bottom - top) / bounds.height };
    }).filter((rect) => rect.width > 0 && rect.height > 0);
    if (!rects.length) return;
    const left = Math.min(...clientRects.map((rect) => rect.left));
    const top = Math.min(...clientRects.map((rect) => rect.top));
    const right = Math.max(...clientRects.map((rect) => rect.right));
    const bottom = Math.max(...clientRects.map((rect) => rect.bottom));
    selection = { pageIndex, text, rects, viewportRect: new DOMRect(left, top, right - left, bottom - top) };
  }

  async function saveSelection(openEditor: boolean) {
    if (!selection || !book.bookId || !book.upsertAnnotation) return;
    annotationError = "";
    const now = Date.now();
    const selected = selection;
    const annotation: ReaderAnnotation = {
      id: crypto.randomUUID(),
      content_hash: book.bookId,
      anchor: { kind: "pdf", page_index: selected.pageIndex, rects: selected.rects, fallback_cfi: pageLocator(selected.pageIndex) },
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
    return new DOMRect(page.left + left * page.width, page.top + top * page.height, (right - left) * page.width, (bottom - top) * page.height);
  }

  function annotationPopupStyle() {
    if (!activeAnnotation) return "";
    const rect = annotationViewportRect(activeAnnotation);
    if (!rect) return "left:50%;top:50%;transform:translate(-50%,-50%);";
    const width = Math.min(360, window.innerWidth - 24);
    const left = Math.max(12, Math.min(rect.left + rect.width / 2 - width / 2, window.innerWidth - width - 12));
    const top = rect.bottom + 10 + 300 < window.innerHeight ? rect.bottom + 10 : Math.max(12, rect.top - 310);
    return `left:${left}px;top:${top}px;width:${width}px;`;
  }

  async function openAnnotation(annotation: ReaderAnnotation) {
    if (sidebarOpen && window.matchMedia("(max-width: 720px)").matches) {
      sidebarOpen = false;
      await tick();
    }
    const anchor = pdfAnchor(annotation);
    if (anchor) goToPage(anchor.page_index);
    activeAnnotation = annotation;
    annotationNote = annotation.note;
  }

  async function updateActiveAnnotation(style?: ReaderAnnotationStyle, color?: string) {
    if (!activeAnnotation || !book.upsertAnnotation) return;
    const updated = { ...activeAnnotation, style: style ?? activeAnnotation.style, color: color ?? activeAnnotation.color, note: annotationNote, modified_at: Date.now() };
    activeAnnotation = updated;
    annotations = annotations.map((annotation) => annotation.id === updated.id ? updated : annotation);
    try {
      annotationSaveQueue = annotationSaveQueue.then(() => book.upsertAnnotation?.(updated));
      await annotationSaveQueue;
    } catch (reason) {
      annotationError = reason instanceof Error ? reason.message : String(reason);
      annotationSaveQueue = Promise.resolve();
    }
  }

  function closeAnnotation() {
    if (activeAnnotation && annotationNote !== activeAnnotation.note) void updateActiveAnnotation();
    activeAnnotation = null;
  }

  async function deleteAnnotation(annotation: ReaderAnnotation) {
    if (!book.deleteAnnotation) return;
    try {
      await book.deleteAnnotation(annotation.id, Date.now());
      annotations = annotations.filter((item) => item.id !== annotation.id);
      if (activeAnnotation?.id === annotation.id) activeAnnotation = null;
    } catch (reason) {
      annotationError = reason instanceof Error ? reason.message : String(reason);
    }
  }

  function deleteActiveAnnotation() {
    if (activeAnnotation) void deleteAnnotation(activeAnnotation);
  }

  async function addBookmark() {
    if (!book.bookId || !book.createBookmark) return;
    const now = Date.now();
    const bookmark: ReaderBookmark = {
      id: crypto.randomUUID(), content_hash: book.bookId,
      cfi: `epubcfi(/6/${(currentPage + 1) * 2})`, label: `Page ${currentPage + 1}`,
      created_at: now, modified_at: now,
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

  function pageFromLocator(locator?: string | null) {
    if (!locator) return 0;
    const direct = /^pdf-page:(\d+)$/.exec(locator);
    if (direct) return Number(direct[1]);
    // Preserve positions written by the previous Foliate fixed-layout reader.
    const legacy = /^epubcfi\(\/6\/(\d+)/.exec(locator);
    return legacy ? Math.max(0, Math.floor(Number(legacy[1]) / 2) - 1) : 0;
  }

  function sourceName() {
    if (typeof File !== "undefined" && book.file instanceof File) return book.file.name;
    return book.fileName ?? book.title;
  }

  async function openSource(pdfjs: any) {
    pdfjs.GlobalWorkerOptions.workerSrc = new URL("/pdfjs/pdf.worker.mjs", globalThis.location.href).toString();
    const options = {
      cMapUrl: new URL("/pdfjs/cmaps/", globalThis.location.href).toString(),
      standardFontDataUrl: new URL("/pdfjs/standard_fonts/", globalThis.location.href).toString(),
      isEvalSupported: false,
    };
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
        return pdfjs.getDocument({ ...options, data: await response.arrayBuffer() }).promise;
      }
    }
    if (!(book.file instanceof File)) throw new Error("PDF source is not a local file");
    return pdfjs.getDocument({ ...options, data: await book.file.arrayBuffer() }).promise;
  }

  function surfaceSize() {
    return {
      width: Math.max(1, (viewport?.clientWidth ?? window.innerWidth) - SURFACE_INSET * 2),
      height: Math.max(1, (viewport?.clientHeight ?? window.innerHeight) - SURFACE_INSET * 2),
    };
  }

  /// The page the layout is measured against. Page sizes vary within a
  /// document, so this follows the page being read.
  function referencePageSize() {
    const page = pages[currentPage] ?? pages[0];
    return { width: page?.width || 612, height: page?.height || 792 };
  }

  /// The zoom at which `columns` pages fill the surface width.
  function widthFitZoom(columns: number) {
    const usable = surfaceSize().width - (columns - 1) * COLUMN_GAP;
    return Math.max(MIN_ZOOM, usable / columns / referencePageSize().width);
  }

  function columnsThatFit(columnWidth: number) {
    const columns = Math.floor((surfaceSize().width + COLUMN_GAP) / (Math.max(1, columnWidth) + COLUMN_GAP));
    return Math.max(1, Math.min(MAX_PAGE_COLUMNS, columns));
  }

  /// Settles the mode into the zoom, column width and column count the layout
  /// runs on. One page filling the width is the ceiling in every mode.
  function resolveLayout() {
    const page = referencePageSize();
    const widestZoom = widthFitZoom(1);
    let columns = pageColumns;
    let resolved = zoom;
    if (fitMode === "width") {
      columns = Math.max(1, Math.min(MAX_PAGE_COLUMNS, pageColumns));
      resolved = widthFitZoom(columns);
    } else {
      resolved = fitMode === "height" ? surfaceSize().height / page.height : zoom;
      resolved = Math.max(MIN_ZOOM, Math.min(widestZoom, resolved));
      columns = columnsThatFit(page.width * resolved);
    }
    return { columns, columnWidth: Math.max(1, page.width * resolved), zoom: resolved };
  }

  /// How many pages are actually side by side right now: chosen directly in
  /// fit-width, derived from the zoom otherwise.
  function visibleColumns() {
    return resolveLayout().columns;
  }

  /// Every page is drawn at the resolved column width, so a document with
  /// mixed page sizes still lines up in the grid.
  function layoutScale(page: PdfPageState) {
    return Math.max(MIN_ZOOM, resolveLayout().columnWidth / Math.max(1, page.width));
  }

  function updatePageLayout() {
    pages = pages.map((page, index) => {
      const scale = layoutScale(page);
      const displayWidth = page.width * scale;
      const displayHeight = page.height * scale;
      const textLayer = shellNodes[index]?.querySelector<HTMLElement>(".pdf-text-layer");
      if (textLayer) {
        textLayer.style.setProperty("--scale-factor", String(scale));
        textLayer.style.setProperty("--user-unit", String(pageObjects.get(index)?.userUnit || 1));
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

  function collectAtomicOffsets(index: number, textLayer: HTMLElement, pageShell: HTMLElement) {
    const shellRect = pageShell.getBoundingClientRect();
    const offsets = Array.from(textLayer.querySelectorAll<HTMLElement>("span"))
      .map((span) => span.getBoundingClientRect())
      .filter((rect) => rect.width > 0 && rect.height > 0)
      .map((rect) => Math.max(0, rect.top - shellRect.top))
      .sort((left, right) => left - right)
      .filter((offset, position, values) => position === 0 || Math.abs(offset - values[position - 1]) > 2);
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
        range.setStart(startNode, Math.min(result.startOffset, startNode.textContent?.length ?? 0));
        range.setEnd(endNode, Math.min(result.endOffset, endNode.textContent?.length ?? 0));
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
    if (Math.abs(state.renderedScale - scale) < 0.001 && pageShell.querySelector("canvas")) return;

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
      textDivsByPage.set(index, Array.from((layer.textDivs as HTMLElement[] | null | undefined) ?? textLayer.querySelectorAll<HTMLElement>("span")));
      collectAtomicOffsets(index, textLayer, pageShell);
      refreshSearchRects(index);
      pages[index] = { ...pages[index], rendering: false, renderedScale: scale };
      pages = pages;
    } catch (reason) {
      if ((reason as { name?: string })?.name !== "RenderingCancelledException" && !disposed) {
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
      if (bounds.bottom >= viewportBounds.top - viewportBounds.height && bounds.top <= viewportBounds.bottom + viewportBounds.height) void renderPage(index);
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

  function nearestPage() {
    const anchor = viewport.scrollTop + viewport.clientHeight * 0.35;
    let best = 0;
    let distance = Number.POSITIVE_INFINITY;
    for (const [index, node] of shellNodes.entries()) {
      if (!node) continue;
      const candidate = Math.abs(node.offsetTop - anchor);
      if (candidate < distance) {
        distance = candidate;
        best = index;
      }
    }
    const columns = visibleColumns();
    return Math.floor(best / columns) * columns;
  }

  function persistPosition() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      saveTimer = undefined;
      const progress = pages.length ? ((currentPage + 1) / pages.length) * 100 : 0;
      if (book.savePdfPage) void book.savePdfPage(currentPage, progress);
      else void book.saveCfi(pageLocator(currentPage), progress);
    }, 500);
  }

  function handleScroll() {
    if (scrollFrame !== undefined) return;
    scrollFrame = requestAnimationFrame(() => {
      scrollFrame = undefined;
      const next = nearestPage();
      if (next !== currentPage) {
        currentPage = next;
        persistPosition();
        releaseDistantPages();
      }
      renderVisiblePages();
    });
  }

  function goToPage(index: number) {
    const target = Math.max(0, Math.min(pages.length - 1, index));
    const node = shellNodes[target];
    if (!node) return;
    const columns = visibleColumns();
    currentPage = Math.floor(target / columns) * columns;
    viewport.scrollTo({ top: node.offsetTop, behavior: "auto" });
    void renderPage(target);
    persistPosition();
  }

  function flattenOutline(items: any[] | null | undefined, depth = 0, result: OutlineItem[] = []) {
    if (!items) return result;
    for (const item of items) {
      if (item?.title) result.push({ label: String(item.title), destination: item.dest, depth });
      if (item?.items?.length) flattenOutline(item.items, depth + 1, result);
    }
    return result;
  }

  async function goToOutline(item: OutlineItem) {
    try {
      const destination = typeof item.destination === "string" ? await pdfDocument.getDestination(item.destination) : item.destination;
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
      for (let index = 0; index < pages.length && generation === searchGeneration; index += 1) {
        const content = await (await pdfPage(index)).getTextContent();
        const textItems = content.items.filter((item: { str?: unknown }) => typeof item.str === "string") as Array<{ str: string }>;
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
          const startSegment = segments.find((segment) => segment.end > match) ?? segments.at(-1);
          const endSegment = [...segments].reverse().find((segment) => segment.start < matchEnd) ?? startSegment;
          if (startSegment && endSegment) {
            results.push({
              pageIndex: index,
              excerpt: text.slice(Math.max(0, match - 45), Math.min(text.length, matchEnd + 75)).replace(/\s+/g, " ").trim(),
              startItem: startSegment.item,
              endItem: endSegment.item,
              startOffset: Math.max(0, match - startSegment.start),
              endOffset: Math.max(0, Math.min(endSegment.end, matchEnd) - endSegment.start),
              rects: [],
            });
          }
          match = searchable.indexOf(query, match + Math.max(1, query.length));
        }
        if (index % 8 === 0) await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
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

  function handleSearchInput() {
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
      if (node && rect) viewport.scrollTo({ top: Math.max(0, node.offsetTop + rect.top * node.offsetHeight - 72), behavior: "auto" });
    });
  }

  function togglePanel(panel: "sidebar" | "search" | "settings") {
    const next = panel === "sidebar" ? !sidebarOpen : panel === "search" ? !searchOpen : !settingsOpen;
    sidebarOpen = panel === "sidebar" && next;
    searchOpen = panel === "search" && next;
    settingsOpen = panel === "settings" && next;
  }

  function showSidebar(tab: "contents" | "annotations") {
    sidebarTab = tab;
    sidebarOpen = true;
    searchOpen = false;
    settingsOpen = false;
  }

  function lineOffsetsInDocument() {
    const offsets = [0];
    for (const [index, values] of atomicOffsets) {
      const pageTop = shellNodes[index]?.offsetTop;
      if (pageTop === undefined) continue;
      offsets.push(...values.map((offset) => pageTop + offset));
    }
    for (const node of shellNodes) if (node) offsets.push(node.offsetTop);
    offsets.sort((left, right) => left - right);
    return offsets.filter((offset, index) => index === 0 || Math.abs(offset - offsets[index - 1]) > 2);
  }

  function moveAtomic(direction: -1 | 1) {
    const current = viewport.scrollTop;
    const offsets = lineOffsetsInDocument();
    const target = direction > 0
      ? offsets.find((offset) => offset > current + 2)
      : [...offsets].reverse().find((offset) => offset < current - 2);
    if (target !== undefined) viewport.scrollTo({ top: target, behavior: "auto" });
  }

  function moveViewport(direction: -1 | 1) {
    const current = viewport.scrollTop;
    const intended = current + direction * viewport.clientHeight;
    const offsets = lineOffsetsInDocument();
    const target = direction > 0
      ? (offsets.filter((offset) => offset > current + 2 && offset <= intended).at(-1) ?? offsets.find((offset) => offset > current + 2))
      : (offsets.find((offset) => offset >= intended && offset < current - 2) ?? [...offsets].reverse().find((offset) => offset < current - 2));
    if (target !== undefined) viewport.scrollTo({ top: target, behavior: "auto" });
  }

  function moveVolumePage(direction: -1 | 1) {
    const rowStarts = shellNodes
      .map((node, index) => ({ node, index }))
      .filter(({ node, index }) => node && index % visibleColumns() === 0) as Array<{ node: HTMLElement; index: number }>;
    if (!rowStarts.length) return;

    const scrollTop = viewport.scrollTop;
    const current = [...rowStarts].reverse().find(({ node }) => node.offsetTop <= scrollTop + 2) ?? rowStarts[0];
    if (direction > 0) {
      const next = rowStarts.find(({ node }) => node.offsetTop > current.node.offsetTop + 2);
      if (next) goToPage(next.index);
      return;
    }

    // When part-way through a page, volume-up aligns its beginning first.
    // A subsequent press from that boundary moves to the previous page.
    if (scrollTop > current.node.offsetTop + 2) {
      goToPage(current.index);
      return;
    }
    const previous = [...rowStarts].reverse().find(({ node }) => node.offsetTop < current.node.offsetTop - 2);
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
    if ((event.target as Element | null)?.closest?.("input,textarea,select,[contenteditable='true']")) return;
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
    zoom = Math.max(MIN_ZOOM, Math.min(widthFitZoom(1), Math.round(nextZoom * 10) / 10));
    persistViewerPreferences();
    for (const task of renderTasks.values()) task.cancel?.();
    pages = pages.map((page) => ({ ...page, renderedScale: 0, rendering: false }));
    updatePageLayout();
    await tick();
    goToPage(anchor);
    renderVisiblePages();
  }

  function adjustPageColumns(delta: number) {
    pageColumns = Math.max(1, Math.min(MAX_PAGE_COLUMNS, resolveLayout().columns + delta));
    persistViewerPreferences();
    void relayout("width", zoom);
  }

  /// Nudging the zoom also switches to the mode where the zoom is what is held
  /// fixed and the page count follows.
  function adjustZoom(delta: number) {
    void relayout("zoom", resolveLayout().zoom + delta);
  }

  onMount(() => {
    disposed = false;
    loadViewerPreferences();
    documentTitle = book.title;
    enableReaderImmersiveMode(true);
    enableVolumePageTurning(true);
    const unlistenVolume = onVolumePageTurn((direction) => moveVolumePage(direction === "next" ? 1 : -1));
    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        enableVolumePageTurning(true);
        enableReaderImmersiveMode(true);
      }
    };
    const handleWindowFocus = () => {
      enableVolumePageTurning(true);
      enableReaderImmersiveMode(true);
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("focus", handleWindowFocus);
    window.addEventListener("keydown", handleKey);
    const handleSelectionPointerUp = () => setTimeout(capturePdfSelection);
    viewport.addEventListener("pointerup", handleSelectionPointerUp);
    window.addEventListener("keyup", capturePdfSelection);

    if (book.loadAnnotations) {
      void book.loadAnnotations().then((loaded) => {
        if (!disposed) annotations = loaded.filter((annotation) => annotation.anchor.kind === "pdf");
      }).catch((reason) => {
        if (!disposed) annotationError = reason instanceof Error ? reason.message : String(reason);
      });
    }
    if (book.loadBookmarks) {
      void book.loadBookmarks().then((loaded) => {
        if (!disposed) bookmarks = loaded;
      }).catch((reason) => {
        if (!disposed) annotationError = reason instanceof Error ? reason.message : String(reason);
      });
    }

    intersectionObserver = new IntersectionObserver((entries) => {
      for (const entry of entries) if (entry.isIntersecting) {
        const index = Number((entry.target as HTMLElement).dataset.pageIndex);
        if (Number.isInteger(index)) void renderPage(index);
      }
    }, { root: viewport, rootMargin: "100% 0px" });

    resizeObserver = new ResizeObserver(() => void relayout(fitMode, zoom));
    resizeObserver.observe(viewport);

    let unlistenCloseRequested: (() => void) | undefined;
    if (isNativeApp) {
      void getCurrentWindow().onCloseRequested((event) => {
        event.preventDefault();
        onclose();
      }).then((unlisten) => {
        if (disposed) unlisten();
        else unlistenCloseRequested = unlisten;
      });
    }

    void (async () => {
      try {
        pdfjsApi = await import("../vendor/foliate-js/vendor/pdfjs/pdf.mjs");
        pdfDocument = await openSource(pdfjsApi);
        if (disposed) return;
        const metadata = await pdfDocument.getMetadata().catch(() => null);
        documentTitle = metadata?.metadata?.get?.("dc:title") || metadata?.info?.Title || book.title || sourceName();
        outlineItems = flattenOutline(await pdfDocument.getOutline().catch(() => null));

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
        const initialPage = Math.min(pages.length - 1, Math.max(0, book.initialPdfPage ?? pageFromLocator(book.initialCfi)));
        goToPage(initialPage);
        void renderPage(initialPage);

        // Learn real page dimensions without delaying the first visible page.
        for (let index = 1; index < pages.length && !disposed; index += 1) {
          const page = await pdfPage(index);
          const pageViewport = page.getViewport({ scale: 1 });
          const state = pages[index];
          const scale = layoutScale({ ...state, width: pageViewport.width, height: pageViewport.height });
          pages[index] = { ...state, width: pageViewport.width, height: pageViewport.height, displayWidth: pageViewport.width * scale, displayHeight: pageViewport.height * scale };
          pages = pages;
          if (index % 8 === 0) await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
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
      viewport.removeEventListener("pointerup", handleSelectionPointerUp);
      window.removeEventListener("keyup", capturePdfSelection);
      intersectionObserver?.disconnect();
      resizeObserver?.disconnect();
      for (const task of renderTasks.values()) task.cancel?.();
      renderTasks.clear();
      if (saveTimer) clearTimeout(saveTimer);
      if (searchTimer) clearTimeout(searchTimer);
      if (scrollFrame !== undefined) cancelAnimationFrame(scrollFrame);
      if (pages.length) {
        const progress = ((currentPage + 1) / pages.length) * 100;
        if (book.savePdfPage) void book.savePdfPage(currentPage, progress);
        else void book.saveCfi(pageLocator(currentPage), progress);
      }
      void pdfDocument?.destroy?.();
    };
  });
</script>

<svelte:head>
  <title>{documentTitle} · Reader</title>
  <link rel="stylesheet" href="/pdfjs/text_layer_builder.css" />
</svelte:head>

<main class="pdf-reader-shell">
  <header class="pdf-toolbar" aria-label="PDF controls">
    <div class="toolbar-side toolbar-left">
      <button class="icon" type="button" aria-label="Return to library" onclick={onclose}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 18-6-6 6-6"></path></svg>
      </button>
      <button class:active={sidebarOpen} class="icon" type="button" aria-label="Contents and annotations" onclick={() => togglePanel("sidebar") }>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 5h16M4 12h16M4 19h16"></path></svg>
      </button>
    </div>
    <div class="identity">
      <strong>{documentTitle}</strong>
    </div>
    <div class="toolbar-side toolbar-right">
      <button class:active={searchOpen} class="icon" type="button" aria-label="Search PDF" onclick={() => togglePanel("search")}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="6.5"></circle><path d="m16 16 4 4"></path></svg>
      </button>
      <output class="reader-progress" aria-live="polite" title={pageIndicator()}>{progressLabel()}</output>
      <button class:active={settingsOpen} class="icon" type="button" aria-label="PDF appearance settings" onclick={() => togglePanel("settings")}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h10M18 7h2M4 17h2M10 17h10"></path><circle cx="16" cy="7" r="2"></circle><circle cx="8" cy="17" r="2"></circle></svg>
      </button>
    </div>
  </header>

  {#if selection}
    <div class="selection-actions" role="toolbar" aria-label="Selected PDF text">
      <q>{selection.text}</q>
      <button type="button" onclick={() => void saveSelection(false)}>Highlight</button>
      <button type="button" onclick={() => void saveSelection(true)}>Note</button>
      <button type="button" aria-label="Dismiss selected text actions" onclick={clearPdfSelection}>×</button>
    </div>
  {/if}

  {#if sidebarOpen}
    <aside class="pdf-panel contents-panel" aria-label={sidebarTab === "contents" ? "PDF contents" : "PDF annotations and bookmarks"}>
      <div class="sidebar-switcher" role="tablist" aria-label="PDF sidebar">
        <button class:active={sidebarTab === "contents"} role="tab" aria-selected={sidebarTab === "contents"} type="button" onclick={() => showSidebar("contents")}>Contents</button>
        <button class:active={sidebarTab === "annotations"} role="tab" aria-selected={sidebarTab === "annotations"} type="button" onclick={() => showSidebar("annotations")}>Annotations</button>
        <button type="button" aria-label="Close PDF sidebar" onclick={() => (sidebarOpen = false)}>×</button>
      </div>
      {#if sidebarTab === "contents"}
        {#if outlineItems.length}
          <nav>{#each outlineItems as item}<button type="button" style={`--outline-depth:${item.depth}`} onclick={() => void goToOutline(item)}>{item.label}</button>{/each}</nav>
        {:else}
          <p>This PDF has no outline.</p>
        {/if}
      {:else}
        <div class="marks-switcher" role="tablist" aria-label="PDF marks">
          <button class:active={marksTab === "highlights"} type="button" onclick={() => (marksTab = "highlights")}>Highlights ({annotations.length})</button>
          <button class:active={marksTab === "bookmarks"} type="button" onclick={() => (marksTab = "bookmarks")}>Bookmarks ({bookmarks.length})</button>
        </div>
        {#if annotationError}<p class="panel-error" role="alert">{annotationError}</p>{/if}
        {#if marksTab === "highlights"}
          <div class="marks-list">
            {#each annotations as annotation (annotation.id)}
              <article class:active={activeAnnotation?.id === annotation.id}>
                <button class="mark-target" type="button" onclick={() => openAnnotation(annotation)}>
                  <span class="annotation-color" style={`--annotation-color:${annotation.color}`}></span>
                  <q>{annotation.exact_text}</q>
                  {#if annotation.note}<small>{annotation.note}</small>{/if}
                </button>
                <button type="button" aria-label="Delete annotation" onclick={() => void deleteAnnotation(annotation)}>×</button>
              </article>
            {/each}
            {#if !annotations.length}<p>Select PDF text, then use Highlight or Note.</p>{/if}
          </div>
        {:else}
          <button class="bookmark-here" type="button" onclick={() => void addBookmark()}>Bookmark here</button>
          <div class="marks-list">
            {#each bookmarks as bookmark (bookmark.id)}
              <article>
                <button class="mark-target bookmark-target" type="button" onclick={() => void openBookmark(bookmark)}>{bookmark.label}</button>
                <button type="button" aria-label="Delete bookmark" onclick={() => void deleteBookmark(bookmark)}>×</button>
              </article>
            {/each}
            {#if !bookmarks.length}<p>No bookmarks yet. Use Bookmark here to save this page.</p>{/if}
          </div>
        {/if}
      {/if}
    </aside>
  {/if}

  {#if searchOpen}
    <section class="pdf-search" aria-label="Search PDF">
      <form onsubmit={(event) => { event.preventDefault(); selectSearchResult(selectedSearchResult + 1); }}>
        <input bind:value={searchQuery} oninput={handleSearchInput} type="search" placeholder="Search this PDF" aria-label="Search PDF text" />
        <output>{searchResults.length ? `${selectedSearchResult + 1}/${searchResults.length}` : "0/0"}</output>
        <button type="button" aria-label="Previous search result" disabled={!searchResults.length} onclick={() => selectSearchResult(selectedSearchResult - 1)}>‹</button>
        <button type="button" aria-label="Next search result" disabled={!searchResults.length} onclick={() => selectSearchResult(selectedSearchResult + 1)}>›</button>
        <button type="button" aria-label="Close PDF search" onclick={closeSearch}>×</button>
      </form>
      {#if searching}<p>Searching…</p>{:else if selectedSearchResult >= 0}<p>{searchResults[selectedSearchResult]?.excerpt}</p>{/if}
    </section>
  {/if}

  {#if settingsOpen}
    <section class="pdf-panel settings-panel" aria-label="PDF appearance settings">
      <div class="panel-heading"><strong>Page layout</strong><button type="button" aria-label="Close PDF settings" onclick={() => (settingsOpen = false)}>×</button></div>
      <div class="setting-row">
        <span>Fit</span>
        <div class="choice"><button class:active={fitMode === "width"} type="button" onclick={() => void relayout("width", zoom)}>Fit width</button><button class:active={fitMode === "zoom"} type="button" onclick={() => void relayout("zoom", resolveLayout().zoom)}>Zoom</button><button class:active={fitMode === "height"} type="button" onclick={() => void relayout("height", zoom)}>Fit height</button></div>
      </div>
      {#if fitMode === "width"}
        <div class="setting-row">
          <span>Pages</span>
          <div class="stepper"><button type="button" disabled={resolveLayout().columns <= 1} onclick={() => adjustPageColumns(-1)}>−</button><output>{resolveLayout().columns}</output><button type="button" disabled={resolveLayout().columns >= MAX_PAGE_COLUMNS} onclick={() => adjustPageColumns(1)}>+</button></div>
        </div>
      {:else if fitMode === "zoom"}
        <div class="setting-row">
          <span>Zoom</span>
          <div class="stepper"><button type="button" disabled={resolveLayout().zoom <= MIN_ZOOM} onclick={() => adjustZoom(-0.1)}>−</button><output>{Math.round(resolveLayout().zoom * 100)}%</output><button type="button" disabled={resolveLayout().zoom >= widthFitZoom(1)} onclick={() => adjustZoom(0.1)}>+</button></div>
        </div>
      {/if}
    </section>
  {/if}

  <div class="pdf-viewport" bind:this={viewport} onscroll={handleScroll}>
    {#if loading}
      <div class="reader-message"><span class="loader"></span><strong>Opening PDF…</strong></div>
    {:else if error}
      <div class="reader-message error"><strong>This PDF could not be opened.</strong><span>{error}</span></div>
    {:else}
      <div class:fit-page={fitMode === "height"} class="page-stream" style={`--pdf-columns:${resolveLayout().columns}`}>
        {#each pages as page, index (index)}
          <section
            use:shell={index}
            class="pdf-page"
            data-page-index={index}
            aria-label={`PDF page ${index + 1}`}
            style={`width:${page.displayWidth}px;height:${page.displayHeight}px;--page-gap:${index + 1 < pages.length ? PAGE_GAP : 0}px`}
          >
            <canvas></canvas>
            <div class="pdf-search-highlight-layer" aria-hidden="true">
              {#each searchResultsForPage(index) as { result, resultIndex } (`${resultIndex}-${result.startItem}-${result.startOffset}`)}
                {#each result.rects as rect, rectIndex (`${resultIndex}-${rectIndex}`)}
                  <span
                    class:active={resultIndex === selectedSearchResult}
                    style={`left:${rect.left * 100}%;top:${rect.top * 100}%;width:${rect.width * 100}%;height:${rect.height * 100}%`}
                  ></span>
                {/each}
              {/each}
            </div>
            <div class="pdf-annotation-layer" aria-label={`Annotations on page ${index + 1}`}>
              {#each annotationsForPage(index) as annotation (annotation.id)}
                {@const anchor = pdfAnchor(annotation)}
                {#each anchor?.rects ?? [] as rect, rectIndex (`${annotation.id}-${rectIndex}`)}
                  <button
                    class:active={activeAnnotation?.id === annotation.id}
                    class:highlight={annotation.style === "highlight"}
                    class:underline={annotation.style === "underline"}
                    class:squiggly={annotation.style === "squiggly"}
                    class:strikethrough={annotation.style === "strikethrough"}
                    type="button"
                    aria-label={`Open annotation: ${annotation.exact_text}`}
                    style={`left:${rect.left * 100}%;top:${rect.top * 100}%;width:${rect.width * 100}%;height:${rect.height * 100}%;--annotation-color:${annotation.color}`}
                    onclick={(event) => { event.stopPropagation(); openAnnotation(annotation); }}
                  ></button>
                {/each}
              {/each}
            </div>
            <div class="pdf-text-layer textLayer"></div>
            {#if page.rendering}<span class="page-loading" aria-label={`Rendering page ${index + 1}`}></span>{/if}
          </section>
        {/each}
      </div>
    {/if}
  </div>

  {#if activeAnnotation}
    <section class="annotation-editor" aria-label="PDF annotation" style={annotationPopupStyle()}>
      <q>{activeAnnotation.exact_text}</q>
      <textarea bind:value={annotationNote} placeholder="Add a note" aria-label="Annotation note"></textarea>
      <div class="annotation-options" role="group" aria-label="Annotation style">
        <button class:active={activeAnnotation.style === "highlight"} type="button" onclick={() => void updateActiveAnnotation("highlight")}>Highlight</button>
        <button class:active={activeAnnotation.style === "underline"} type="button" onclick={() => void updateActiveAnnotation("underline")}>Underline</button>
        <button class:active={activeAnnotation.style === "squiggly"} type="button" onclick={() => void updateActiveAnnotation("squiggly")}>Squiggly</button>
        <button class:active={activeAnnotation.style === "strikethrough"} type="button" onclick={() => void updateActiveAnnotation("strikethrough")}>Strike</button>
      </div>
      <div class="annotation-colors" role="group" aria-label="Annotation color">
        {#each [["Yellow", "#f6c945"], ["Blue", "#60a5fa"], ["Pink", "#f472b6"], ["Green", "#4ade80"]] as color}
          <button class:active={activeAnnotation.color === color[1]} type="button" style={`--swatch:${color[1]}`} onclick={() => void updateActiveAnnotation(undefined, color[1])}>{color[0]}</button>
        {/each}
      </div>
      <div class="annotation-editor-actions">
        <button type="button" onclick={deleteActiveAnnotation}>Delete</button>
        <button type="button" onclick={closeAnnotation}>Close</button>
        <button type="button" onclick={() => void updateActiveAnnotation()}>Save</button>
      </div>
    </section>
  {/if}
</main>

<style>
  :global(html), :global(body), :global(#app) { height: 100%; overflow: hidden; }
  :global(body) { margin: 0; }

  .pdf-reader-shell {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    width: 100%;
    height: 100dvh;
    color: var(--color-ink-text);
    background: var(--color-page-bg, var(--color-surface-bg));
  }

  .pdf-toolbar {
    position: relative;
    z-index: 10;
    display: grid;
    grid-template-columns: minmax(76px, 1fr) minmax(0, 520px) minmax(126px, 1fr);
    align-items: center;
    gap: 12px;
    min-width: 0;
    height: calc(58px + env(safe-area-inset-top));
    padding: env(safe-area-inset-top) max(12px, env(safe-area-inset-right)) 0 max(12px, env(safe-area-inset-left));
    box-sizing: border-box;
    overflow: hidden;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-bg);
  }

  button {
    min-width: 34px;
    height: 34px;
    padding: 0 9px;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: inherit;
    background: var(--color-raised-bg);
    font: inherit;
    font-size: 0.82rem;
    font-weight: 700;
    white-space: nowrap;
  }

  button.active, button:active { background: var(--color-border); }
  button:disabled { opacity: 0.38; }
  button.icon { display: grid; place-items: center; padding: 0; }
  button.icon svg { width: 18px; height: 18px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }

  .toolbar-side { display: flex; min-width: 0; align-items: center; gap: 8px; }
  .toolbar-right { justify-content: flex-end; }

  .identity {
    min-width: 0;
    text-align: center;
    line-height: 1.1;
  }

  .identity strong { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 0.82rem; }
  .reader-progress { min-width: 44px; text-align: right; color: var(--color-ink-muted); font-size: 0.78rem; font-variant-numeric: tabular-nums; }

  .pdf-panel {
    position: fixed;
    z-index: 20;
    top: calc(58px + env(safe-area-inset-top));
    max-height: min(620px, calc(100dvh - 72px - env(safe-area-inset-top)));
    overflow: auto;
    box-sizing: border-box;
    border: 1px solid var(--color-border);
    background: var(--color-surface-bg);
    box-shadow: 0 12px 32px color-mix(in srgb, black 24%, transparent);
  }

  .contents-panel { left: max(12px, env(safe-area-inset-left)); width: min(360px, calc(100vw - 24px)); }
  .settings-panel { right: max(12px, env(safe-area-inset-right)); width: min(400px, calc(100vw - 24px)); padding-bottom: 8px; }
  .panel-heading { display: flex; align-items: center; justify-content: space-between; min-height: 48px; padding: 0 8px 0 14px; border-bottom: 1px solid var(--color-border); }
  .sidebar-switcher, .marks-switcher { display: grid; grid-template-columns: 1fr 1fr auto; gap: 5px; padding: 6px; border-bottom: 1px solid var(--color-border); }
  .marks-switcher { grid-template-columns: 1fr 1fr; }
  .contents-panel nav { display: grid; padding: 6px; }
  .contents-panel nav button { height: auto; min-height: 40px; padding: 8px 10px 8px calc(10px + var(--outline-depth) * 16px); border: 0; text-align: left; background: transparent; white-space: normal; }
  .contents-panel p { padding: 8px 14px 18px; color: var(--color-ink-muted); }
  .panel-error { margin: 0; color: var(--color-danger-text, #a22); }
  .bookmark-here { margin: 8px; }
  .marks-list { display: grid; gap: 5px; padding: 6px; }
  .marks-list article { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 4px; border: 1px solid var(--color-border); background: var(--color-raised-bg); }
  .marks-list article.active { background: var(--color-active-highlight, var(--color-border)); }
  .mark-target { display: grid; grid-template-columns: 5px minmax(0, 1fr); gap: 3px 8px; height: auto; min-height: 44px; padding: 7px; border: 0; text-align: left; background: transparent; white-space: normal; }
  .mark-target q, .mark-target small { grid-column: 2; overflow: hidden; text-overflow: ellipsis; }
  .mark-target small { color: var(--color-ink-muted); }
  .bookmark-target { display: block; }
  .annotation-color { grid-row: 1 / span 2; background: var(--annotation-color); }

  .setting-row { display: flex; align-items: center; justify-content: space-between; gap: 14px; min-height: 52px; padding: 0 14px; border-bottom: 1px solid var(--color-border); }
  .setting-row > span { font-weight: 650; }
  .stepper, .choice { display: flex; align-items: center; }
  .stepper output { min-width: 58px; text-align: center; font-size: 0.8rem; font-variant-numeric: tabular-nums; }
  .choice button { min-width: 88px; }

  .pdf-search {
    position: fixed;
    z-index: 19;
    top: calc(58px + env(safe-area-inset-top));
    left: 0;
    right: 0;
    display: grid;
    gap: 6px;
    padding: 8px max(12px, env(safe-area-inset-right)) 8px max(12px, env(safe-area-inset-left));
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-bg);
  }

  .pdf-search form { display: flex; align-items: center; gap: 6px; }
  .pdf-search input { min-width: 90px; min-height: 34px; flex: 1; padding: 0 10px; border: 1px solid var(--color-border); color: inherit; background: var(--color-raised-bg); font: inherit; }
  .pdf-search form output { min-width: 42px; text-align: center; font-size: 0.78rem; }
  .pdf-search p { margin: 0; overflow: hidden; color: var(--color-ink-muted); font-size: 0.78rem; text-overflow: ellipsis; white-space: nowrap; }

  .selection-actions {
    position: fixed;
    z-index: 24;
    top: calc(66px + env(safe-area-inset-top));
    left: 50%;
    display: flex;
    width: min(560px, calc(100vw - 24px));
    align-items: center;
    gap: 6px;
    padding: 6px;
    box-sizing: border-box;
    border: 1px solid var(--color-border);
    background: var(--color-surface-bg);
    box-shadow: 0 10px 28px color-mix(in srgb, black 24%, transparent);
    transform: translateX(-50%);
  }
  .selection-actions q { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: .78rem; }

  .pdf-viewport {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
    background: color-mix(in srgb, var(--color-page-bg, var(--color-surface-bg)) 84%, var(--color-ink-text));
  }

  .page-stream {
    display: grid;
    grid-template-columns: repeat(var(--pdf-columns), max-content);
    justify-content: center;
    align-items: start;
    column-gap: 1px;
    min-width: min-content;
    width: 100%;
    padding: 12px;
    box-sizing: border-box;
  }

  .page-stream.fit-page { min-height: 100%; justify-content: flex-start; }

  .pdf-page {
    position: relative;
    flex: none;
    margin: 0 0 var(--page-gap);
    overflow: hidden;
    background: white;
    box-shadow: 0 1px 2px color-mix(in srgb, black 28%, transparent);
  }

  .pdf-page canvas, .pdf-text-layer, .pdf-search-highlight-layer, .pdf-annotation-layer { position: absolute; inset: 0; display: block; }
  .pdf-page canvas { z-index: 0; }
  .pdf-search-highlight-layer { z-index: 1; pointer-events: none; }
  .pdf-search-highlight-layer span { position: absolute; background: rgb(255 210 0 / 38%); outline: 1px solid rgb(180 120 0 / 45%); }
  .pdf-search-highlight-layer span.active { background: rgb(255 145 0 / 58%); outline-color: rgb(150 75 0 / 75%); }
  .pdf-annotation-layer { z-index: 3; pointer-events: none; }
  .pdf-annotation-layer button {
    position: absolute;
    min-width: 0;
    height: auto;
    padding: 0;
    border: 0;
    background: transparent;
    pointer-events: auto;
  }
  .pdf-annotation-layer button.highlight { background: color-mix(in srgb, var(--annotation-color) 38%, transparent); }
  .pdf-annotation-layer button.underline { border-bottom: 2px solid var(--annotation-color); }
  .pdf-annotation-layer button.squiggly { border-bottom: 2px dotted var(--annotation-color); }
  .pdf-annotation-layer button.strikethrough { border-top: 2px solid var(--annotation-color); transform: translateY(52%); }
  .pdf-annotation-layer button.active { outline: 1px solid var(--annotation-color); outline-offset: 1px; }
  .pdf-text-layer {
    --scale-factor: 1;
    --user-unit: 1;
    --total-scale-factor: calc(var(--scale-factor) * var(--user-unit));
    --scale-round-x: 1px;
    --scale-round-y: 1px;
    overflow: hidden;
    line-height: 1;
    opacity: 1;
    z-index: 2;
  }
  .page-loading { position: absolute; inset: 50% auto auto 50%; width: 22px; height: 22px; margin: -11px; border: 2px solid #777; border-radius: 50%; }

  .reader-message { display: grid; place-items: center; align-content: center; gap: 10px; width: 100%; height: 100%; padding: 24px; box-sizing: border-box; text-align: center; }
  .reader-message.error span { max-width: 42rem; color: var(--color-ink-muted); }
  .loader { width: 28px; height: 28px; border: 3px solid var(--color-border); border-top-color: var(--color-accent); border-radius: 50%; }

  .annotation-editor {
    position: fixed;
    z-index: 25;
    max-height: min(520px, calc(100dvh - 24px));
    overflow: auto;
    box-sizing: border-box;
    padding: 10px;
    border: 1px solid var(--color-border);
    color: var(--color-ink-text);
    background: var(--color-surface-bg);
    box-shadow: 0 14px 36px color-mix(in srgb, black 28%, transparent);
  }
  .annotation-editor q { display: block; margin-bottom: 9px; line-height: 1.35; }
  .annotation-editor textarea { width: 100%; min-height: 72px; padding: 8px; box-sizing: border-box; resize: vertical; border: 1px solid var(--color-border); color: inherit; background: var(--color-raised-bg); font: inherit; }
  .annotation-options, .annotation-colors, .annotation-editor-actions { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 8px; }
  .annotation-options button { flex: 1; min-width: 72px; }
  .annotation-colors button { border-left: 8px solid var(--swatch); }
  .annotation-editor-actions { justify-content: flex-end; padding-top: 8px; border-top: 1px solid var(--color-border); }

  @media (max-width: 760px) {
    .pdf-toolbar { gap: 6px; }
    .toolbar-side { gap: 5px; }
  }

  @media (max-width: 520px) {
    .choice button { min-width: 78px; padding-inline: 6px; }
  }

</style>
