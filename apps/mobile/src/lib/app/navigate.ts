import { goto } from "$app/navigation";
import { page } from "$app/state";

/**
 * Whether this session has pushed a history entry of its own.
 *
 * `history.length` cannot answer that: arriving at a deep link in a browser
 * leaves the previous website in the stack, so the length is greater than one
 * while there is still nowhere of ours to go back to. Popping then leaves the
 * application altogether.
 */
let pushedOwnEntry = false;

export function hasOwnHistory(): boolean {
  return pushedOwnEntry;
}

/**
 * Goes somewhere, unless already there.
 *
 * The guard matters because several things react to the URL and would
 * otherwise re-enter navigation for a route they are already on. Scroll and
 * focus are preserved: the shell moves between pages of one application, not
 * between documents.
 */
export function navigate(path: string, replaceState = false): Promise<void> {
  if (`${page.url.pathname}${page.url.search}` === path) return Promise.resolve();
  // Replacing overwrites the current entry, so it adds nothing to go back to.
  if (!replaceState) pushedOwnEntry = true;
  return goto(path, { replaceState, noScroll: true, keepFocus: true });
}
