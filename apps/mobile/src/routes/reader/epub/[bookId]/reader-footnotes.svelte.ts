import { tick } from "svelte";
import type { FoliateRenderer, FoliateView } from "../../../../lib/shared/types";

interface FootnoteHandler {
  handle: (book: unknown, event: Event) => void | Promise<void>;
  addEventListener: EventTarget["addEventListener"];
  removeEventListener: EventTarget["removeEventListener"];
}

export interface ReaderFootnotesOptions {
  /// Applies the reader's current preferences to a footnote's own renderer.
  applyPreferences: (renderer: FoliateRenderer | undefined) => void;
}

/**
 * Opens footnotes and endnotes in their own sheet.
 *
 * Foliate renders a footnote as a second, self-contained view, so it needs the
 * reader's preferences applied separately from the publication. They are applied
 * twice: once before the view renders, so the note never paints with default
 * styling, and again once it is in the sheet, when its renderer exists.
 */
export function createReaderFootnotes({ applyPreferences }: ReaderFootnotesOptions) {
  let open = $state(false);
  let label = $state("Note");
  let host = $state<HTMLDivElement>();
  let view: FoliateView | null = null;
  let handler: FootnoteHandler | null = null;

  function prepare(event: Event) {
    const detail = (event as CustomEvent<{ view: FoliateView }>).detail;
    applyPreferences(detail.view.renderer);
  }

  async function show(event: Event) {
    const detail = (event as CustomEvent<{ view: FoliateView; type?: string }>).detail;
    close();
    view = detail.view;
    view.classList.add("footnote-view");
    label = detail.type
      ? detail.type.replace(/^./, (letter) => letter.toUpperCase())
      : "Note";
    open = true;
    await tick();
    host?.replaceChildren(view);
    applyPreferences(view.renderer);
  }

  /**
   * Loads foliate's footnote handler and starts intercepting internal links.
   *
   * The handler is imported lazily because a publication may never have a
   * footnote, and it is only useful once the book itself has opened.
   */
  async function attach() {
    const { FootnoteHandler } =
      await import("../../../../vendor/foliate-js/footnotes.js");
    handler = new FootnoteHandler();
    handler.addEventListener("before-render", prepare);
    handler.addEventListener("render", show);
  }

  /// Routes an internal link through the footnote handler, if it wants it.
  function handleInternalLink(book: unknown, event: Event) {
    if (!handler || !book) return;
    const handled = handler.handle(book, event);
    if (handled)
      Promise.resolve(handled).catch((reason) =>
        console.warn("Could not open footnote", reason),
      );
  }

  function close() {
    view?.close();
    view?.remove();
    view = null;
    open = false;
  }

  function dispose() {
    handler?.removeEventListener("before-render", prepare);
    handler?.removeEventListener("render", show);
    handler = null;
    close();
  }

  return {
    get open() {
      return open;
    },
    get label() {
      return label;
    },
    /// The renderer of the visible footnote, so preference changes reach it too.
    get renderer() {
      return view?.renderer;
    },
    setHost: (node: HTMLDivElement) => (host = node),
    attach,
    handleInternalLink,
    close,
    dispose,
  };
}
