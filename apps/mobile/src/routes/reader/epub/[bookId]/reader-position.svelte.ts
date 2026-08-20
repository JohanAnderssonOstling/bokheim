import type {
  FoliateRelocateDetail,
  FoliateView,
  ReaderNavigationAction,
} from "../../../../lib/shared/types";
import { createReaderPositionAutosave } from "../../../../components/shared/readers/state/reader-position-autosave.ts";

/// Position writes are debounced; page turns arrive far faster than they persist.
const SAVE_DEBOUNCE_MS = 600;

export interface ReaderPositionOptions {
  getView: () => FoliateView | null;
  canSave: () => boolean;
  saveCfi: (cfi: string, fraction: number) => Promise<void> | void;
  /// Distance a "line" scroll should cover, derived from the current type size.
  getLineDistance: () => number;
  /// The reader moved to a new place in the book.
  onRelocate: (tocHref: string) => void;
}

/**
 * Tracks where the reader is in the book, and moves it.
 *
 * The fraction, section and location counters all come from the same foliate
 * `relocate` event and are only meaningful together, so they are updated as one
 * and persisted from one place.
 */
export function createReaderPosition(options: ReaderPositionOptions) {
  let fraction = $state(0);
  let location = $state(0);
  let section = $state(0);
  let sectionCount = $state(0);
  let activeTocHref = $state("");
  let lastCfi = $state("");

  const autosave = createReaderPositionAutosave<{ cfi: string; fraction: number }>({
    debounceMs: SAVE_DEBOUNCE_MS,
    areEqual: (left, right) => left.cfi === right.cfi && left.fraction === right.fraction,
    persist: ({ cfi, fraction: saved }) => {
      if (!options.canSave()) return;
      return options.saveCfi(cfi, saved);
    },
  });

  function handleRelocate(event: Event) {
    const detail = (event as CustomEvent<FoliateRelocateDetail>).detail;
    activeTocHref = detail.tocItem?.href || "";
    fraction = Math.max(0, Math.min(1, detail.fraction ?? fraction));
    location = detail.location?.current ?? location;
    section = detail.section?.current ?? section;
    sectionCount = detail.section?.total ?? sectionCount;
    options.onRelocate(activeTocHref);
    if (detail.cfi) {
      lastCfi = detail.cfi;
      autosave.schedule({ cfi: detail.cfi, fraction: fraction * 100 });
    }
  }

  /// Steps whole sections, clamped to the ends of the book.
  function goToSection(offset: number) {
    const view = options.getView();
    if (!view || !sectionCount) return;
    const target = Math.max(0, Math.min(sectionCount - 1, section + offset));
    if (target !== section) void view.goTo(target);
  }

  function navigate(action: ReaderNavigationAction) {
    const view = options.getView();
    if (!view) return;
    const lineDistance = options.getLineDistance();
    if (action === "previous_line") void view.prev(lineDistance);
    if (action === "next_line") void view.next(lineDistance);
    if (action === "previous_page") void view.prev();
    if (action === "next_page") void view.next();
    if (action === "previous_section") goToSection(-1);
    if (action === "next_section") goToSection(1);
    if (action === "history_back") view.history?.back();
    if (action === "history_forward") view.history?.forward();
  }

  return {
    get fraction() {
      return fraction;
    },
    get location() {
      return location;
    },
    get section() {
      return section;
    },
    get sectionCount() {
      return sectionCount;
    },
    get activeTocHref() {
      return activeTocHref;
    },
    /// The last CFI the book reported, and the anchor a new bookmark uses.
    get lastCfi() {
      return lastCfi;
    },
    handleRelocate,
    navigate,
    flush: () => autosave.flush(),
    dispose: () => autosave.dispose(),
  };
}
