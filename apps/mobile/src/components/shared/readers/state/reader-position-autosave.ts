export type ReaderAutosaveState = "idle" | "saving" | "saved" | "error";

interface ReaderPositionAutosaveOptions<T> {
  persist: (value: T) => Promise<void> | void;
  debounceMs?: number;
  areEqual?: (left: T, right: T) => boolean;
  onStateChange?: (state: ReaderAutosaveState) => void;
  enabled?: boolean;
}

interface ReaderPositionAutosaveController<T> {
  schedule: (value: T) => void;
  flush: () => Promise<void>;
  clearPending: () => void;
  dispose: () => void;
}

const NONE = Symbol("uninitialized-reader-position-autosave-value");

export function createReaderPositionAutosave<T>(
  options: ReaderPositionAutosaveOptions<T>,
): ReaderPositionAutosaveController<T> {
  const {
    persist,
    debounceMs = 450,
    areEqual = (left, right) => left === (right as unknown),
    onStateChange,
    enabled = true,
  } = options;

  let queued: Promise<void> = Promise.resolve();
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  let pendingValue: T | typeof NONE = NONE;
  let lastPersistedValue: T | typeof NONE = NONE;
  let isDisposed = false;

  let active = enabled;

  function setState(next: ReaderAutosaveState) {
    if (!isDisposed) onStateChange?.(next);
  }

  async function persistValue(value: T) {
    if (lastPersistedValue !== NONE && areEqual(value, lastPersistedValue)) return;
    setState("saving");
    await persist(value);
    lastPersistedValue = value;
    setState("saved");
  }

  function queuePersist(value: T) {
    queued = queued
      .then(() => persistValue(value))
      .catch((error) => {
        if (!isDisposed) setState("error");
        throw error;
      });
    return queued;
  }

  function schedule(value: T) {
    if (isDisposed || !active) return;

    if (
      pendingValue !== NONE &&
      areEqual(pendingValue, value) &&
      lastPersistedValue !== NONE &&
      areEqual(value, lastPersistedValue)
    )
      return;

    pendingValue = value;

    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      debounceTimer = undefined;
      const next = pendingValue;
      pendingValue = NONE;
      void queuePersist(next as T).catch(() => {
        /* errors are surfaced through onStateChange */
      });
    }, debounceMs);
  }

  async function flush() {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = undefined;
    }
    if (pendingValue !== NONE) {
      const next = pendingValue;
      pendingValue = NONE;
      await queuePersist(next as T);
    }
    await queued;
  }

  function clearPending() {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = undefined;
    }
    pendingValue = NONE;
  }

  function dispose() {
    isDisposed = true;
    active = false;
    clearPending();
  }

  return {
    schedule,
    flush,
    clearPending,
    dispose,
  };
}
