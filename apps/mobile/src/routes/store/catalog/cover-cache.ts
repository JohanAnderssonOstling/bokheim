import { commands } from "../../../lib/app/commands";

type BinaryPayload = ArrayBuffer | Uint8Array | number[];
type CacheEntry = { active: boolean; promise: Promise<string | null>; url?: string };

const MAX_CACHED_CATALOG_COVERS = 160;
const cache = new Map<string, CacheEntry>();

function asBytes(payload: BinaryPayload): Uint8Array {
  if (payload instanceof Uint8Array) return payload;
  if (payload instanceof ArrayBuffer) return new Uint8Array(payload);
  return Uint8Array.from(payload);
}

function evictOverflow() {
  while (cache.size > MAX_CACHED_CATALOG_COVERS) {
    const oldestKey = cache.keys().next().value as string | undefined;
    if (!oldestKey) return;
    const entry = cache.get(oldestKey);
    cache.delete(oldestKey);
    if (!entry) continue;
    entry.active = false;
    if (entry.url) URL.revokeObjectURL(entry.url);
  }
}

export function getCatalogCoverUrl(
  sourceId: string,
  imageUrl: string,
): Promise<string | null> {
  const key = `${sourceId}\n${imageUrl}`;
  const existing = cache.get(key);
  if (existing) {
    cache.delete(key);
    cache.set(key, existing);
    return existing.promise;
  }
  const entry: CacheEntry = { active: true, promise: Promise.resolve(null) };
  entry.promise = (
    commands.getCatalogCover(sourceId, imageUrl) as unknown as Promise<BinaryPayload>
  )
    .then((payload) => {
      const bytes = asBytes(payload);
      if (!bytes.byteLength) return null;
      const url = URL.createObjectURL(new Blob([bytes], { type: "image/jpeg" }));
      if (!entry.active) {
        URL.revokeObjectURL(url);
        return null;
      }
      entry.url = url;
      return url;
    })
    .catch(() => {
      if (cache.get(key) === entry) cache.delete(key);
      return null;
    });
  cache.set(key, entry);
  evictOverflow();
  return entry.promise;
}

export function clearCatalogCoverCache() {
  for (const entry of cache.values()) {
    entry.active = false;
    if (entry.url) URL.revokeObjectURL(entry.url);
  }
  cache.clear();
}
