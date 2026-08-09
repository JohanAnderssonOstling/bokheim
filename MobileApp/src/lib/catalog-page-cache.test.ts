import assert from "node:assert/strict";
import test from "node:test";
import type { CatalogPage } from "./bindings";
import { createCatalogPageCache } from "./catalog-page-cache.ts";

function page(locator: string): CatalogPage {
  return { source_id: "opds-test", freshness: "current", title: locator, locator, navigation: [], publications: [], groups: [], facets: [], pagination: { first: null, previous: null, next: null, last: null, number_of_items: null, items_per_page: null, current_page: null }, search: null };
}

test("intent prefetch and navigation share one in-flight request", async () => {
  let resolveFetch: ((value: CatalogPage) => void) | undefined;
  let fetches = 0;
  const cache = createCatalogPageCache(async (_sourceId, locator) => {
    fetches += 1;
    return new Promise<CatalogPage>((resolve) => { resolveFetch = resolve; });
  });
  cache.prefetch("opds-test", "https://example.com/epub");
  const navigation = cache.get("opds-test", "https://example.com/epub");
  await Promise.resolve();
  assert.equal(fetches, 1);
  resolveFetch?.(page("https://example.com/epub"));
  assert.equal((await navigation).title, "https://example.com/epub");
});

test("completed pages expire and failed prefetches are retryable", async () => {
  let currentTime = 100;
  let fetches = 0;
  const cache = createCatalogPageCache(async (_sourceId, locator) => {
    fetches += 1;
    if (fetches === 1) throw new Error("temporary failure");
    return page(locator ?? "root");
  }, 50, 16, () => currentTime);
  cache.prefetch("opds-test", "https://example.com/page");
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal((await cache.get("opds-test", "https://example.com/page")).locator, "https://example.com/page");
  assert.equal(fetches, 2);
  assert.equal((await cache.get("opds-test", "https://example.com/page")).locator, "https://example.com/page");
  assert.equal(fetches, 2);
  currentTime = 151;
  await cache.get("opds-test", "https://example.com/page");
  assert.equal(fetches, 3);
});

test("navigation jumps ahead of queued speculative work and cancelled work never starts", async () => {
  const started: string[] = [];
  const resolvers = new Map<string, (page: CatalogPage) => void>();
  const cache = createCatalogPageCache(async (_sourceId, locator) => {
    const destination = locator ?? "root";
    started.push(destination);
    return new Promise<CatalogPage>((resolve) => { resolvers.set(destination, resolve); });
  });
  cache.prefetch("opds-test", "slow");
  const cancelUnused = cache.prefetch("opds-test", "unused");
  cache.prefetch("opds-test", "chosen");
  await Promise.resolve();
  assert.deepEqual(started, ["slow"]);
  const chosen = cache.get("opds-test", "chosen");
  await Promise.resolve();
  assert.deepEqual(started, ["slow", "chosen"]);
  cancelUnused();
  resolvers.get("chosen")?.(page("chosen"));
  resolvers.get("slow")?.(page("slow"));
  await chosen;
  await Promise.resolve();
  assert.deepEqual(started, ["slow", "chosen"]);
});

test("back navigation reuses completed pages while stale pages are checked again", async () => {
  const fetches = new Map<string, number>();
  const cache = createCatalogPageCache(async (_sourceId, locator) => {
    const destination = locator ?? "root";
    fetches.set(destination, (fetches.get(destination) ?? 0) + 1);
    const loaded = page(destination);
    if (destination === "stale") loaded.freshness = "stale";
    return loaded;
  });
  await cache.get("opds-test", "first");
  await cache.get("opds-test", "second");
  await cache.get("opds-test", "first");
  await cache.get("opds-test", "stale");
  await cache.get("opds-test", "stale");
  assert.equal(fetches.get("first"), 1);
  assert.equal(fetches.get("second"), 1);
  assert.equal(fetches.get("stale"), 2);
});
