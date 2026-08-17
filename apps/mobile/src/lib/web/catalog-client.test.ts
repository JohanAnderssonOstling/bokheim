import assert from "node:assert/strict";
import test from "node:test";

import { ROOT_DIRECTORY_ID } from "../directory-id.ts";
import { getWebCatalogSubjectRecommendations, importWebCatalogBook, refreshWebCatalogSources } from "./catalog-client.ts";
import { createWebLibrary, getOwnedWebCatalogPublications, getWebLibraryHome, removeWebLibrary } from "./library-database.ts";
import { testEpub } from "./test-epub.ts";

test("browser catalogue client loads sources and imports a proxied EPUB", async () => {
  const originalFetch = globalThis.fetch;
  const library = await createWebLibrary(`Catalogue client ${crypto.randomUUID()}`);
  const source = {
    id: "opds-test",
    title: "Test",
    description: "Test catalogue",
    languages: ["English"],
    authentication: { kind: "none" as const },
    capabilities: ["browse" as const, "search" as const, "import" as const],
  };
  const request = {
    source_id: source.id,
    catalog_locator: "https://catalog.example/root",
    publication: { edition_id: "one", work_id: null },
    download_url: "https://catalog.example/one.epub",
    book_format: "epub" as const,
  };
  globalThis.fetch = async (input, init) => {
    const url = String(input);
    if (url.endsWith("/api/web/catalogs")) return Response.json([source]);
    assert.ok(url.endsWith("/api/web/sources/opds-test/acquire"));
    assert.equal(init?.method, "POST");
    assert.deepEqual(JSON.parse(String(init?.body)), request);
    return new Response(testEpub("Proxied Catalogue Book"), { headers: { "Content-Type": "application/epub+zip" } });
  };
  try {
    assert.deepEqual(await refreshWebCatalogSources(), [source]);
    await importWebCatalogBook({ library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID }, request);
    assert.deepEqual((await getWebLibraryHome(library.library_id)).recently_added.map(({ title }) => title), ["Proxied Catalogue Book"]);
    assert.deepEqual(await getOwnedWebCatalogPublications(library.library_id), [{
      source_id: source.id,
      publication: request.publication,
    }]);
  } finally {
    globalThis.fetch = originalFetch;
    await removeWebLibrary(library.library_id);
  }
});

test("browser subject recommendations preserve the hierarchy's canonical systems", async () => {
  const originalFetch = globalThis.fetch;
  const library = await createWebLibrary(`Subject recommendations ${crypto.randomUUID()}`);
  globalThis.fetch = async (input, init) => {
    assert.ok(String(input).endsWith("/api/web/recommendations"));
    const body = JSON.parse(String(init?.body));
    assert.deepEqual(body.subjects.map(({ system_id, subject_path }: { system_id: string; subject_path: string }) => [system_id, subject_path]), [
      ["lcsh", "LCC"], ["lcgft", "LCC"], ["lcc", "LCC"], ["ddc", "LCC"],
    ]);
    assert.equal(body.limit, 36, "the client over-fetches before applying its private ownership overlay");
    return Response.json({ recommendations: [] });
  };
  try {
    assert.deepEqual(await getWebCatalogSubjectRecommendations(library.library_id, "LCC", 12), { recommendations: [] });
  } finally {
    globalThis.fetch = originalFetch;
    await removeWebLibrary(library.library_id);
  }
});
