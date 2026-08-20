import { goto } from "$app/navigation";
import { libraryRootHref } from "../../lib/app/navigation";

/**
 * Leaves the reader the way the reader arrived.
 *
 * Opening a book pushes the reader URL, so the browser's own history is the
 * return path — no separate record of where the reader came from is needed.
 * Only a reader reached directly, with nothing behind it, needs a destination.
 */
export function closeReader(libraryId?: string) {
  if (history.length > 1) {
    history.back();
    return;
  }
  void goto(libraryId ? libraryRootHref(libraryId) : "/library", {
    replaceState: true,
  });
}
