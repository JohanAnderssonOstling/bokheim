import { FRONTEND_CONTRACT, type LibraryFeature } from "../bindings.ts";

const STORAGE_KEY = "bokheim-ui-session-v3";
const LEGACY_STORAGE_KEY = "bokheim-ui-session-v2";
const SCHEMA_VERSION = 3;

export type PlaceLevel = { id: string; name: string };

export type LibraryPlace = {
  route: LibraryFeature;
  folder_path: PlaceLevel[];
  subject_path: PlaceLevel[];
};

export type ReaderUiSession = {
  kind: "reader";
  content_hash: string;
  reader_kind: "epub" | "pdf";
  library_id?: string;
};

export type BrowseUiSession = {
  kind: "browse";
  active_library_id?: string;
  places: Record<string, LibraryPlace>;
};

export type AppUiSession = ReaderUiSession | BrowseUiSession;

type PersistedUiSession = { v: number; session: AppUiSession };

export function defaultLibraryPlace(): LibraryPlace {
  return {
    route: FRONTEND_CONTRACT.browser_session.default_library_route,
    folder_path: [],
    subject_path: [],
  };
}

function hasLocalStorage(): boolean {
  return typeof window !== "undefined" && typeof window.localStorage !== "undefined";
}

function normalizeId(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0 ? value : undefined;
}

function normalizeLibraryRoute(value: unknown): value is LibraryFeature {
  return FRONTEND_CONTRACT.navigation.some(({ id }) => id === value);
}

function normalizePath(value: unknown): PlaceLevel[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((entry) => {
    if (!entry || typeof entry !== "object") return [];
    const id = normalizeId((entry as { id?: unknown }).id);
    const name = normalizeId((entry as { name?: unknown }).name);
    return id && name ? [{ id, name }] : [];
  });
}

function normalizePlaces(value: unknown): Record<string, LibraryPlace> {
  if (!value || typeof value !== "object") return {};
  const places: Record<string, LibraryPlace> = {};
  for (const [libraryId, raw] of Object.entries(value)) {
    if (!normalizeId(libraryId) || !raw || typeof raw !== "object") continue;
    const route = (raw as { route?: unknown }).route;
    places[libraryId] = {
      route: normalizeLibraryRoute(route) ? route : defaultLibraryPlace().route,
      folder_path: normalizePath((raw as { folder_path?: unknown }).folder_path),
      subject_path: normalizePath((raw as { subject_path?: unknown }).subject_path),
    };
  }
  return places;
}

function normalizeSession(value: unknown): AppUiSession | null {
  if (!value || typeof value !== "object") return null;
  const session = value as Record<string, unknown>;
  if (
    session.kind === "reader" &&
    typeof session.content_hash === "string" &&
    /^[0-9a-f]{64}$/.test(session.content_hash)
  ) {
    return {
      kind: "reader",
      content_hash: session.content_hash,
      reader_kind: session.reader_kind === "pdf" ? "pdf" : "epub",
      library_id: normalizeId(session.library_id),
    };
  }
  if (session.kind === "browse") {
    return {
      kind: "browse",
      active_library_id: normalizeId(session.active_library_id),
      places: normalizePlaces(session.places),
    };
  }
  return null;
}

function migrateLegacySession(value: unknown): AppUiSession | null {
  if (!value || typeof value !== "object") return null;
  const session = (value as { session?: Record<string, unknown> }).session;
  if (!session) return null;
  if (session.kind === "reader") return normalizeSession(session);
  const libraryId = normalizeId(session.library_id);
  const destination = session.destination;
  const route = normalizeLibraryRoute(destination)
    ? destination
    : defaultLibraryPlace().route;
  const places = libraryId
    ? {
        [libraryId]: {
          route,
          folder_path: session.folder_id
            ? [{ id: String(session.folder_id), name: String(session.folder_id) }]
            : [],
          subject_path: session.subject_id
            ? [{ id: String(session.subject_id), name: String(session.subject_id) }]
            : [],
        },
      }
    : {};
  return { kind: "browse", active_library_id: libraryId, places };
}

export function readUiSession(): AppUiSession | null {
  if (!hasLocalStorage()) return null;
  const saved = window.localStorage.getItem(STORAGE_KEY);
  if (saved) {
    try {
      const parsed = JSON.parse(saved) as Partial<PersistedUiSession>;
      if (parsed.v === SCHEMA_VERSION) return normalizeSession(parsed.session);
    } catch {
      /* ignore corrupt state */
    }
  }
  const legacy = window.localStorage.getItem(LEGACY_STORAGE_KEY);
  if (!legacy) return null;
  try {
    return migrateLegacySession(JSON.parse(legacy));
  } catch {
    return null;
  }
}

export function writeUiSession(session: AppUiSession | null): void {
  if (!hasLocalStorage()) return;
  if (!session) window.localStorage.removeItem(STORAGE_KEY);
  else
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ v: SCHEMA_VERSION, session }),
    );
}
