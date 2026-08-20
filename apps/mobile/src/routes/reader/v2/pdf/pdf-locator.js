/// Encodes a PDF page index as the opaque locator string bookmarks and the
/// annotation `fallback_cfi` store it under. Duplicated from the v1 PDF
/// reader's `pageLocator`/`pageFromLocator` rather than imported, so the two
/// readers stay free to diverge; the wire format is kept identical on purpose
/// so a bookmark made in one reader still resolves in the other.
export function pageLocator(index) {
  return `pdf-page:${Math.max(0, index)}`;
}

export function pageFromLocator(locator) {
  if (!locator) return 0;
  let direct = /^pdf-page:(\d+)$/.exec(locator);
  return direct ? Number(direct[1]) : 0;
}
