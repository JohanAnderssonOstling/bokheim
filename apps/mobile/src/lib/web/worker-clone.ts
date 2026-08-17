function isInstance(value: object, constructorName: "Blob" | "File"): boolean {
  const constructor = globalThis[constructorName] as (new (...args: never[]) => object) | undefined;
  return typeof constructor === "function" && value instanceof constructor;
}

/**
 * Svelte's reactive objects are proxies, which the browser structured-clone
 * algorithm rejects. Rebuild ordinary data containers while preserving the
 * browser-native values that postMessage already knows how to clone.
 */
export function toCloneableWorkerValue<T>(value: T, seen = new WeakMap<object, unknown>()): T {
  if (value === null || typeof value !== "object") return value;
  if (isInstance(value, "Blob") || isInstance(value, "File") || value instanceof ArrayBuffer || ArrayBuffer.isView(value) || value instanceof Date) return value;
  const previous = seen.get(value);
  if (previous !== undefined) return previous as T;
  if (Array.isArray(value)) {
    const clone: unknown[] = [];
    seen.set(value, clone);
    for (const entry of value) clone.push(toCloneableWorkerValue(entry, seen));
    return clone as T;
  }
  if (value instanceof Map) {
    const clone = new Map();
    seen.set(value, clone);
    for (const [key, entry] of value) clone.set(toCloneableWorkerValue(key, seen), toCloneableWorkerValue(entry, seen));
    return clone as T;
  }
  if (value instanceof Set) {
    const clone = new Set();
    seen.set(value, clone);
    for (const entry of value) clone.add(toCloneableWorkerValue(entry, seen));
    return clone as T;
  }
  const clone: Record<string, unknown> = {};
  seen.set(value, clone);
  for (const [key, entry] of Object.entries(value)) clone[key] = toCloneableWorkerValue(entry, seen);
  return clone as T;
}
