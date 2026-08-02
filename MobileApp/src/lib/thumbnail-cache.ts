import { commands } from "./commands";
import { measureInvoke } from "./performance";

type BinaryPayload = ArrayBuffer | Uint8Array | number[];
type CacheEntry = {
  active: boolean;
  promise: Promise<string | null>;
  url?: string;
};

const MAX_CACHED_THUMBNAILS = 160;
const cache = new Map<string, CacheEntry>();

function asBytes(payload: BinaryPayload): Uint8Array {
  if (payload instanceof Uint8Array) return payload;
  if (payload instanceof ArrayBuffer) return new Uint8Array(payload);
  return Uint8Array.from(payload);
}

function evictOverflow() {
  while (cache.size > MAX_CACHED_THUMBNAILS) {
    const oldestKey = cache.keys().next().value as string | undefined;
    if (!oldestKey) return;
    const entry = cache.get(oldestKey);
    cache.delete(oldestKey);
    if (!entry) continue;
    entry.active = false;
    if (entry.url) URL.revokeObjectURL(entry.url);
  }
}

export function getThumbnailUrl(bookId: string): Promise<string | null> {
  const existing = cache.get(bookId);
  if (existing) {
    cache.delete(bookId);
    cache.set(bookId, existing);
    return existing.promise;
  }

  const entry: CacheEntry = { active: true, promise: Promise.resolve(null) };
  entry.promise = measureInvoke("thumbnail", () => commands.getBookThumbnail(bookId) as unknown as Promise<BinaryPayload>)
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
    .catch(() => null);
  cache.set(bookId, entry);
  evictOverflow();
  return entry.promise;
}

export function clearThumbnailCache() {
  for (const entry of cache.values()) {
    entry.active = false;
    if (entry.url) URL.revokeObjectURL(entry.url);
  }
  cache.clear();
}
