import assert from "node:assert/strict";
import test from "node:test";
import type { CatalogLink, CatalogPublicationAction } from "./bindings";
import { catalogActionKey, catalogActionLabel } from "./catalog-actions.ts";
import { catalogLinkAction, catalogLinkCount } from "./catalog-links.ts";

function link(overrides: Partial<CatalogLink> = {}): CatalogLink {
  return { title: "Books", target: { kind: "page", locator: "catalog:books" }, number_of_items: null, active: false, ...overrides };
}

test("page and external links resolve to different exhaustive actions", () => {
  assert.deepEqual(catalogLinkAction(link()), { kind: "load_page", title: "Books", locator: "catalog:books" });
  assert.deepEqual(catalogLinkAction(link({ target: { kind: "external", url: "https://example.com" } })), { kind: "open_external", url: "https://example.com" });
  assert.deepEqual(catalogLinkAction(link({ active: true })), { kind: "disabled" });
});

test("facet counts preserve zero and format populated counts", () => {
  assert.equal(catalogLinkCount(link({ number_of_items: null })), "");
  assert.equal(catalogLinkCount(link({ number_of_items: 0 })), "0");
  assert.equal(catalogLinkCount(link({ number_of_items: 1234 })), (1234).toLocaleString());
});

test("import actions are format-specific and external paid actions never look importable", () => {
  const request = { source_id: "opds-test", catalog_locator: "https://example.com/catalog", publication: { edition_id: "edition", work_id: null }, download_url: "https://example.com/book.epub", book_format: "epub" as const };
  const importAction: CatalogPublicationAction = { kind: "import", request };
  const paidAction: CatalogPublicationAction = { kind: "external", purpose: "buy", url: "https://example.com/buy", price: { currency: "EUR", value: "3.50" }, available_formats: ["epub", "pdf"] };
  assert.equal(catalogActionLabel(importAction, "not_owned", "Test"), "Add EPUB to Test");
  assert.equal(catalogActionLabel(paidAction, "not_owned", "Test"), "Buy · EUR 3.50 · EPUB / PDF");
  assert.notEqual(catalogActionKey(importAction), catalogActionKey(paidAction));
});
