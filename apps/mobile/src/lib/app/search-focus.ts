const EVENT = "bokheim:focus-search";

/**
 * How long a focus request stays honourable.
 *
 * A request made from a page without a search field navigates to one, so the
 * field mounts a moment later and claims it. Past this window the navigation
 * clearly went somewhere else, and stealing focus then would be a surprise.
 */
const REQUEST_TTL_MS = 2000;

let requestedAt = 0;

/**
 * Asks the contextual search field for focus, wherever it is.
 *
 * The shell owns the keyboard shortcut but not the field, so it announces the
 * request rather than reaching into the page for an input. A field that is
 * already mounted hears the event; one that mounts afterwards claims the
 * pending request instead, which is what makes this work across a navigation.
 */
export function requestSearchFocus() {
  requestedAt = Date.now();
  window.dispatchEvent(new Event(EVENT));
}

export function onSearchFocus(handler: () => void) {
  const listener = () => {
    requestedAt = 0;
    handler();
  };
  window.addEventListener(EVENT, listener);
  return () => window.removeEventListener(EVENT, listener);
}

/** Claims a request made before this field existed. */
export function consumeSearchFocus(): boolean {
  const pending = requestedAt !== 0 && Date.now() - requestedAt < REQUEST_TTL_MS;
  requestedAt = 0;
  return pending;
}
