import type { Destination } from "./navigation";

const STORAGE_KEY = "bokheim-ui-session-v2";
const SCHEMA_VERSION = 2;

export type ReaderUiSession = {
  kind: "reader";
  content_hash: string;
  library_id?: string;
};

export type BrowseUiSession = {
  kind: "browse";
  destination: Destination;
  library_id?: string;
  folder_id?: string;
  subject_id?: string;
};

export type AppUiSession = ReaderUiSession | BrowseUiSession;

type PersistedUiSession = {
  v: number;
  session: AppUiSession;
};

function hasLocalStorage(): boolean {
  return typeof window !== "undefined" && typeof window.localStorage !== "undefined";
}

function normalizeContentHash(value: unknown): string | null {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value) ? value : null;
}

function normalizeDestination(value: unknown): value is Destination {
  if (typeof value !== "string") return false;
  return value === "home" || value === "folders" || value === "subjects" || value === "authors" || value === "store" || value === "account" || value === "settings";
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

  if (session.kind === "reader" && normalizeContentHash(session.content_hash)) {
    return {
      kind: "reader",
      content_hash: normalizeContentHash(session.content_hash)!,
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
      subject_id: normalizeId((session as { subject_id?: unknown }).subject_id),
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
