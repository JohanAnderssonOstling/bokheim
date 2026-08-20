import { FootnoteHandler } from "../../../../vendor/foliate-js/footnotes.js";

/**
 * Opens footnotes without leaving the page.
 *
 * Foliate renders a note as a second, self-contained `<foliate-view>` that it
 * creates itself, so the reader's styles have to be applied twice: once before
 * it renders, so the note never paints with default styling, and again once its
 * renderer exists.
 *
 * The rendered view is held as state rather than written into a host element,
 * because `handle` must run synchronously inside the click — it calls
 * `preventDefault` to stop foliate navigating — and the popup that will hold it
 * has not been mounted at that point.
 */
export function createFootnotes(applyStyles) {
  let handler = new FootnoteHandler();
  let element = $state(null);

  handler.addEventListener("before-render", function (event) {
    applyStyles(event.detail.view.renderer);
  });

  handler.addEventListener("render", function (event) {
    element = event.detail.view;
    applyStyles(element.renderer);
  });

  return {
    get element() {
      return element;
    },

    /// Returns a promise when the link really was a footnote reference.
    open(book, event) {
      element = null;
      return handler.handle(book, event);
    },

    close() {
      element?.close?.();
      element?.remove?.();
      element = null;
    },
  };
}
