import type { CatalogBookFormat, CatalogPublicationAction } from "../../../lib/bindings";

type CatalogOwnershipStatus = "not_owned" | "exact_edition_owned" | "other_edition_owned";
export type CatalogImportAction = Extract<CatalogPublicationAction, { kind: "import" }>;

const formatLabel = (format: CatalogBookFormat) => format.toUpperCase();

export function catalogActionKey(action: CatalogPublicationAction) {
  return action.kind === "import"
    ? `import:${action.request.source_id}:${action.request.publication.edition_id}:${action.request.book_format}:${action.request.download_url}`
    : `external:${action.purpose}:${action.url}`;
}

/** Formats the browser can read for itself; native delegates to the platform. */
const BROWSER_READABLE_FORMATS = ["epub", "pdf"];

/**
 * The action a publication's card offers.
 *
 * `supportsAllFormats` is the caller's, because which formats can be opened is
 * a property of the build rather than of the catalogue. Passing it keeps this
 * a pure selector over the actions a source advertised.
 */
export function primaryCatalogAction(
  actions: CatalogPublicationAction[],
  supportsAllFormats: boolean,
): CatalogImportAction | undefined {
  return actions.find(
    (action): action is CatalogImportAction =>
      action.kind === "import" &&
      (supportsAllFormats ||
        BROWSER_READABLE_FORMATS.includes(action.request.book_format)),
  );
}

export function catalogActionLabel(
  action: CatalogPublicationAction,
  ownership: CatalogOwnershipStatus,
  libraryName: string,
) {
  if (action.kind === "import") {
    const format = formatLabel(action.request.book_format);
    if (ownership === "exact_edition_owned") return `${format} added`;
    return ownership === "other_edition_owned"
      ? `Add this ${format} edition`
      : `Add ${format} to ${libraryName}`;
  }
  const purpose =
    action.purpose === "buy"
      ? "Buy"
      : action.purpose === "borrow"
        ? "Borrow"
        : action.purpose === "preview"
          ? "Preview"
          : action.purpose === "subscribe"
            ? "Subscribe"
            : "Open";
  const price = action.price ? `${action.price.currency} ${action.price.value}` : "";
  const formats = action.available_formats.map(formatLabel).join(" / ");
  return [purpose, price, formats].filter(Boolean).join(" · ");
}
