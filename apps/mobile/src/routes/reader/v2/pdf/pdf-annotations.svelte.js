import { createAnnotationStore } from "../shared/annotation-store.svelte";
import { pageLocator } from "./pdf-locator";

const DEFAULT_ANNOTATION_COLOR = "#f6c945";

/**
 * The annotations of one open PDF.
 *
 * A PDF anchor is a page index plus fractional rects rather than a CFI, so
 * `draft()` is its own here rather than shared with the EPUB store — the two
 * anchor shapes need different code at every step that touches them
 * (drafting, drawing, hit-testing), and a parameter picking between them
 * would just move the fork inside the function instead of removing it. The
 * restore/save/remove core underneath doesn't care about the anchor shape at
 * all, so that part is shared via `createAnnotationStore`.
 */
export function createPdfAnnotations(libraryId, bookId) {
  let store = createAnnotationStore(libraryId, bookId);

  /// Builds an annotation for a page selection without storing it.
  function draft(pageIndex, rects, text) {
    let now = Date.now();
    return {
      id: crypto.randomUUID(),
      content_hash: bookId,
      anchor: {
        kind: "pdf",
        page_index: pageIndex,
        rects,
        fallback_cfi: pageLocator(pageIndex),
      },
      exact_text: text,
      style: "highlight",
      color: DEFAULT_ANNOTATION_COLOR,
      note: "",
      created_at: now,
      modified_at: now,
    };
  }

  return {
    get items() {
      return store.items;
    },
    restore: store.restore,
    draft,
    save: store.save,
    remove: store.remove,
  };
}
