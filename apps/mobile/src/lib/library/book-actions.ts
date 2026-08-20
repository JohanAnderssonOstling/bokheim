import { commands } from "../app/commands";
import { publishBookDownloadStatus, requestLibraryRefresh } from "../app/library-events";
import type { BookCardData } from "../shared/types";

/**
 * The actions every page offering a book offers.
 *
 * A book appears on the home page, in folders, under subjects, under an author
 * and on its own detail page, and acting on it there changes it everywhere.
 * Each action therefore announces what it did rather than only updating the
 * page that ran it; pages listen for those announcements already.
 *
 * Each returns whether it went ahead, so a caller with something further to do
 * — the detail page leaving after a book is trashed — can tell a completed
 * action from a cancelled confirmation.
 */

function title(book: BookCardData) {
  return book.title || "Untitled";
}

export async function downloadBook(libraryId: string, book: BookCardData): Promise<void> {
  publishBookDownloadStatus({ contentHash: book.content_hash, state: "Queued" });
  const state = await commands.downloadBook(libraryId, book.content_hash);
  publishBookDownloadStatus({ contentHash: book.content_hash, state });
}

export async function removeLocalCopy(
  libraryId: string,
  book: BookCardData,
): Promise<boolean> {
  if (!window.confirm(`Remove the downloaded copy of “${title(book)}” from this device?`))
    return false;
  const state = await commands.removeLocalBookCopy(libraryId, book.content_hash);
  publishBookDownloadStatus({ contentHash: book.content_hash, state });
  return true;
}

export async function trashBook(libraryId: string, book: BookCardData): Promise<boolean> {
  if (!window.confirm(`Move “${title(book)}” to Trash?`)) return false;
  await commands.moveBookToTrash(libraryId, book.content_hash);
  // The book leaves every listing at once, and Trash gains it.
  requestLibraryRefresh();
  return true;
}
