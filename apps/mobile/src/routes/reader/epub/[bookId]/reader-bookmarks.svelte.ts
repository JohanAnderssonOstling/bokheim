import type { ReaderBookmark } from "../../../../lib/shared/types";

export interface ReaderBookmarksOptions {
  getBookId: () => string | undefined;
  /// The CFI the reader is currently showing, or "" before the first relocate.
  getCurrentCfi: () => string;
  /// Human-readable name for the current place, used as the bookmark label.
  getCurrentLabel: () => string;
  canCreate: () => boolean;
  canDelete: () => boolean;
  canLoad: () => boolean;
  createBookmark: (bookmark: ReaderBookmark) => Promise<void> | void;
  deleteBookmark: (id: string, at: number) => Promise<void> | void;
  loadBookmarks: () => Promise<ReaderBookmark[]>;
  readerState: { error: string };
}

/**
 * Keeps the bookmark list for the open publication.
 *
 * Bookmarks are a separate entity from annotations: they mark a place rather
 * than a passage, so they carry no overlay and never touch the foliate view.
 */
export function createReaderBookmarks(options: ReaderBookmarksOptions) {
  let bookmarks = $state<ReaderBookmark[]>([]);

  /// Loads stored bookmarks; `isStale` lets a closing reader drop late results.
  async function restore(isStale: () => boolean) {
    if (!options.canLoad()) return;
    try {
      const loaded = await options.loadBookmarks();
      if (!isStale()) bookmarks = loaded;
    } catch (reason) {
      if (!isStale())
        options.readerState.error = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function add() {
    const cfi = options.getCurrentCfi();
    const bookId = options.getBookId();
    if (!cfi || !bookId || !options.canCreate()) return;
    const now = Date.now();
    const bookmark: ReaderBookmark = {
      id: crypto.randomUUID(),
      content_hash: bookId,
      cfi,
      label: options.getCurrentLabel(),
      created_at: now,
      modified_at: now,
    };
    await options.createBookmark(bookmark);
    bookmarks = [bookmark, ...bookmarks];
  }

  async function remove(bookmark: ReaderBookmark) {
    if (!options.canDelete()) return;
    await options.deleteBookmark(bookmark.id, Date.now());
    bookmarks = bookmarks.filter((item) => item.id !== bookmark.id);
  }

  return {
    get items() {
      return bookmarks;
    },
    restore,
    add,
    remove,
  };
}
