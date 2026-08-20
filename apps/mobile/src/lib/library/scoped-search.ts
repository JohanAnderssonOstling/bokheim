import { BROWSING_CONTENT_CONTRACT } from "../shared/types";
import type { LibrarySearchResult } from "../shared/types";

export interface ScopedSearchHandlers {
  /** The query became empty: there is nothing to show and nothing pending. */
  cleared: () => void;
  pending: () => void;
  results: (query: string, results: LibrarySearchResult) => void;
  failed: (reason: unknown) => void;
}

/**
 * Debounced search within one part of a library.
 *
 * Folders and subjects search the same way against different commands, so the
 * timing, the cancellation and the "is this still the query being typed?" check
 * live here rather than being written out per page.
 *
 * The generation counter is this search's own, not the page's: a search must
 * not cancel the page's load, and reloading a page must not silently drop a
 * search that is already in flight.
 */
export function createScopedSearch(
  run: (query: string) => Promise<LibrarySearchResult>,
  handlers: ScopedSearchHandlers,
) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  let latest = "";

  function cancel() {
    if (timer) clearTimeout(timer);
    timer = undefined;
    generation += 1;
    latest = "";
  }

  function schedule(rawQuery: string) {
    if (timer) clearTimeout(timer);
    const query = rawQuery.trim();
    latest = query;
    if (!query) {
      generation += 1;
      handlers.cleared();
      return;
    }
    handlers.pending();
    timer = setTimeout(
      () => void execute(query),
      BROWSING_CONTENT_CONTRACT.search_debounce_ms,
    );
  }

  async function execute(query: string) {
    const current = ++generation;
    try {
      const results = await run(query);
      // Discard anything the reader has already typed past.
      if (current !== generation || query !== latest) return;
      handlers.results(query, results);
    } catch (reason) {
      if (current === generation) handlers.failed(reason);
    }
  }

  return { schedule, cancel };
}
