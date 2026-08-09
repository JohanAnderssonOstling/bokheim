import type { Destination } from "./navigation";

const STORAGE_KEY = "bokheim-ui-session-v1";
const SCHEMA_VERSION = 1;

export type ReaderUiSession = {
  kind: "reader";
  book_id: string;
  library_id?: string;
};

export type BrowseUiSession = {
  kind: "browse";
  destination: Destination;
  library_id?: string;
  folder_id?: string;
};

export type AppUiSession = ReaderUiSession | BrowseUiSession;

type PersistedUiSession = {
  v: number;
  session: AppUiSession;
};

function hasLocalStorage(): boolean {
  return typeof window !== "undefined" && typeof window.localStorage !== "undefined";
}

function normalizeBookId(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

function normalizeDestination(value: unknown): value is Destination {
  if (typeof value !== "string") return false;
  return value === "home" || value === "library" || value === "authors" || value === "search" || value === "store" || value === "account" || value === "settings";
}

function normalizeId(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0 ? value : undefined;
}

export function readUiSession(): AppUiSession | null {
  if (!hasLocalStorage()) return null;
  const saved = window.localStorage.getItem(STORAGE_KEY);
  if (!saved) return null;

  let parsed: unknown;
  try {
    parsed = JSON.parse(saved);
  } catch {
    return null;
  }

  if (typeof parsed !== "object" || parsed === null) return null;
  const container = parsed as Partial<PersistedUiSession>;
  if (container.v !== SCHEMA_VERSION || typeof container.session !== "object" || container.session === null) return null;
  const session = container.session;

  if (session.kind === "reader" && normalizeBookId(session.book_id)) {
    return {
      kind: "reader",
      book_id: normalizeBookId(session.book_id)!,
      library_id: normalizeId((session as { library_id?: unknown }).library_id),
    };
  }

  if (session.kind === "browse" && normalizeDestination(session.destination)) {
    const destination = session.destination;
    const normalized = {
      kind: "browse" as const,
      destination,
      library_id: normalizeId((session as { library_id?: unknown }).library_id),
      folder_id: normalizeId((session as { folder_id?: unknown }).folder_id),
    };
    return normalized;
  }

  return null;
}

export function writeUiSession(session: AppUiSession | null): void {
  if (!hasLocalStorage()) return;
  if (!session) {
    window.localStorage.removeItem(STORAGE_KEY);
    return;
  }
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify({ v: SCHEMA_VERSION, session }));
}

