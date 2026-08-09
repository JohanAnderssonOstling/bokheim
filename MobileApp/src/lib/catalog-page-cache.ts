import type { CatalogPage } from "./bindings";

type CatalogPageFetcher = (sourceId: string, locator: string | null) => Promise<CatalogPage>;
type DeferredPage = { promise: Promise<CatalogPage>; resolve: (page: CatalogPage) => void; reject: (reason: unknown) => void };
type QueuedEntry = DeferredPage & { kind: "queued"; sourceId: string; locator: string | null };
type LoadingEntry = { kind: "loading"; promise: Promise<CatalogPage>; speculative: boolean };
type ReadyEntry = { kind: "ready"; promise: Promise<CatalogPage>; expiresAt: number };
type CacheEntry = QueuedEntry | LoadingEntry | ReadyEntry;

const DEFAULT_TTL_MS = 5 * 60 * 1000;
const DEFAULT_MAX_PAGES = 16;
const DEFAULT_SPECULATIVE_CONCURRENCY = 1;

export function createCatalogPageCache(fetchPage: CatalogPageFetcher, ttlMs = DEFAULT_TTL_MS, maxPages = DEFAULT_MAX_PAGES, now = Date.now, speculativeConcurrency = DEFAULT_SPECULATIVE_CONCURRENCY) {
  const entries = new Map<string, CacheEntry>();
  const speculativeQueue: string[] = [];
  let activeSpeculative = 0;

  function key(sourceId: string, locator: string | null) {
    return `${sourceId}\u0000${locator ?? ""}`;
  }

  function deferredPage(): DeferredPage {
    let resolve!: (page: CatalogPage) => void;
    let reject!: (reason: unknown) => void;
    const promise = new Promise<CatalogPage>((resolvePromise, rejectPromise) => { resolve = resolvePromise; reject = rejectPromise; });
    return { promise, resolve, reject };
  }

  function queuedEntry(sourceId: string, locator: string | null): QueuedEntry {
    return { kind: "queued", sourceId, locator, ...deferredPage() };
  }

  function removeFromQueue(cacheKey: string) {
    const index = speculativeQueue.indexOf(cacheKey);
    if (index >= 0) speculativeQueue.splice(index, 1);
  }

  function trimReadyPages() {
    const readyKeys = [...entries].filter(([, entry]) => entry.kind === "ready").map(([cacheKey]) => cacheKey);
    while (readyKeys.length > maxPages) entries.delete(readyKeys.shift()!);
  }

  function finishSpeculative() {
    activeSpeculative -= 1;
    drainSpeculativeQueue();
  }

  function start(cacheKey: string, queued: QueuedEntry, speculative: boolean) {
    removeFromQueue(cacheKey);
    const loading: LoadingEntry = { kind: "loading", promise: queued.promise, speculative };
    entries.set(cacheKey, loading);
    if (speculative) activeSpeculative += 1;
    void Promise.resolve().then(() => fetchPage(queued.sourceId, queued.locator)).then(queued.resolve, queued.reject);
    void queued.promise.then((loaded) => {
      if (entries.get(cacheKey) === loading) {
        entries.set(cacheKey, { kind: "ready", promise: Promise.resolve(loaded), expiresAt: loaded.freshness === "stale" ? now() : now() + ttlMs });
        trimReadyPages();
      }
      if (speculative) finishSpeculative();
    }, () => {
      if (entries.get(cacheKey) === loading) entries.delete(cacheKey);
      if (speculative) finishSpeculative();
    });
  }

  function drainSpeculativeQueue() {
    while (activeSpeculative < speculativeConcurrency) {
      const cacheKey = speculativeQueue.shift();
      if (cacheKey === undefined) return;
      const entry = entries.get(cacheKey);
      if (entry?.kind === "queued") start(cacheKey, entry, true);
    }
  }

  function get(sourceId: string, locator: string | null) {
    const cacheKey = key(sourceId, locator);
    const existing = entries.get(cacheKey);
    if (existing?.kind === "ready" && existing.expiresAt <= now()) {
      entries.delete(cacheKey);
    } else if (existing?.kind === "ready") {
      entries.delete(cacheKey);
      entries.set(cacheKey, existing);
      return existing.promise;
    } else if (existing?.kind === "loading") {
      return existing.promise;
    } else if (existing?.kind === "queued") {
      start(cacheKey, existing, false);
      return existing.promise;
    }
    const queued = queuedEntry(sourceId, locator);
    entries.set(cacheKey, queued);
    start(cacheKey, queued, false);
    return queued.promise;
  }

  function prefetch(sourceId: string, locator: string | null) {
    const cacheKey = key(sourceId, locator);
    const existing = entries.get(cacheKey);
    if (existing?.kind === "ready" && existing.expiresAt <= now()) entries.delete(cacheKey);
    else if (existing) return () => cancelPrefetch(cacheKey);
    const queued = queuedEntry(sourceId, locator);
    entries.set(cacheKey, queued);
    speculativeQueue.push(cacheKey);
    void queued.promise.catch(() => undefined);
    drainSpeculativeQueue();
    return () => cancelPrefetch(cacheKey);
  }

  function cancelPrefetch(cacheKey: string) {
    const entry = entries.get(cacheKey);
    if (entry?.kind !== "queued") return;
    removeFromQueue(cacheKey);
    entries.delete(cacheKey);
    entry.reject(new Error("catalog prefetch cancelled"));
  }

  return { get, prefetch };
}
