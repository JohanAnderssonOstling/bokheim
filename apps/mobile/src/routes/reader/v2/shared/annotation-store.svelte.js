import { commands } from "../../../../lib/app/commands";

/**
 * The persisted CRUD core shared by every reader's annotation store: an
 * annotation is stored the same way regardless of what its anchor looks
 * like, so restoring, saving and removing it don't need to know the format.
 * Each reader wraps this with its own `draft()`.
 */
export function createAnnotationStore(libraryId, bookId) {
  let items = $state([]);

  async function restore() {
    try {
      items = await commands.getAnnotations(libraryId, bookId);
    } catch {
      // An unreadable list is empty, not fatal: the book is still readable.
    }
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
    save,
    remove,
  };
}
