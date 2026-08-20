import { commands } from "../../../lib/app/commands";
import { Overlayer } from "../../../vendor/foliate-js/overlayer.js";

export const DEFAULT_ANNOTATION_COLOR = "#f6c945";

/**
 * The annotations of one open book.
 *
 * Created by the reader, so it lives and dies with the book: a second reader
 * gets its own. Both the highlights drawn in the text and the list in the
 * sidebar read `items`, so there is one copy and they cannot disagree.
 *
 * Writes apply locally first and then store; a failed write leaves the reader
 * with the annotation it just made rather than an error it cannot act on.
 */
export function createAnnotations(libraryId, bookId) {
  let items = $state([]);

  async function restore() {
    try {
      items = await commands.getAnnotations(libraryId, bookId);
    } catch {
      // An unreadable list is empty, not fatal: the book is still readable.
    }
  }

  /// Builds an annotation for a selection without storing it.
  function draft(cfi, text) {
    let now = Date.now();
    return {
      id: crypto.randomUUID(),
      content_hash: bookId,
      anchor: { kind: "epub_cfi", cfi },
      exact_text: text,
      style: "highlight",
      color: DEFAULT_ANNOTATION_COLOR,
      note: "",
      created_at: now,
      modified_at: now,
    };
  }

  async function save(annotation) {
    let stored = { ...annotation, modified_at: Date.now() };
    items = [...items.filter((item) => item.id !== stored.id), stored];
    try {
      await commands.upsertAnnotation(libraryId, stored);
    } catch {
      // Kept locally; the next open reads whatever did store.
    }
    return stored;
  }

  async function remove(annotation) {
    items = items.filter((item) => item.id !== annotation.id);
    try {
      await commands.deleteAnnotation(libraryId, annotation.id, Date.now());
    } catch {
      // Gone from view either way; the next open reads the stored truth.
    }
  }

  return {
    get items() {
      return items;
    },
    restore,
    draft,
    save,
    remove,
  };
}

/// Foliate's own annotation shape. Only CFI-anchored ones can be drawn.
export function overlayOf(annotation) {
  let cfi = annotation.anchor.kind === "epub_cfi" ? annotation.anchor.cfi : "";
  if (!cfi.startsWith("epubcfi(")) return null;
  return { value: cfi, color: annotation.color, style: annotation.style, id: annotation.id };
}

/**
 * Paints one highlight. Foliate asks for this per annotation per section, so
 * the shape is chosen here rather than stored.
 */
export function drawOverlay(event) {
  let { draw, annotation, doc } = event.detail;
  let style =
    annotation.style === "underline"
      ? Overlayer.underline
      : annotation.style === "squiggly"
        ? Overlayer.squiggly
        : annotation.style === "strikethrough"
          ? Overlayer.strikethrough
          : Overlayer.highlight;
  draw(style, {
    color: annotation.color || DEFAULT_ANNOTATION_COLOR,
    writingMode: getComputedStyle(doc.documentElement).writingMode,
  });
}
