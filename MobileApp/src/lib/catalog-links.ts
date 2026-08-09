import type { CatalogLink } from "./bindings";

export type CatalogLinkAction = { kind: "disabled" } | { kind: "load_page"; title: string; locator: string } | { kind: "open_external"; url: string };

export function catalogLinkAction(link: CatalogLink): CatalogLinkAction {
  if (link.active) return { kind: "disabled" };
  return link.target.kind === "page" ? { kind: "load_page", title: link.title, locator: link.target.locator } : { kind: "open_external", url: link.target.url };
}

export function catalogLinkCount(link: CatalogLink) {
  return link.number_of_items === null ? "" : link.number_of_items.toLocaleString();
}
