import type { FoliateSearchGroup, FoliateView } from "../../../../lib/shared/types";
import type { SearchResult } from "./reader-types";

export interface ReaderSearchOptions {
  getView: () => FoliateView | null;
  /// Label for results the engine reports without a section of their own.
  getCurrentSectionLabel: () => string;
}

/**
 * Runs in-book search and tracks which result is selected.
 *
 * Foliate streams results, so a search started while another is still running
 * would interleave two result sets. Every run takes a generation number and a
 * run whose generation is stale drops its remaining output, which is also how
 * clearing mid-search works.
 */
export function createReaderSearch({
  getView,
  getCurrentSectionLabel,
}: ReaderSearchOptions) {
  let query = $state("");
  let results = $state<SearchResult[]>([]);
  let progress = $state(0);
  let running = $state(false);
  let error = $state("");
  let selectedIndex = $state(-1);
  let generation = 0;

  async function run() {
    const trimmed = query.trim();
    const view = getView();
    const current = ++generation;
    results = [];
    progress = 0;
    error = "";
    selectedIndex = -1;
    view?.clearSearch();
    if (!trimmed || !view) return;

    running = true;
    try {
      for await (const result of view.search({
        query: trimmed,
        index: undefined,
        matchCase: false,
        matchDiacritics: false,
        matchWholeWords: false,
      })) {
        if (current !== generation) return;
        if (typeof result === "string") continue;
        if ("progress" in result) {
          progress = result.progress;
          continue;
        }
        if ("subitems" in result) {
          const group = result as FoliateSearchGroup;
          results = [
            ...results,
            ...group.subitems.map((match) => ({
              ...match,
              section: group.label || "Book",
            })),
          ];
        } else if ("cfi" in result) {
          results = [...results, { ...result, section: getCurrentSectionLabel() }];
        }
      }
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (current === generation) {
        running = false;
        progress = 1;
      }
    }
  }

  async function select(result: SearchResult, index: number) {
    selectedIndex = index;
    await getView()?.select(result.cfi);
  }

  /// Steps through results, wrapping at both ends.
  function cycle(offset: number) {
    if (!results.length) return;
    const index =
      selectedIndex < 0
        ? offset > 0
          ? 0
          : results.length - 1
        : (selectedIndex + offset + results.length) % results.length;
    void select(results[index], index);
  }

  function clear() {
    generation += 1;
    running = false;
    results = [];
    progress = 0;
    error = "";
    selectedIndex = -1;
    getView()?.clearSearch();
  }

  return {
    get query() {
      return query;
    },
    set query(value: string) {
      query = value;
    },
    get results() {
      return results;
    },
    get progress() {
      return progress;
    },
    get running() {
      return running;
    },
    get error() {
      return error;
    },
    get selectedIndex() {
      return selectedIndex;
    },
    run,
    select,
    cycle,
    clear,
  };
}
