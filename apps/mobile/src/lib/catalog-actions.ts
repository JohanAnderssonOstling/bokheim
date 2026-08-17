import type { CatalogBookFormat, CatalogPublicationAction } from "./bindings";
import { isNativeApp } from "./platform";

type CatalogOwnershipStatus = "not_owned" | "exact_edition_owned" | "other_edition_owned";

const formatLabel = (format: CatalogBookFormat) => format === "web_pub" ? "WebPub" : format.toUpperCase();

export function catalogActionKey(action: CatalogPublicationAction) {
  return action.kind === "import" ? `import:${action.request.source_id}:${action.request.publication.edition_id}:${action.request.book_format}:${action.request.download_url}` : `external:${action.purpose}:${action.url}`;
}

export function primaryCatalogAction(actions: CatalogPublicationAction[]): CatalogPublicationAction | undefined {
  return actions.find((action) => action.kind === "import" && (isNativeApp || action.request.book_format === "epub" || action.request.book_format === "pdf"))
    ?? actions.find((action) => action.kind === "external");
}

export function catalogActionLabel(action: CatalogPublicationAction, ownership: CatalogOwnershipStatus, libraryName: string) {
  if (action.kind === "import") {
    const format = formatLabel(action.request.book_format);
    if (ownership === "exact_edition_owned") return `${format} added`;
    return ownership === "other_edition_owned" ? `Add this ${format} edition` : `Add ${format} to ${libraryName}`;
  }
  const purpose = action.purpose === "buy" ? "Buy" : action.purpose === "borrow" ? "Borrow" : action.purpose === "preview" ? "Preview" : action.purpose === "subscribe" ? "Subscribe" : "Open";
  const price = action.price ? `${action.price.currency} ${action.price.value}` : "";
  const formats = action.available_formats.map(formatLabel).join(" / ");
  return [purpose, price, formats].filter(Boolean).join(" · ");
}
