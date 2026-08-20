import { readerSession } from "../../lib/app/reader-session.svelte";
import type { ReaderBook } from "../../lib/shared/types";

/**
 * Opens the book a reader route is showing, and closes it again on the way out.
 *
 * Both reader routes need the same lifecycle around a different renderer, so it
 * lives here rather than being written twice. The open is cancellable: a second
 * navigation while the first book is still opening must not publish it, and the
 * file it opened has to be released rather than leaked.
 */
export function createReaderRoute() {
  let book = $state<ReaderBook | null>(null);
  let error = $state("");
  let request = 0;

  async function open(libraryId: string, bookId: string, target?: string) {
    const current = ++request;
    const previous = book;
    book = null;
    error = "";
    void readerSession.closeForRoute(previous);
    if (!bookId) return;
    try {
      const opened = await readerSession.openForRoute(libraryId, bookId, target);
      if (current !== request) {
        await readerSession.closeForRoute(opened);
        return;
      }
      book = opened;
    } catch (reason) {
      if (current !== request) return;
      error = reason instanceof Error ? reason.message : String(reason);
    }
  }

  function close() {
    request += 1;
    void readerSession.closeForRoute(book);
    book = null;
  }

  return {
    get book() {
      return book;
    },
    get error() {
      return error;
    },
    open,
    close,
  };
}
