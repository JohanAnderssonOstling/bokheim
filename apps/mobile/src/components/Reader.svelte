<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import designTokens from "../../../../ui/design-tokens/design_tokens.json";

  // Foliate is vendored as a pinned submodule. Importing view.js registers the
  // <foliate-view> custom element and keeps the unstable API boundary here.
  import "../vendor/foliate-js/view.js";
  import { Overlayer } from "../vendor/foliate-js/overlayer.js";
  import { DEFAULT_READER_PREFERENCES, READER_NAVIGATION_CONTRACT, READER_PREFERENCES_CONTRACT, READER_SETTINGS_PRESENTATION_CONTRACT } from "../lib/types";
  import { openExternalUrl } from "../lib/external-links";
  import { isNativeApp } from "../lib/platform";
  import { enableVolumePageTurning, onVolumePageTurn } from "../lib/volume-navigation";
  import { APP_NAME_LOWER } from "../lib/application-name";
  import type {
    FoliateRelocateDetail,
    FoliateRenderer,
    FoliateSearchGroup,
    FoliateSearchMatch,
    FoliateTocItem,
    FoliateView,
    ReaderBook,
    ReaderAnnotation,
    ReaderAnnotationStyle,
    ApplicationTheme,
    ReaderBookmark,
    ReaderFlow,
    ReaderFontFamily,
    ReaderNavigationAction,
    ReaderNavigationKey,
    ReaderPreferencePatch,
    ReaderPreferences,
    ReaderTextAlignment,
  } from "../lib/types";

  let {
    book,
    preferences,
    applicationTheme,
    onpreferenceschange,
    onclose,
  }: {
    book: ReaderBook;
    preferences: ReaderPreferences;
    applicationTheme: ApplicationTheme;
    onpreferenceschange: (patch: ReaderPreferencePatch) => void | Promise<void>;
    onclose: () => void;
  } = $props();

  let host: HTMLDivElement;
  let view = $state<FoliateView | null>(null);
  let loading = $state(true);
  let error = $state("");
  let settings = $state<ReaderPreferences>({ ...DEFAULT_READER_PREFERENCES });
  let settingsOpen = $state(false);
  let tocOpen = $state(false);
  let searchOpen = $state(false);
  let notesOpen = $state(false);
  let infoOpen = $state(false);
  let footnoteOpen = $state(false);
  let footnoteHost = $state<HTMLDivElement>();
  let footnoteLabel = $state("Note");
  let footnoteView: FoliateView | null = null;
  let tocItems = $state<TocItem[]>([]);
  let landmarkItems = $state<TocItem[]>([]);
  let pageListItems = $state<TocItem[]>([]);
  let navigationList = $state<"toc" | "landmarks" | "pages">("toc");
  let expandedTocKeys = $state<Set<string>>(new Set());
  let activeTocHref = $state("");
  let readingFraction = $state(0);
  let currentLocation = $state(0);
  let totalLocations = $state(0);
  let currentSection = $state(0);
  let totalSections = $state(0);
  let currentPageLabel = $state("");
  let canHistoryBack = $state(false);
  let canHistoryForward = $state(false);
  let searchQuery = $state("");
  let searchResults = $state<SearchResult[]>([]);
  let searchProgress = $state(0);
  let searching = $state(false);
  let searchError = $state("");
  let searchMatchCase = $state(false);
  let searchMatchDiacritics = $state(false);
  let searchMatchWholeWords = $state(false);
  let searchScope = $state<"book" | "section">("book");
  let selectedSearchIndex = $state(-1);
  let saveState = $state<"idle" | "saving" | "saved" | "error">("idle");
  let annotations = $state<ReaderAnnotation[]>([]);
  let bookmarks = $state<ReaderBookmark[]>([]);
  let annotationFilter = $state("");
  let readerStateError = $state("");
  let ttsPlaying = $state(false);
  let ttsPaused = $state(false);
  let ttsRate = $state(1);
  let hasMediaOverlay = $state(false);
  let mediaOverlayPlaying = $state(false);
  let imageViewerSrc = $state("");
  let tapZonesEnabled = $state(true);
  let chromeVisible = $state(true);
  let readerMargin = $state(48);
  let pageGap = $state(7);
  let maxBlockSize = $state(1440);
  const READER_UI_PREFERENCES_KEY = `${APP_NAME_LOWER}-reader-ui-preferences`;
  let isPdfBook = $derived(book.file.type === "application/pdf" || /\.pdf$/i.test(book.file.name));
  let selection = $state<{ cfi: string; text: string; pdfAnchor?: PdfAnnotationAnchor } | null>(null);
  let selectionNote = $state("");
  let activeAnnotation = $state<ReaderAnnotation | null>(null);
  let annotationNoteDraft = $state("");
  let selectionContextMenu = $state<{ x: number; y: number } | null>(null);

  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  const READER_UI_PREFERENCES_VERSION = 2;
  const preferenceConstraints = READER_PREFERENCES_CONTRACT.constraints;

  function buildSettingOptions([minimum, maximum]: readonly [number, number], step: number, decimals: number) {
    const values: number[] = [];
    for (let value = minimum; value <= maximum + 1e-9; value += step) {
      const scale = 10 ** decimals;
      values.push(Number((Math.round(value * scale) / scale).toFixed(decimals)));
    }
    return values;
  }

  const maxLineWidthOptions = buildSettingOptions(preferenceConstraints.max_line_width_px, 40, 0);
  const readerFontSizeOptions = buildSettingOptions(preferenceConstraints.font_size_px, 1, 0);
  const zoomOptions = buildSettingOptions(preferenceConstraints.zoom, 0.1, 1);
  const maxColumnCountOptions = buildSettingOptions(preferenceConstraints.max_column_count, 1, 0);
  const lineHeightOptions = buildSettingOptions(preferenceConstraints.line_height, 0.1, 1);
  let lastCfi = $state("");
  let lastPersistedCfi = "";
  let positionSaveQueue = Promise.resolve();
  let searchGeneration = 0;
  let footnoteHandler: {
    handle: (book: unknown, event: Event) => void | Promise<void>;
    addEventListener: EventTarget["addEventListener"];
    removeEventListener: EventTarget["removeEventListener"];
  } | null = null;

  interface TocItem {
    key: string;
    label: string;
    href?: string;
    depth: number;
    parentKeys: string[];
    hasChildren: boolean;
  }

  interface SearchResult extends FoliateSearchMatch {
    section: string;
  }

  interface PdfAnnotationRect {
    left: number;
    top: number;
    width: number;
    height: number;
  }

  interface PdfAnnotationAnchor {
    kind: "pdf";
    page_index: number;
    rects: PdfAnnotationRect[];
    fallback_cfi: string | null;
  }

  function isKoboReader() {
    return /kobo/i.test(navigator.userAgent);
  }

  function initialTocState() {
    try {
      const stored = localStorage.getItem(READER_UI_PREFERENCES_KEY);
      const parsed = stored ? JSON.parse(stored) : {};
      if (typeof parsed === "boolean") return isKoboReader() ? false : parsed;
      if (parsed && typeof parsed === "object" && typeof parsed.tocOpen === "boolean") {
        if (typeof parsed.version === "number") return parsed.tocOpen;
        return isKoboReader() ? false : parsed.tocOpen;
      }
    } catch {
      // Ignore malformed or inaccessible persisted state.
    }
    return false;
  }

  function persistTocState(open: boolean) {
    try {
      localStorage.setItem(READER_UI_PREFERENCES_KEY, JSON.stringify({ version: READER_UI_PREFERENCES_VERSION, tocOpen: open }));
    } catch {
      // Ignore persistence failures.
    }
  }

  function setTocOpen(next: boolean) {
    tocOpen = next;
    persistTocState(next);
  }

  function flattenToc(items: FoliateTocItem[] = [], depth = 0, parentKeys: string[] = [], result: TocItem[] = []) {
    for (const [index, item] of items.entries()) {
      const key = item.href || `${parentKeys.join("/") || "root"}/${item.id ?? index}-${item.label || "section"}`;
      if (item.label) {
        result.push({
          key,
          label: item.label,
          href: item.href || undefined,
          depth,
          parentKeys,
          hasChildren: Boolean(item.subitems?.length),
        });
      }
      if (item.subitems?.length) flattenToc(item.subitems, depth + 1, [...parentKeys, key], result);
    }
    return result;
  }

  function visibleTocItems() {
    const items = navigationList === "landmarks" ? landmarkItems : navigationList === "pages" ? pageListItems : tocItems;
    return items.filter((item) => item.parentKeys.every((key) => expandedTocKeys.has(key)));
  }

  function toggleTocItem(item: TocItem) {
    const next = new Set(expandedTocKeys);
    if (next.has(item.key)) next.delete(item.key);
    else next.add(item.key);
    expandedTocKeys = next;
  }

  function expandActiveTocAncestors(href: string) {
    const item = tocItems.find((candidate) => candidate.href === href);
    if (!item?.parentKeys.length) return;
    const next = new Set(expandedTocKeys);
    for (const key of item.parentKeys) next.add(key);
    expandedTocKeys = next;
  }

  $effect(() => {
    applicationTheme;
    const next = { ...preferences };
    settings = next;
    applyReaderPreferences(next);
  });

  function readerStyles(readerPreferences: ReaderPreferences) {
    const colors = designTokens.themes[applicationTheme];
    const family = designTokens.font_families[readerPreferences.font_family];
    const familyRule = family ? `font-family: ${family} !important;` : "";
    const alignmentRule = readerPreferences.text_alignment === "publisher" ? "" : `text-align: ${readerPreferences.text_alignment} !important;`;
    const hyphens = readerPreferences.hyphenation ? "auto" : "manual";
    return `
      :root { color-scheme: light; font-size: ${readerPreferences.font_size_px}px !important; }
      html, body { background: ${colors.app_bg} !important; color: ${colors.ink_text} !important; ${familyRule} }
      p, li, blockquote, dd { line-height: ${readerPreferences.line_height} !important; ${alignmentRule} -webkit-hyphens: ${hyphens}; hyphens: ${hyphens}; }
      a { color: ${colors.accent} !important; }
      img, svg { max-width: 100% !important; max-height: 95vh !important; object-fit: contain; }
    `;
  }

  function contrastRatio(first: string, second: string) {
    const luminance = (hex: string) => {
      const channels = [1, 3, 5].map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16) / 255).map((value) => value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);
      return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
    };
    const values = [luminance(first), luminance(second)].sort((left, right) => right - left);
    return (values[0] + 0.05) / (values[1] + 0.05);
  }

  function applyPreferencesToRenderer(renderer: FoliateRenderer | undefined, readerPreferences: ReaderPreferences) {
    if (!renderer) return;
    renderer.setAttribute("flow", readerPreferences.flow);
    renderer.setAttribute("max-inline-size", `${readerPreferences.max_line_width_px}px`);
    renderer.setAttribute("max-column-count", String(readerPreferences.max_column_count));
    renderer.setAttribute("zoom", String(readerPreferences.zoom));
    renderer.setAttribute("margin", `${readerMargin}px`);
    renderer.setAttribute("gap", `${pageGap}%`);
    renderer.setAttribute("max-block-size", `${maxBlockSize}px`);
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
        await book.saveCfi(cfi, readingFraction * 100);
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
    selectionContextMenu = null;
    const detail = (event as CustomEvent<FoliateRelocateDetail>).detail;
    activeTocHref = detail.tocItem?.href || "";
    readingFraction = Math.max(0, Math.min(1, detail.fraction ?? readingFraction));
    currentLocation = detail.location?.current ?? currentLocation;
    totalLocations = detail.location?.total ?? totalLocations;
    currentSection = detail.section?.current ?? currentSection;
    totalSections = detail.section?.total ?? totalSections;
    currentPageLabel = detail.pageItem?.label || "";
    expandActiveTocAncestors(activeTocHref);
    if (detail.cfi) scheduleSave(detail.cfi);
  }

  function updateHistoryAvailability() {
    canHistoryBack = view?.history?.canGoBack ?? false;
    canHistoryForward = view?.history?.canGoForward ?? false;
  }

  function updatePreference<K extends keyof ReaderPreferences>(key: K, value: ReaderPreferences[K]) {
    settings = { ...settings, [key]: value };
    applyReaderPreferences();
    void onpreferenceschange({ preference: key, value } as ReaderPreferencePatch);
  }

  function resetReaderSettings() {
    settings = { ...DEFAULT_READER_PREFERENCES };
    readerMargin = 48;
    pageGap = 7;
    maxBlockSize = 1440;
    applyReaderPreferences(settings);
    for (const [preference, value] of Object.entries(DEFAULT_READER_PREFERENCES)) {
      if (preference === "theme") continue;
      void onpreferenceschange({ preference, value } as ReaderPreferencePatch);
    }
  }

  function closePanels() {
    settingsOpen = false;
    setTocOpen(false);
    searchOpen = false;
    notesOpen = false;
    infoOpen = false;
  }

  function togglePanel(panel: "toc" | "search" | "settings" | "notes" | "info") {
    const next = panel === "toc" ? !tocOpen : panel === "search" ? !searchOpen : panel === "notes" ? !notesOpen : panel === "info" ? !infoOpen : !settingsOpen;
    closePanels();
    if (panel === "toc") setTocOpen(next);
    if (panel === "search") searchOpen = next;
    if (panel === "settings") settingsOpen = next;
    if (panel === "notes") notesOpen = next;
    if (panel === "info") infoOpen = next;
  }

  function pdfAnnotationAnchor(annotation: ReaderAnnotation): PdfAnnotationAnchor | null {
    return annotation.anchor.kind === "pdf" ? annotation.anchor : null;
  }

  function annotationCfi(annotation: ReaderAnnotation) {
    return annotation.anchor.kind === "epub_cfi" ? annotation.anchor.cfi : annotation.anchor.fallback_cfi || "";
  }

  function annotationOverlay(annotation: ReaderAnnotation) {
    const value = annotationCfi(annotation);
    if (!value.startsWith("epubcfi(")) return null;
    return { value, color: annotation.color, style: annotation.style, id: annotation.id };
  }

  function paintPdfAnnotations(doc: Document, index: number) {
    doc.querySelector(".bokheim-pdf-annotation-layer")?.remove();
    if (!isPdfBook) return;
    const canvas = doc.querySelector<HTMLCanvasElement>("#canvas canvas");
    if (!canvas) return;
    const layer = doc.createElement("div");
    layer.className = "bokheim-pdf-annotation-layer";
    Object.assign(layer.style, {
      position: "absolute", left: "0", top: "0", width: `${canvas.width}px`, height: `${canvas.height}px`,
      zIndex: "3", pointerEvents: "none",
    });
    for (const annotation of annotations) {
      const anchor = pdfAnnotationAnchor(annotation);
      if (!anchor || anchor.page_index !== index) continue;
      for (const rect of anchor.rects) {
        const mark = doc.createElement("span");
        Object.assign(mark.style, {
          position: "absolute", left: `${rect.left * 100}%`, top: `${rect.top * 100}%`,
          width: `${rect.width * 100}%`, height: `${rect.height * 100}%`, boxSizing: "border-box",
        });
        if (annotation.style === "highlight") {
          mark.style.background = annotation.color;
          mark.style.opacity = "0.38";
        } else if (annotation.style === "strikethrough") {
          mark.style.borderTop = `2px solid ${annotation.color}`;
          mark.style.transform = "translateY(52%)";
        } else {
          mark.style.borderBottom = `2px ${annotation.style === "squiggly" ? "dotted" : "solid"} ${annotation.color}`;
        }
        layer.append(mark);
      }
    }
    doc.body.style.position = "relative";
    doc.body.append(layer);
  }

  function repaintPdfAnnotations() {
    if (!isPdfBook) return;
    for (const content of view?.renderer?.getContents?.() ?? []) paintPdfAnnotations(content.doc, content.index);
  }

  async function restoreAnnotations() {
    if (!book.loadAnnotations) return;
    try {
      annotations = await book.loadAnnotations();
      if (isPdfBook) {
        repaintPdfAnnotations();
        for (const annotation of annotations.filter((item) => !pdfAnnotationAnchor(item))) {
          const overlay = annotationOverlay(annotation);
          if (overlay) await view?.addAnnotation(overlay);
        }
      } else for (const annotation of annotations) {
        const overlay = annotationOverlay(annotation);
        if (overlay) await view?.addAnnotation(overlay);
      }
    } catch (reason) {
      readerStateError = reason instanceof Error ? reason.message : String(reason);
    }
  }

  function handleCreateOverlay() {
    if (isPdfBook) {
      repaintPdfAnnotations();
      for (const annotation of annotations.filter((item) => !pdfAnnotationAnchor(item))) {
        const overlay = annotationOverlay(annotation);
        if (overlay) void view?.addAnnotation(overlay);
      }
      return;
    }
    for (const annotation of annotations) {
      const overlay = annotationOverlay(annotation);
      if (overlay) void view?.addAnnotation(overlay);
    }
  }

  function handleSectionLoad(event: Event) {
    const { doc, index } = (event as CustomEvent<{ doc: Document; index: number }>).detail;
    let lastPointerType = "";
    if (isPdfBook) {
      setTimeout(() => paintPdfAnnotations(doc, index));
      const canvasHost = doc.querySelector("#canvas");
      if (canvasHost) new MutationObserver(() => queueMicrotask(() => paintPdfAnnotations(doc, index))).observe(canvasHost, { childList: true });
    }
    const capture = () => {
      const selected = doc.getSelection();
      if (!selected?.rangeCount || selected.isCollapsed) return;
      const range = selected.getRangeAt(0).cloneRange();
      const text = selected.toString().trim();
      if (!text) return;
      const cfi = view?.getCFI(index, range) || "";
      const canvasBounds = doc.querySelector<HTMLCanvasElement>("#canvas canvas")?.getBoundingClientRect();
      const rects = canvasBounds
        ? Array.from(range.getClientRects()).map((rect) => ({
            left: Math.max(0, (rect.left - canvasBounds.left) / Math.max(1, canvasBounds.width)),
            top: Math.max(0, (rect.top - canvasBounds.top) / Math.max(1, canvasBounds.height)),
            width: Math.min(1, rect.width / Math.max(1, canvasBounds.width)),
            height: Math.min(1, rect.height / Math.max(1, canvasBounds.height)),
          })).filter((rect) => rect.width > 0 && rect.height > 0 && rect.left + rect.width <= 1.00001 && rect.top + rect.height <= 1.00001)
        : [];
      selection = {
        cfi,
        text,
        pdfAnchor: isPdfBook && rects.length ? { kind: "pdf", page_index: index, rects, fallback_cfi: cfi || null } : undefined,
      };
      selectionNote = "";
    };
    doc.addEventListener("pointerdown", (event) => {
      selectionContextMenu = null;
      lastPointerType = event.pointerType;
    });
    doc.addEventListener("pointerup", () => setTimeout(capture));
    doc.addEventListener("pointermove", revealChromeAtTopEdge);
    doc.addEventListener("keyup", capture);
    doc.addEventListener("keydown", handleReaderKey);
    doc.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      const selected = doc.getSelection();
      if (!selected?.rangeCount || selected.isCollapsed) {
        selectionContextMenu = null;
        return;
      }
      const range = selected.getRangeAt(0);
      const isOverSelection = Array.from(range.getClientRects()).some((rect) =>
        event.clientX >= rect.left && event.clientX <= rect.right
        && event.clientY >= rect.top && event.clientY <= rect.bottom);
      if (!isOverSelection) {
        selectionContextMenu = null;
        return;
      }
      capture();
      const frame = doc.defaultView?.frameElement as HTMLElement | null;
      const frameBounds = frame?.getBoundingClientRect();
      const scaleX = frameBounds && doc.defaultView?.innerWidth ? frameBounds.width / doc.defaultView.innerWidth : 1;
      const scaleY = frameBounds && doc.defaultView?.innerHeight ? frameBounds.height / doc.defaultView.innerHeight : 1;
      const x = (frameBounds?.left ?? 0) + event.clientX * scaleX;
      const y = (frameBounds?.top ?? 0) + event.clientY * scaleY;
      selectionContextMenu = {
        x: Math.max(8, Math.min(x, window.innerWidth - 170)),
        y: Math.max(8, Math.min(y, window.innerHeight - 150)),
      };
    });
    doc.addEventListener("click", (event) => {
      const image = (event.target as Element | null)?.closest?.("img,svg") as HTMLImageElement | SVGElement | null;
      if (image instanceof HTMLImageElement && image.currentSrc) imageViewerSrc = image.currentSrc;
      if (!tapZonesEnabled || image || (event.target as Element | null)?.closest?.("a,button,input,textarea,select") || doc.getSelection()?.type === "Range" || settingsOpen || tocOpen || searchOpen || notesOpen || infoOpen || footnoteOpen) return;
      const ratio = (event as MouseEvent).clientX / Math.max(1, doc.documentElement.clientWidth);
      if (ratio >= 0.3 && ratio <= 0.7) chromeVisible = !chromeVisible;
      else if (lastPointerType === "touch" && !isKoboReader()) {
        if (ratio < 0.3) void view?.goLeft();
        else void view?.goRight();
      }
    });
  }

  function revealChromeAtTopEdge(event: PointerEvent) {
    if (!chromeVisible && event.pointerType === "mouse" && event.clientY <= 8) chromeVisible = true;
  }

  function handleDrawAnnotation(event: Event) {
    const { draw, annotation, doc } = (event as CustomEvent<{ draw: (renderer: unknown, options: unknown) => void; annotation: { style?: ReaderAnnotationStyle; color?: string }; doc: Document }>).detail;
    const renderer = annotation.style === "underline" ? Overlayer.underline
      : annotation.style === "squiggly" ? Overlayer.squiggly
      : annotation.style === "strikethrough" ? Overlayer.strikethrough
      : Overlayer.highlight;
    draw(renderer, { color: annotation.color || "#f6c945", writingMode: getComputedStyle(doc.documentElement).writingMode });
  }

  function handleShowAnnotation(event: Event) {
    const cfi = (event as CustomEvent<{ value: string }>).detail.value;
    const annotation = annotations.find((item) => annotationCfi(item) === cfi);
    if (!annotation) return;
    selection = { cfi, text: annotation.exact_text, pdfAnchor: pdfAnnotationAnchor(annotation) ?? undefined };
    selectionNote = annotation.note;
  }

  async function showStoredAnnotation(annotation: ReaderAnnotation) {
    const anchor = pdfAnnotationAnchor(annotation);
    if (anchor && isPdfBook) {
      await view?.goTo(anchor.page_index);
      selection = { cfi: anchor.fallback_cfi || "", text: annotation.exact_text, pdfAnchor: anchor };
      selectionNote = annotation.note;
      activeAnnotation = annotation;
      annotationNoteDraft = annotation.note;
      return;
    }
    const overlay = annotationOverlay(annotation);
    if (overlay) await view?.showAnnotation(overlay);
    activeAnnotation = annotation;
    annotationNoteDraft = annotation.note;
  }

  async function saveSelection(style: ReaderAnnotationStyle = "highlight"): Promise<ReaderAnnotation | undefined> {
    if (!selection || !book.bookId || !book.upsertAnnotation) return;
    selectionContextMenu = null;
    const anchor = selection.pdfAnchor ?? { kind: "epub_cfi" as const, cfi: selection.cfi };
    const existing = annotations.find((item) => JSON.stringify(item.anchor) === JSON.stringify(anchor) || annotationCfi(item) === selection?.cfi);
    const now = Date.now();
    const annotation: ReaderAnnotation = {
      id: existing?.id || crypto.randomUUID(), content_hash: book.bookId, anchor,
      exact_text: selection.text,
      style: existing?.style || style, color: existing?.color || "#f6c945", note: selectionNote,
      created_at: existing?.created_at || now, modified_at: now,
    };
    try {
      await book.upsertAnnotation(annotation);
      const existingOverlay = existing ? annotationOverlay(existing) : null;
      if (!isPdfBook && existingOverlay) await view?.deleteAnnotation(existingOverlay);
      annotations = [annotation, ...annotations.filter((item) => item.id !== annotation.id)];
      if (isPdfBook) repaintPdfAnnotations();
      else {
        const overlay = annotationOverlay(annotation);
        if (overlay) await view?.addAnnotation(overlay);
      }
      selection = null;
      return annotation;
    } catch (reason) {
      readerStateError = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function saveSelectionAndOpenAnnotation() {
    const annotation = await saveSelection("highlight");
    if (!annotation) return;
    activeAnnotation = annotation;
    annotationNoteDraft = annotation.note;
  }

  async function saveActiveAnnotation() {
    if (!activeAnnotation || !book.upsertAnnotation) return;
    const annotation = { ...activeAnnotation, note: annotationNoteDraft, modified_at: Date.now() };
    try {
      await book.upsertAnnotation(annotation);
      annotations = annotations.map((item) => item.id === annotation.id ? annotation : item);
      activeAnnotation = null;
    } catch (reason) {
      readerStateError = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function removeAnnotation(annotation: ReaderAnnotation) {
    if (!book.deleteAnnotation) return;
    await book.deleteAnnotation(annotation.id, Date.now());
    const overlay = annotationOverlay(annotation);
    if (!isPdfBook && overlay) await view?.deleteAnnotation(overlay);
    annotations = annotations.filter((item) => item.id !== annotation.id);
    if (isPdfBook) repaintPdfAnnotations();
    selection = null;
  }

  async function addBookmark() {
    if (!lastCfi || !book.bookId || !book.createBookmark) return;
    const now = Date.now();
    const bookmark: ReaderBookmark = { id: crypto.randomUUID(), content_hash: book.bookId, cfi: lastCfi, label: tocItems.find((item) => item.href === activeTocHref)?.label || `Location ${currentLocation + 1}`, created_at: now, modified_at: now };
    await book.createBookmark(bookmark);
    bookmarks = [bookmark, ...bookmarks];
  }

  async function copySelectionCitation() {
    if (!selection) return;
    selectionContextMenu = null;
    const attribution = [book.title, book.author].filter(Boolean).join(" — ");
    const locator = selection.cfi ? ` (${selection.cfi})` : "";
    await navigator.clipboard.writeText(`“${selection.text}”\n\n${attribution}${locator}`);
  }

  async function removeBookmark(bookmark: ReaderBookmark) {
    if (!book.deleteBookmark) return;
    await book.deleteBookmark(bookmark.id, Date.now());
    bookmarks = bookmarks.filter((item) => item.id !== bookmark.id);
  }

  function speechText(payload: string | undefined) {
    if (!payload) return "";
    return new DOMParser().parseFromString(payload, "application/xml").documentElement.textContent?.trim() || "";
  }

  function speakTts(payload: string | undefined) {
    const text = speechText(payload);
    if (!text) {
      stopTts();
      return;
    }
    speechSynthesis.cancel();
    const utterance = new SpeechSynthesisUtterance(text);
    utterance.rate = ttsRate;
    utterance.onend = () => {
      if (ttsPlaying && !ttsPaused) speakTts(view?.tts?.next());
    };
    utterance.onerror = () => { ttsPlaying = false; ttsPaused = false; };
    speechSynthesis.speak(utterance);
  }

  async function toggleTts() {
    if (!ttsPlaying) {
      await view?.initTTS("sentence");
      ttsPlaying = true;
      ttsPaused = false;
      speakTts(view?.tts?.start());
    } else if (ttsPaused) {
      speechSynthesis.resume();
      ttsPaused = false;
    } else {
      speechSynthesis.pause();
      ttsPaused = true;
    }
  }

  function stopTts() {
    speechSynthesis.cancel();
    ttsPlaying = false;
    ttsPaused = false;
  }

  function skipTts(direction: -1 | 1) {
    speakTts(direction < 0 ? view?.tts?.prev(ttsPaused) : view?.tts?.next(ttsPaused));
  }

  async function toggleMediaOverlay() {
    if (!view?.mediaOverlay) return;
    if (mediaOverlayPlaying) {
      view.mediaOverlay.pause();
      mediaOverlayPlaying = false;
    } else {
      view.mediaOverlay.setRate(ttsRate);
      if (view.startMediaOverlay) await view.startMediaOverlay();
      else view.mediaOverlay.resume();
      mediaOverlayPlaying = true;
    }
  }

  function stepMediaOverlay(direction: -1 | 1) {
    if (direction < 0) view?.mediaOverlay?.prev();
    else view?.mediaOverlay?.next();
  }

  function stopMediaOverlay() {
    view?.mediaOverlay?.stop();
    mediaOverlayPlaying = false;
  }

  async function goToTocItem(item: TocItem) {
    if (!item.href || !view) return;
    await view.goTo(item.href);
    closePanels();
  }

  function goToProgress(event: Event) {
    const fraction = Number((event.currentTarget as HTMLInputElement).value);
    readingFraction = fraction;
    void view?.goToFraction(fraction);
  }

  function goToSection(offset: number) {
    if (!view || !totalSections) return;
    const target = Math.max(0, Math.min(totalSections - 1, currentSection + offset));
    if (target !== currentSection) void view.goTo(target);
  }

  function navigationKey(key: string): ReaderNavigationKey | null {
    if (key === "ArrowUp") return "arrow_up";
    if (key === "ArrowDown") return "arrow_down";
    if (key === "ArrowLeft") return "arrow_left";
    if (key === "ArrowRight") return "arrow_right";
    if (key === "PageUp") return "page_up";
    if (key === "PageDown") return "page_down";
    if (key === " " || key === "Spacebar") return "space";
    return null;
  }

  function isTextEntryTarget(target: EventTarget | null) {
    const element = target as (Element & { closest?: Element["closest"] }) | null;
    return Boolean(element && typeof element.closest === "function" && element.closest("input, textarea, select, [contenteditable='true']"));
  }

  function performNavigation(action: ReaderNavigationAction) {
    if (!view) return;
    const lineDistance = Math.max(1, settings.font_size_px * settings.line_height);
    if (action === "previous_line") void view.prev(lineDistance);
    if (action === "next_line") void view.next(lineDistance);
    if (action === "previous_page") void view.prev();
    if (action === "next_page") void view.next();
    if (action === "previous_section") goToSection(-1);
    if (action === "next_section") goToSection(1);
    if (action === "history_back") view.history?.back();
    if (action === "history_forward") view.history?.forward();
  }

  function handleReaderKey(event: KeyboardEvent) {
    if (!isTextEntryTarget(event.target)) {
      const key = navigationKey(event.key);
      const shortcut = key
        ? READER_NAVIGATION_CONTRACT.shortcuts.find((candidate) =>
            candidate.key === key
            && candidate.primary_modifier === (event.ctrlKey || event.metaKey)
            && candidate.alt === event.altKey
            && candidate.shift === event.shiftKey)
        : undefined;
      if (shortcut) {
        event.preventDefault();
        performNavigation(shortcut.action);
        return;
      }
      if (event.altKey && !event.ctrlKey && !event.metaKey && (event.key === "+" || event.key === "=" || event.key === "-")) {
        event.preventDefault();
        const delta = event.key === "-" ? -1 : 1;
        const [minimum, maximum] = preferenceConstraints.font_size_px;
        updatePreference("font_size_px", Math.min(maximum, Math.max(minimum, settings.font_size_px + delta)));
        return;
      }
    }

    if (event.key === "Escape") void requestClose();
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
      event.preventDefault();
      togglePanel("search");
    }
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "g") {
      event.preventDefault();
      cycleSearchResult(event.shiftKey ? -1 : 1);
    }
  }

  async function runSearch(event?: SubmitEvent) {
    event?.preventDefault();
    const query = searchQuery.trim();
    const currentView = view;
    const generation = ++searchGeneration;
    searchResults = [];
    searchProgress = 0;
    searchError = "";
    selectedSearchIndex = -1;
    currentView?.clearSearch();
    if (!query || !currentView) return;

    searching = true;
    try {
      for await (const result of currentView.search({
        query,
        index: searchScope === "section" ? currentSection : undefined,
        matchCase: searchMatchCase,
        matchDiacritics: searchMatchDiacritics,
        matchWholeWords: searchMatchWholeWords,
      })) {
        if (generation !== searchGeneration) return;
        if (typeof result === "string") continue;
        if ("progress" in result) {
          searchProgress = result.progress;
          continue;
        }
        if ("subitems" in result) {
          const group = result as FoliateSearchGroup;
          searchResults = [...searchResults, ...group.subitems.map((match) => ({ ...match, section: group.label || "Book" }))];
        } else if ("cfi" in result) {
          searchResults = [...searchResults, { ...result, section: activeTocHref ? tocItems.find((item) => item.href === activeTocHref)?.label || "Current section" : "Current section" }];
        }
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

  async function selectSearchResult(result: SearchResult, index: number) {
    selectedSearchIndex = index;
    await view?.select(result.cfi);
  }

  function cycleSearchResult(offset: number) {
    if (!searchResults.length) return;
    const index = selectedSearchIndex < 0
      ? (offset > 0 ? 0 : searchResults.length - 1)
      : (selectedSearchIndex + offset + searchResults.length) % searchResults.length;
    void selectSearchResult(searchResults[index], index);
  }

  function clearSearch() {
    searchGeneration += 1;
    searching = false;
    searchResults = [];
    searchProgress = 0;
    searchError = "";
    selectedSearchIndex = -1;
    view?.clearSearch();
  }

  function retryPositionSave() {
    if (lastCfi) void persistCfi(lastCfi);
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
    if (settingsOpen || tocOpen || searchOpen || notesOpen) {
      closePanels();
      return;
    }
    // Closing the reader must not wait for network persistence. The save queue
    // continues in the background and the component cleanup flushes again.
    void flushPosition();
    onclose();
  }

  onMount(() => {
    tocOpen = initialTocState();

    let disposed = false;
    let unlistenCloseRequested: (() => void) | undefined;
    lastPersistedCfi = book.initialCfi || "";
    const element = document.createElement("foliate-view") as FoliateView;
    element.className = "foliate-view";
    element.addEventListener("relocate", handleRelocate);
    element.addEventListener("link", handleInternalLink);
    element.addEventListener("external-link", openExternalLink);
    element.addEventListener("load", handleSectionLoad);
    element.addEventListener("draw-annotation", handleDrawAnnotation);
    element.addEventListener("show-annotation", handleShowAnnotation);
    element.addEventListener("create-overlay", handleCreateOverlay);
    element.history?.addEventListener("index-change", updateHistoryAvailability);
    host.append(element);
    view = element;
    updateHistoryAvailability();

    const unlistenVolumePageTurn = onVolumePageTurn((direction) => {
      if (direction === "next") void view?.goRight();
      else void view?.goLeft();
    });
    enableVolumePageTurning(true);
    const handleVisibilityChange = () => {
      if (document.visibilityState === "hidden") void flushPosition();
    };
    const handlePageHide = () => void flushPosition();
    const closeSelectionContextMenu = (event?: Event) => {
      const target = event?.target as Element | null;
      if (target?.closest?.(".selection-context-menu")) return;
      selectionContextMenu = null;
    };
    window.addEventListener("keydown", handleReaderKey);
    window.addEventListener("pointerdown", closeSelectionContextMenu, true);
    window.addEventListener("resize", closeSelectionContextMenu);
    window.addEventListener("pagehide", handlePageHide);
    document.addEventListener("visibilitychange", handleVisibilityChange);

    if (isNativeApp) {
      void getCurrentWindow()
        .onCloseRequested((event) => {
          event.preventDefault();
          void requestClose();
        })
        .then((unlisten) => {
          if (disposed) unlisten();
          else unlistenCloseRequested = unlisten;
        })
        .catch((reason) => {
          console.error("Could not register the native reader window lifecycle", reason);
        });
    }

    void (async () => {
      try {
        await element.open(book.file);
        if (disposed) return;
        hasMediaOverlay = Boolean(element.mediaOverlay);
        tocItems = flattenToc(element.book?.toc ?? []);
        landmarkItems = flattenToc(element.book?.landmarks ?? []);
        pageListItems = flattenToc(element.book?.pageList ?? []);
        expandedTocKeys = new Set(tocItems.filter((item) => item.depth === 0 && item.hasChildren).map((item) => item.key));
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
        if (disposed) return;
        loading = false;

        // Annotations and bookmarks enrich an already usable reader. Loading
        // them must not keep the first rendered page behind the opening veil.
        void restoreAnnotations();
        if (book.loadBookmarks) {
          void book.loadBookmarks()
            .then((loaded) => {
              if (!disposed) bookmarks = loaded;
            })
            .catch((reason) => {
              if (!disposed) readerStateError = reason instanceof Error ? reason.message : String(reason);
            });
        }
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
      window.removeEventListener("keydown", handleReaderKey);
      window.removeEventListener("pointerdown", closeSelectionContextMenu, true);
      window.removeEventListener("resize", closeSelectionContextMenu);
      window.removeEventListener("pagehide", handlePageHide);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      element.removeEventListener("relocate", handleRelocate);
      element.removeEventListener("link", handleInternalLink);
      element.removeEventListener("external-link", openExternalLink);
      element.removeEventListener("load", handleSectionLoad);
      element.removeEventListener("draw-annotation", handleDrawAnnotation);
      element.removeEventListener("show-annotation", handleShowAnnotation);
      element.removeEventListener("create-overlay", handleCreateOverlay);
      element.history?.removeEventListener("index-change", updateHistoryAvailability);
      footnoteHandler?.removeEventListener("before-render", prepareFootnote);
      footnoteHandler?.removeEventListener("render", showFootnote);
      footnoteHandler = null;
      clearSearch();
      stopTts();
      element.mediaOverlay?.stop();
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

<main
  class:hidden-chrome={!chromeVisible}
  class:has-sidebar={tocOpen || notesOpen || infoOpen}
  class:search-active={searchOpen}
  class="reader-shell"
  data-reader-settings-presentation={READER_SETTINGS_PRESENTATION_CONTRACT.presentation}
  onpointermove={revealChromeAtTopEdge}
  style={`--reader-settings-max-width: ${READER_SETTINGS_PRESENTATION_CONTRACT.max_width_px}px; --reader-settings-max-height: ${READER_SETTINGS_PRESENTATION_CONTRACT.max_height_px}px; --reader-settings-padding: ${READER_SETTINGS_PRESENTATION_CONTRACT.padding_px}px; --reader-settings-gap: ${READER_SETTINGS_PRESENTATION_CONTRACT.gap_px}px; --reader-settings-min-column-width: ${READER_SETTINGS_PRESENTATION_CONTRACT.minimum_column_width_px}px;`}
>
  <header class="reader-header" aria-label="Reader controls">
    <button class="reader-control" type="button" onclick={() => void requestClose()}>Back</button>
    <button class="reader-control square" type="button" disabled={!canHistoryBack} aria-label="Link history back" title="Link history back (Alt+←)" onclick={() => view?.history?.back()}>←</button>
    <button class="reader-control square" type="button" disabled={!canHistoryForward} aria-label="Link history forward" title="Link history forward (Alt+→)" onclick={() => view?.history?.forward()}>→</button>
    <button class:active={tocOpen} class="reader-control" type="button" onclick={() => togglePanel("toc")}>Contents</button>
    {#if book.loadAnnotations}
      <button class:active={notesOpen} class="reader-control" type="button" title="Annotations and bookmarks" onclick={() => togglePanel("notes")}>Annotations</button>
    {/if}
    <button class:active={infoOpen} class="reader-control" type="button" title="Book information" onclick={() => togglePanel("info")}>Info</button>
    {#if book.createBookmark}
      <button class="reader-control" type="button" disabled={!lastCfi} title="Bookmark the current location" onclick={() => void addBookmark()}>Bookmark</button>
    {/if}
    <button class:active={ttsPlaying} class="reader-control" type="button" title="Read the visible page aloud" onclick={() => ttsPlaying ? stopTts() : void toggleTts()}>{ttsPlaying ? "Stop" : "Read"}</button>
    {#if hasMediaOverlay}
      <button class="reader-control square" type="button" aria-label="Previous media-overlay segment" onclick={() => stepMediaOverlay(-1)}>◀</button>
      <button class:active={mediaOverlayPlaying} class="reader-control" type="button" onclick={() => void toggleMediaOverlay()}>{mediaOverlayPlaying ? "Pause audio" : "Book audio"}</button>
      <button class="reader-control square" type="button" aria-label="Next media-overlay segment" onclick={() => stepMediaOverlay(1)}>▶</button>
      {#if mediaOverlayPlaying}<button class="reader-control" type="button" onclick={stopMediaOverlay}>Stop audio</button>{/if}
    {/if}
    <div class="book-heading" title={book.title}>
      <strong>{book.title}</strong>
    </div>
    <button class:active={searchOpen} class="reader-control" type="button" title="Search in book (Ctrl+F)" onclick={() => togglePanel("search")}>Search</button>
    <button class:active={settingsOpen} class="reader-control" type="button" aria-label="Reader settings" title="Reader settings" onclick={() => togglePanel("settings")}>{READER_SETTINGS_PRESENTATION_CONTRACT.trigger_label}</button>
  </header>

  {#if tocOpen}
    <section class="reader-panel toc-panel" aria-label="Table of contents">
      <div class="panel-heading">
        <strong>Contents</strong>
        <button type="button" aria-label="Close table of contents" onclick={() => setTocOpen(false)}>×</button>
      </div>
      <div class="navigation-tabs" role="tablist" aria-label="Book navigation">
        <button class:active={navigationList === "toc"} type="button" onclick={() => (navigationList = "toc")}>Contents</button>
        {#if landmarkItems.length}<button class:active={navigationList === "landmarks"} type="button" onclick={() => (navigationList = "landmarks")}>Landmarks</button>{/if}
        {#if pageListItems.length}<button class:active={navigationList === "pages"} type="button" onclick={() => (navigationList = "pages")}>Pages</button>{/if}
      </div>
      {#if visibleTocItems().length}
        <nav class="toc-list">
          {#each visibleTocItems() as item (item.key)}
            <div class:current={item.href === activeTocHref} class="toc-row" style={`--toc-depth: ${item.depth}`}>
              {#if item.hasChildren}
                <button
                  class="toc-disclosure"
                  type="button"
                  aria-label={`${expandedTocKeys.has(item.key) ? "Collapse" : "Expand"} ${item.label}`}
                  aria-expanded={expandedTocKeys.has(item.key)}
                  onclick={() => toggleTocItem(item)}
                >{expandedTocKeys.has(item.key) ? "▾" : "▸"}</button>
              {:else}
                <span class="toc-disclosure-spacer" aria-hidden="true"></span>
              {/if}
              <button class="toc-target" type="button" disabled={!item.href} onclick={() => void goToTocItem(item)}>{item.label}</button>
            </div>
          {/each}
        </nav>
      {:else}
        <p class="panel-empty">This navigation list is empty.</p>
      {/if}
    </section>
  {/if}

  {#if infoOpen}
    <section class="reader-panel info-panel" aria-label="Book information">
      <div class="panel-heading"><strong>Book information</strong><button type="button" aria-label="Close book information" onclick={() => (infoOpen = false)}>×</button></div>
      <dl>
        <dt>Title</dt><dd>{book.title}</dd>
        {#if book.author}<dt>Author</dt><dd>{book.author}</dd>{/if}
        {#if view?.book?.metadata?.language}<dt>Language</dt><dd>{String(view.book.metadata.language)}</dd>{/if}
        {#if view?.book?.metadata?.publisher}<dt>Publisher</dt><dd>{String(view.book.metadata.publisher)}</dd>{/if}
        {#if view?.book?.metadata?.published}<dt>Published</dt><dd>{String(view.book.metadata.published)}</dd>{/if}
        {#if view?.book?.metadata?.identifier}<dt>Identifier</dt><dd>{String(view.book.metadata.identifier)}</dd>{/if}
      </dl>
    </section>
  {/if}

  {#if notesOpen}
    <section class="reader-panel notes-panel" aria-label="Annotations and bookmarks">
      <div class="panel-heading">
        <strong>Annotations & bookmarks</strong>
        <button type="button" aria-label="Close annotations" onclick={() => (notesOpen = false)}>×</button>
      </div>
      <div class="notes-actions">
        <button type="button" disabled={!lastCfi} onclick={() => void addBookmark()}>Bookmark here</button>
      </div>
      <input bind:value={annotationFilter} type="search" placeholder="Filter annotations" aria-label="Filter annotations" />
      {#if readerStateError}<p class="panel-error">{readerStateError}</p>{/if}
      <div class="annotation-list">
        {#each annotations.filter((item) => `${item.exact_text} ${item.note}`.toLocaleLowerCase().includes(annotationFilter.toLocaleLowerCase())) as annotation (annotation.id)}
          <article>
            <button class="annotation-target" type="button" onclick={() => void showStoredAnnotation(annotation)}>
              <span class="annotation-color" style={`--annotation-color: ${annotation.color}`}></span>
              <q>{annotation.exact_text}</q>
              {#if annotation.note}<span>{annotation.note}</span>{/if}
            </button>
            <button type="button" aria-label="Delete annotation" onclick={() => void removeAnnotation(annotation)}>×</button>
          </article>
        {/each}
        {#if !annotations.length}<p class="panel-empty">Select text in the book to create your first highlight.</p>{/if}
      </div>
      {#if bookmarks.length}
        <strong class="subheading">Bookmarks</strong>
        <div class="bookmark-list">
          {#each bookmarks as bookmark (bookmark.id)}
            <div><button type="button" onclick={() => void view?.goTo(bookmark.cfi)}>{bookmark.label}</button><button type="button" aria-label="Delete bookmark" onclick={() => void removeBookmark(bookmark)}>×</button></div>
          {/each}
        </div>
      {/if}
    </section>
  {/if}

  {#if searchOpen}
    <section class="reader-search-bar" aria-label="Search book">
      <form class="search-form" onsubmit={runSearch}>
        <input bind:value={searchQuery} type="search" enterkeyhint="search" placeholder="Search this book" aria-label="Search text" oninput={() => void runSearch()} />
        <span class="reader-match-count" aria-live="polite">{searchResults.length ? `${Math.max(1, selectedSearchIndex + 1)}/${searchResults.length}` : "0/0"}</span>
        <button class:active={searchMatchCase} type="button" title="Match case" onclick={() => { searchMatchCase = !searchMatchCase; void runSearch(); }}>Aa</button>
        <button class:active={searchMatchWholeWords} type="button" title="Match whole words" onclick={() => { searchMatchWholeWords = !searchMatchWholeWords; void runSearch(); }}>Word</button>
        <button class:active={searchMatchDiacritics} type="button" title="Match diacritics exactly" onclick={() => { searchMatchDiacritics = !searchMatchDiacritics; void runSearch(); }}>Marks</button>
        <button class:active={searchScope === "book"} type="button" title="Search current section or whole book" onclick={() => { searchScope = searchScope === "book" ? "section" : "book"; void runSearch(); }}>{searchScope === "book" ? "Book" : "Section"}</button>
        <button class="square" type="button" aria-label="Previous match" title="Previous match (Shift+Enter)" onclick={() => cycleSearchResult(-1)}>‹</button>
        <button class="square" type="button" aria-label="Next match" title="Next match (Enter)" onclick={() => cycleSearchResult(1)}>›</button>
        <button class="square" type="button" aria-label="Close search" title="Close search (Esc)" onclick={() => { clearSearch(); searchOpen = false; }}>×</button>
      </form>
      {#if searchError}<span class="search-inline-error">{searchError}</span>{/if}
    </section>
  {/if}

  {#if settingsOpen}
    <section class="reader-panel settings-panel" aria-label="Reading settings">
      <div class="panel-heading">
        <strong>{READER_SETTINGS_PRESENTATION_CONTRACT.title}</strong>
        <button type="button" aria-label="Close reading settings" onclick={() => (settingsOpen = false)}>×</button>
      </div>
      <div class="setting-option-row">
        <span>Column width</span>
        <div class="setting-button-group">
          {#each maxLineWidthOptions as option}
            <button
              type="button"
              class:active={settings.max_line_width_px === option}
              aria-pressed={settings.max_line_width_px === option}
              onclick={() => updatePreference("max_line_width_px", option)}
            >
              {option}px
            </button>
          {/each}
        </div>
      </div>
      <div class="setting-option-row">
        <span>Font size</span>
        <div class="setting-button-group">
          {#each readerFontSizeOptions as option}
            <button
              type="button"
              class:active={settings.font_size_px === option}
              aria-pressed={settings.font_size_px === option}
              onclick={() => updatePreference("font_size_px", option)}
            >
              {option}px
            </button>
          {/each}
        </div>
      </div>
      <div class="setting-option-row">
        <span>Zoom</span>
        <div class="setting-button-group">
          {#each zoomOptions as option}
            <button
              type="button"
              class:active={Math.abs(settings.zoom - option) < 0.0001}
              aria-pressed={Math.abs(settings.zoom - option) < 0.0001}
              onclick={() => updatePreference("zoom", option)}
            >
              {option.toFixed(1)}×
            </button>
          {/each}
        </div>
      </div>
      <label class="setting-select">
        <span>Layout</span>
        <select value={settings.flow} onchange={(event) => updatePreference("flow", event.currentTarget.value as ReaderFlow)}>
          {#each READER_PREFERENCES_CONTRACT.options.flows as option}<option value={option.value}>{option.label}</option>{/each}
        </select>
      </label>
      <label class="setting-select">
        <span>Typeface</span>
        <select value={settings.font_family} onchange={(event) => updatePreference("font_family", event.currentTarget.value as ReaderFontFamily)}>
          {#each READER_PREFERENCES_CONTRACT.options.font_families as option}<option value={option.value}>{option.label}</option>{/each}
        </select>
      </label>
      <label class="setting-select">
        <span>Alignment</span>
        <select value={settings.text_alignment} onchange={(event) => updatePreference("text_alignment", event.currentTarget.value as ReaderTextAlignment)}>
          {#each READER_PREFERENCES_CONTRACT.options.text_alignments as option}<option value={option.value}>{option.label}</option>{/each}
        </select>
      </label>
      <div class="setting-option-row">
        <span>Maximum columns</span>
        <div class="setting-button-group">
          {#each maxColumnCountOptions as option}
            <button
              type="button"
              class:active={settings.max_column_count === option}
              aria-pressed={settings.max_column_count === option}
              onclick={() => updatePreference("max_column_count", option)}
            >
              {option}
            </button>
          {/each}
        </div>
      </div>
      <div class="setting-option-row">
        <span>Line height</span>
        <div class="setting-button-group">
          {#each lineHeightOptions as option}
            <button
              type="button"
              class:active={Math.abs(settings.line_height - option) < 0.0001}
              aria-pressed={Math.abs(settings.line_height - option) < 0.0001}
              onclick={() => updatePreference("line_height", option)}
            >
              {option.toFixed(1)}
            </button>
          {/each}
        </div>
      </div>
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

  {#if selectionContextMenu && selection}
    <div
      class="selection-context-menu"
      role="menu"
      aria-label="Selected text actions"
      style={`left: ${selectionContextMenu.x}px; top: ${selectionContextMenu.y}px;`}
    >
      <button role="menuitem" type="button" onclick={() => void saveSelectionAndOpenAnnotation()}>Highlight</button>
      <button role="menuitem" type="button" onclick={() => void copySelectionCitation()}>Cite</button>
    </div>
  {/if}

  {#if activeAnnotation}
    <section class="annotation-popup" aria-label="Annotation">
      <div class="panel-heading">
        <strong>Annotation</strong>
        <button type="button" aria-label="Close annotation" onclick={() => (activeAnnotation = null)}>×</button>
      </div>
      <q>{activeAnnotation.exact_text}</q>
      <textarea bind:value={annotationNoteDraft} placeholder="Add a note" aria-label="Annotation note"></textarea>
      <div class="annotation-popup-actions">
        <button type="button" onclick={() => void saveActiveAnnotation()}>Save</button>
        <button type="button" onclick={() => { const annotation = activeAnnotation; activeAnnotation = null; if (annotation) void removeAnnotation(annotation); }}>Delete</button>
      </div>
    </section>
  {/if}

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

  {#if imageViewerSrc}
    <dialog open class="modal-backdrop" aria-label="Image viewer" onclick={(event) => { if (event.target === event.currentTarget) imageViewerSrc = ""; }}>
      <section class="image-viewer"><div class="panel-heading"><strong>Image</strong><button type="button" aria-label="Close image viewer" onclick={() => (imageViewerSrc = "")}>×</button></div><img src={imageViewerSrc} alt="Expanded book illustration" /><a href={imageViewerSrc} download>Save image</a></section>
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
    background: var(--color-app-bg);
    position: relative;
    transition: grid-template-rows 160ms ease;
  }

  .reader-shell.hidden-chrome { grid-template-rows: 0 minmax(0, 1fr); }

  .reader-header {
    position: relative;
    z-index: 2;
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: thin;
    box-sizing: border-box;
    background: var(--chrome);
  }

  .reader-header { transition: opacity 160ms ease; }

  .hidden-chrome .reader-header { height: 0; min-height: 0; opacity: 0; pointer-events: none; }

  @media (prefers-reduced-motion: reduce) {
    .reader-shell { transition: none; }
    .reader-header { transition: none; }
  }

  .reader-header {
    height: calc(58px + env(safe-area-inset-top));
    min-height: calc(58px + env(safe-area-inset-top));
    padding: env(safe-area-inset-top) max(14px, env(safe-area-inset-right)) 0 max(14px, env(safe-area-inset-left));
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

  .reader-control {
    flex: none;
    min-width: 0;
    width: auto;
    height: 34px;
    min-height: 34px;
    padding: 0 10px;
    border: 1px solid var(--chrome-strong);
    border-radius: 8px;
    background: var(--color-white-surface);
    font-size: 0.875rem;
    white-space: nowrap;
  }

  .reader-control.square {
    width: 34px;
    padding: 0;
    font-size: 1.125rem;
  }

  .reader-control:disabled {
    opacity: 0.4;
  }

  .selection-context-menu {
    position: fixed;
    z-index: 20;
    display: grid;
    min-width: 162px;
    padding: 6px;
    border: 1px solid var(--chrome-strong);
    border-radius: 10px;
    color: var(--ink);
    background: var(--chrome);
    box-shadow: 0 10px 32px color-mix(in srgb, var(--color-ink-text) 26%, transparent);
  }

  .selection-context-menu button {
    min-height: 36px;
    padding: 0 10px;
    text-align: left;
    border-radius: 6px;
  }

  .selection-context-menu button:hover,
  .selection-context-menu button:focus-visible {
    outline: none;
    background: var(--chrome-strong);
  }

  .annotation-popup {
    position: absolute;
    z-index: 8;
    top: calc(58px + env(safe-area-inset-top) + .75rem);
    right: max(.75rem, env(safe-area-inset-right));
    width: min(24rem, calc(100% - 1.5rem - env(safe-area-inset-left) - env(safe-area-inset-right)));
    max-height: calc(100dvh - 58px - env(safe-area-inset-top) - env(safe-area-inset-bottom) - 1.5rem);
    box-sizing: border-box;
    overflow: auto;
    padding: .75rem;
    border: 1px solid var(--chrome-strong);
    border-radius: .75rem;
    color: var(--ink);
    background: var(--chrome);
    box-shadow: 0 1rem 3rem color-mix(in srgb, var(--color-ink-text) 24%, transparent);
  }

  .annotation-popup q { display: block; margin-bottom: .75rem; line-height: 1.45; }
  .annotation-popup textarea { width: 100%; min-height: 7rem; box-sizing: border-box; resize: vertical; border: 1px solid var(--chrome-strong); border-radius: .55rem; padding: .65rem; color: inherit; background: var(--color-white-surface); font: inherit; }
  .annotation-popup-actions { display: flex; gap: .5rem; margin-top: .65rem; }
  .annotation-popup-actions button { border: 1px solid var(--chrome-strong); background: var(--color-white-surface); }

  .book-heading {
    min-width: 9rem;
    flex: 1;
    display: grid;
    padding-inline: 5px;
  }

  .book-heading strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .book-heading strong {
    font-family: var(--font-interface-family);
    font-size: 1rem;
    font-weight: 600;
  }

  .viewport {
    position: relative;
    min-height: 0;
    transition: margin 120ms ease;
  }

  .reader-shell.has-sidebar .viewport {
    margin-left: min(380px, 40vw);
  }

  .reader-shell.search-active .viewport {
    margin-bottom: calc(52px + env(safe-area-inset-bottom));
  }

  .reader-panel {
    position: absolute;
    z-index: 6;
    top: calc(58px + env(safe-area-inset-top));
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
    left: 0;
    bottom: 0;
    width: min(380px, 40vw);
    max-height: none;
    border-width: 0 1px 0 0;
    border-radius: 0;
    box-shadow: none;
  }

  .notes-panel,
  .info-panel {
    left: 0;
    right: auto;
    bottom: 0;
    width: min(380px, 40vw);
    max-height: none;
    border-width: 0 1px 0 0;
    border-radius: 0;
    box-shadow: none;
  }

  .settings-panel {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, var(--reader-settings-min-column-width)), 1fr));
    gap: var(--reader-settings-gap);
    width: min(var(--reader-settings-max-width), calc(100% - 1.3rem - env(safe-area-inset-left) - env(safe-area-inset-right)));
    max-height: min(var(--reader-settings-max-height), calc(100dvh - 58px - env(safe-area-inset-top)));
    padding: var(--reader-settings-padding);
  }

  .reader-search-bar {
    position: absolute;
    z-index: 7;
    right: 0;
    bottom: 0;
    left: 0;
    min-width: 0;
    min-height: calc(52px + env(safe-area-inset-bottom));
    padding: 8px 12px calc(8px + env(safe-area-inset-bottom));
    box-sizing: border-box;
    overflow-x: auto;
    border-top: 1px solid var(--chrome-strong);
    background: var(--chrome);
  }

  .reader-search-bar .search-form {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: max-content;
  }

  .reader-search-bar .search-form input {
    width: 260px;
    height: 36px;
    min-height: 36px;
    padding: 0 10px;
    border: 1px solid var(--chrome-strong);
    border-radius: 8px;
    color: inherit;
    background: var(--color-white-surface);
    font: inherit;
  }

  .reader-search-bar button {
    width: auto;
    min-width: 0;
    height: 34px;
    min-height: 34px;
    padding: 0 10px;
    border: 1px solid var(--chrome-strong);
    border-radius: 8px;
    background: var(--color-white-surface);
    white-space: nowrap;
  }

  .reader-search-bar button.square {
    width: 34px;
    padding: 0;
    font-size: 1.125rem;
  }

  .reader-match-count {
    min-width: 42px;
    color: var(--color-subtle-text);
    font-size: 0.8125rem;
  }

  .search-inline-error {
    margin-left: 8px;
    color: var(--color-danger-text);
    font-size: 0.75rem;
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

  .toc-list {
    display: grid;
    gap: 0.2rem;
  }

  .navigation-tabs {
    display: flex;
    gap: 0.35rem;
    overflow-x: auto;
  }

  .navigation-tabs button.active { background: var(--chrome-strong); }

  .info-panel dl {
    display: grid;
    grid-template-columns: minmax(5rem, auto) minmax(0, 1fr);
    gap: 0.45rem 0.75rem;
    margin: 0;
  }

  .info-panel dt { font-weight: 700; }
  .info-panel dd { margin: 0; overflow-wrap: anywhere; }

  .image-viewer {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    gap: 0.75rem;
    width: min(94vw, 70rem);
    height: min(90dvh, 60rem);
    padding: 0.75rem;
    border-radius: 0.9rem;
    background: var(--chrome);
  }

  .image-viewer img { width: 100%; height: 100%; min-height: 0; object-fit: contain; }

  .toc-row {
    display: grid;
    grid-template-columns: 2rem minmax(0, 1fr);
    align-items: center;
    min-height: 2.75rem;
    padding-inline-start: calc(var(--toc-depth) * 1rem);
    border-radius: 0.7rem;
  }

  .toc-row.current {
    background: var(--chrome-strong);
  }

  .toc-disclosure {
    min-width: 2rem;
    min-height: 2.5rem;
    padding: 0;
    font-size: 1rem;
  }

  .toc-disclosure-spacer {
    width: 2rem;
  }

  .toc-target {
    justify-content: flex-start;
    min-width: 0;
    min-height: 2.75rem;
    padding: 0.55rem 0.7rem 0.55rem 0.2rem;
    text-align: start;
    font-weight: 600;
  }

  .toc-target:disabled {
    opacity: 0.55;
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

  .setting-option-row {
    grid-column: 1 / -1;
    display: grid;
    gap: 0.35rem;
    font-size: 0.78rem;
    font-weight: 700;
  }

  .setting-button-group {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(3.5rem, 1fr));
    gap: 1px;
    border: 1px solid var(--chrome-strong);
    border-radius: var(--radius-sm);
    overflow: hidden;
    background: var(--chrome-strong);
  }

  .setting-button-group button {
    min-height: 34px;
    padding: 0.45rem 0.4rem;
    border: 0;
    border-radius: 0;
    background: var(--color-white-surface);
    color: var(--color-ink-text);
    font-size: 0.74rem;
    font-weight: 700;
    cursor: pointer;
    white-space: nowrap;
  }

  .setting-button-group button.active {
    color: var(--color-white-surface);
    background: var(--color-accent);
  }

  .setting-select {
    display: grid;
    min-width: 0;
    gap: 0.3rem;
    font-size: 0.75rem;
    font-weight: 700;
  }

  .setting-select select {
    min-width: 0;
    height: 2.4rem;
    padding: 0 0.55rem;
    border: 1px solid var(--chrome-strong);
    border-radius: 0.55rem;
    color: inherit;
    background: var(--chrome);
    font: inherit;
  }

  .view-host,
  :global(.foliate-view) {
    display: block;
    width: 100%;
    height: 100%;
  }

  .notes-actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.45rem;
  }

  .notes-panel > input {
    width: 100%;
    min-height: 2.5rem;
  }

  .annotation-list,
  .bookmark-list {
    display: grid;
    gap: 0.4rem;
    max-height: 42dvh;
    overflow: auto;
  }

  .annotation-list article,
  .bookmark-list > div {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 0.25rem;
    border: 1px solid var(--chrome-strong);
    border-radius: 0.65rem;
  }

  .annotation-target {
    display: grid;
    grid-template-columns: 0.35rem minmax(0, 1fr);
    gap: 0.25rem 0.5rem;
    text-align: start;
  }

  .annotation-target q,
  .annotation-target span:last-child {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .annotation-target span:last-child {
    grid-column: 2;
    font-size: 0.76rem;
    opacity: 0.72;
  }

  .annotation-color {
    grid-row: 1 / span 2;
    border-radius: 1rem;
    background: var(--annotation-color);
  }

  .subheading {
    margin-top: 0.5rem;
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

  @media (max-width: 720px) {
    .reader-shell.has-sidebar .viewport {
      margin-left: 0;
    }

    .reader-panel {
      left: 0;
      right: 0;
      width: 100%;
      border-inline: 0;
      border-radius: 0;
    }

    .toc-panel,
    .notes-panel,
    .info-panel {
      width: 100%;
    }

  }
</style>
