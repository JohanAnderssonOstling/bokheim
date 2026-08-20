import { commands } from "../../../lib/app/commands";

/**
 * The bookmarks of one open book.
 *
 * A bookmark marks a place, not a passage: unlike annotations it carries no
 * overlay and never touches the foliate view. Created by the reader, so it
 * lives and dies with the book like `createAnnotations` does.
 */
export function createBookmarks(libraryId, bookId) {
  let items = $state([]);

  async function restore() {
    try {
      items = await commands.getBookmarks(libraryId, bookId);
    } catch {
      // An unreadable list is empty, not fatal: the book is still readable.
    }
  }

  async function add(cfi, label) {
    if (!cfi) return;
    let now = Date.now();
    let bookmark = {
      id: crypto.randomUUID(),
      content_hash: bookId,
      cfi,
      label,
      created_at: now,
      modified_at: now,
    };
    items = [bookmark, ...items];
    try {
      await commands.createBookmark(libraryId, bookmark);
    } catch {
      // Kept locally; the next open reads whatever did store.
    }
  }

  async function remove(bookmark) {
    items = items.filter((item) => item.id !== bookmark.id);
    try {
      await commands.deleteBookmark(libraryId, bookmark.id, Date.now());
    } catch {
      // Gone from view either way; the next open reads the stored truth.
    }
  }

  return {
    get items() {
      return items;
    },
    restore,
    add,
    remove,
  };
}
