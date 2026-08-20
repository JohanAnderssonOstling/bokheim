import { Overlayer } from "../../../../vendor/foliate-js/overlayer.js";
import type {
  FoliateView,
  ReaderAnnotation,
  ReaderAnnotationStyle,
} from "../../../../lib/shared/types";
import { annotationCfi, annotationOverlay } from "./reader-helpers";

/// Used when an annotation carries no colour of its own.
export const DEFAULT_ANNOTATION_COLOR = "#f6c945";

export interface ReaderAnnotationStoreOptions {
  getView: () => FoliateView | null;
  getBookId: () => string | undefined;
  canUpsert: () => boolean;
  canDelete: () => boolean;
  canLoad: () => boolean;
  upsertAnnotation: (annotation: ReaderAnnotation) => Promise<void> | void;
  deleteAnnotation: (id: string, at: number) => Promise<void> | void;
  loadAnnotations: () => Promise<ReaderAnnotation[]>;
  readerState: { error: string };
}

/**
 * Owns the annotation list and keeps the drawn overlays in step with it.
 *
 * Every change has to land in two places — the backend and the overlay foliate
 * paints over the text — and they must not drift apart, so both are done here
 * rather than by each caller.
 */
export function createReaderAnnotationStore(options: ReaderAnnotationStoreOptions) {
  let annotations = $state<ReaderAnnotation[]>([]);

  async function addOverlay(annotation: ReaderAnnotation) {
    const overlay = annotationOverlay(annotation);
    if (overlay) await options.getView()?.addAnnotation(overlay);
  }

  async function removeOverlay(annotation: ReaderAnnotation) {
    const overlay = annotationOverlay(annotation);
    if (overlay) await options.getView()?.deleteAnnotation(overlay);
  }

  async function restore() {
    if (!options.canLoad()) return;
    try {
      annotations = await options.loadAnnotations();
      for (const annotation of annotations) await addOverlay(annotation);
    } catch (reason) {
      options.readerState.error = reason instanceof Error ? reason.message : String(reason);
    }
  }

  /**
   * Redraws every overlay into a freshly created one.
   *
   * Foliate discards overlays with the section they were drawn in, so each new
   * overlay starts empty and has to be repopulated.
   */
  function redrawOverlays() {
    for (const annotation of annotations) void addOverlay(annotation);
  }

  /**
   * Saves a highlight over the given text, replacing any annotation already
   * anchored there so a second highlight of one passage does not stack up.
   */
  async function saveSelection(
    selection: { cfi: string; text: string },
    note: string,
    style: ReaderAnnotationStyle = "highlight",
  ): Promise<ReaderAnnotation | undefined> {
    const bookId = options.getBookId();
    if (!bookId || !options.canUpsert()) return;
    const anchor = { kind: "epub_cfi" as const, cfi: selection.cfi };
    const existing = annotations.find(
      (item) =>
        JSON.stringify(item.anchor) === JSON.stringify(anchor) ||
        annotationCfi(item) === selection.cfi,
    );
    const now = Date.now();
    const annotation: ReaderAnnotation = {
      id: existing?.id || crypto.randomUUID(),
      content_hash: bookId,
      anchor,
      exact_text: selection.text,
      style: existing?.style || style,
      color: existing?.color || DEFAULT_ANNOTATION_COLOR,
      note,
      created_at: existing?.created_at || now,
      modified_at: now,
    };
    try {
      await options.upsertAnnotation(annotation);
      if (existing) await removeOverlay(existing);
      annotations = [
        annotation,
        ...annotations.filter((item) => item.id !== annotation.id),
      ];
      await addOverlay(annotation);
      return annotation;
    } catch (reason) {
      options.readerState.error = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function remove(annotation: ReaderAnnotation) {
    if (!options.canDelete()) return;
    await options.deleteAnnotation(annotation.id, Date.now());
    await removeOverlay(annotation);
    annotations = annotations.filter((item) => item.id !== annotation.id);
  }

  function find(cfi: string) {
    return annotations.find((item) => annotationCfi(item) === cfi);
  }

  return {
    get items() {
      return annotations;
    },
    set items(next: ReaderAnnotation[]) {
      annotations = next;
    },
    restore,
    redrawOverlays,
    saveSelection,
    remove,
    find,
    addOverlay,
    removeOverlay,
  };
}

/// Maps an annotation's style to the overlay stroke foliate should paint.
export function drawAnnotationOverlay(event: Event) {
  const { draw, annotation, doc } = (
    event as CustomEvent<{
      draw: (renderer: unknown, options: unknown) => void;
      annotation: { style?: ReaderAnnotationStyle; color?: string };
      doc: Document;
    }>
  ).detail;
  const renderer =
    annotation.style === "underline"
      ? Overlayer.underline
      : annotation.style === "squiggly"
        ? Overlayer.squiggly
        : annotation.style === "strikethrough"
          ? Overlayer.strikethrough
          : Overlayer.highlight;
  draw(renderer, {
    color: annotation.color || DEFAULT_ANNOTATION_COLOR,
    writingMode: getComputedStyle(doc.documentElement).writingMode,
  });
}
import { getContext, setContext } from "svelte";
