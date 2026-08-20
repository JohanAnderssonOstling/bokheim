import type { ViewportRect } from "../../../../components/shared/readers/annotations/annotation-placement";
import { rangeViewportRect } from "./annotation-placement";

export interface ReaderSelection {
  cfi: string;
  text: string;
}

export interface ReaderSelectionOptions {
  /// Resolves the CFI for a range in the section at `index`.
  getCfi: (index: number, range: Range) => string;
  getAttribution: () => string;
}

/**
 * Tracks the passage the user has selected and what can be done with it.
 *
 * A selection is not an annotation: it is transient, exists before anything is
 * saved, and is also how an existing annotation is re-targeted when its overlay
 * is tapped.
 */
export function createReaderSelection(options: ReaderSelectionOptions) {
  let selection = $state<ReaderSelection | null>(null);
  let note = $state("");
  let contextMenu = $state<{ x: number; y: number } | null>(null);
  let anchorRect = $state<ViewportRect | null>(null);

  /// Reads the live selection out of a section document, if there is one.
  function capture(doc: Document, index: number) {
    const selected = doc.getSelection();
    if (!selected?.rangeCount || selected.isCollapsed) return;
    const range = selected.getRangeAt(0).cloneRange();
    const text = selected.toString().trim();
    if (!text) return;
    selection = { cfi: options.getCfi(index, range), text };
    anchorRect = rangeViewportRect(range, doc);
    note = "";
  }

  /// Points the selection at an existing annotation's text, e.g. on overlay tap.
  function adoptAnnotation(cfi: string, text: string, annotationNote: string) {
    selection = { cfi, text };
    note = annotationNote;
  }

  async function copyCitation() {
    if (!selection) return;
    contextMenu = null;
    const locator = selection.cfi ? ` (${selection.cfi})` : "";
    await navigator.clipboard.writeText(
      `“${selection.text}”\n\n${options.getAttribution()}${locator}`,
    );
  }

  return {
    get current() {
      return selection;
    },
    get text() {
      return selection?.text ?? "";
    },
    get note() {
      return note;
    },
    get contextMenu() {
      return contextMenu;
    },
    set contextMenu(position: { x: number; y: number } | null) {
      contextMenu = position;
    },
    /// Where the selection sits on screen, used to anchor the note popup.
    get anchorRect() {
      return anchorRect;
    },
    set anchorRect(rect: ViewportRect | null) {
      anchorRect = rect;
    },
    capture,
    adoptAnnotation,
    copyCitation,
    clear: () => (selection = null),
    closeContextMenu: () => (contextMenu = null),
  };
}
