import type { LibraryFeature } from "../bindings.ts";
import type { LibraryPageLevel } from "../library/page-types.ts";
import type { BrowsingPreferences, LibraryEntry } from "../shared/types.ts";
import type { AppUiSession } from "../session/ui-session.ts";
import { defaultLibraryPlace } from "../session/ui-session.ts";
import { DEFAULT_BROWSING_PREFERENCES } from "../shared/types.ts";
import { destinationHref, readerHref } from "./navigation.ts";

export interface AppRouteSnapshot {
  id: string | null;
  libraryId?: string;
  folderId?: string;
  subjectId?: string;
  bookId?: string;
  readerLibraryId?: string;
}

export interface BrowserSession {
  remember(
    libraryId: string,
    route: LibraryFeature,
    folderPath: LibraryPageLevel[],
    subjectPath: LibraryPageLevel[],
  ): void;
  snapshot(libraryId?: string): AppUiSession | null;
}

const READER_ROUTE_KINDS = new Map<string, "epub" | "pdf">([
  ["/reader/epub/[bookId]", "epub"],
  ["/reader/pdf/[bookId]", "pdf"],
  ["/reader/v2/epub/[bookId]", "epub"],
  ["/reader/v2/pdf/[bookId]", "pdf"],
]);

export function isReaderRoute(routeId: string | null) {
  return routeId !== null && READER_ROUTE_KINDS.has(routeId);
}

export function loadWebBrowsingPreferencesFallback(
  isWebApp: boolean,
  appName: string,
): BrowsingPreferences | null {
  if (!isWebApp) return null;
  try {
    const saved = localStorage.getItem(`${appName}-browsing-preferences`);
    return saved ? { ...DEFAULT_BROWSING_PREFERENCES, ...JSON.parse(saved) } : null;
  } catch {
    return null;
  }
}

export function saveWebBrowsingPreferencesFallback(
  isWebApp: boolean,
  appName: string,
  preferences: BrowsingPreferences,
) {
  if (!isWebApp) return;
  try {
    localStorage.setItem(`${appName}-browsing-preferences`, JSON.stringify(preferences));
  } catch {
    // Browsers may deny storage in private or restricted contexts. The
    // worker-backed database remains the preferred persistence path.
  }
}

/**
 * The URL is the only record of where the reader is. Names are not in it, so
 * the id doubles as the label: nothing downstream reads the name, and storing
 * a blank one would make the session fail its own validation on the next read.
 */
function pathFromRouteId(locationId: string | undefined): LibraryPageLevel[] {
  const id = locationId?.trim();
  return id && id !== "__root__" ? [{ id, name: id }] : [];
}

export function sessionFromRoute({
  route,
  activeLibraryId,
  browserSession,
}: {
  route: AppRouteSnapshot;
  activeLibraryId: string;
  browserSession: BrowserSession;
}): AppUiSession | null {
  const libraryId =
    (route.readerLibraryId || route.libraryId || undefined)?.trim() || undefined;
  const readerKind = route.id === null ? undefined : READER_ROUTE_KINDS.get(route.id);
  if (readerKind && route.bookId?.trim()) {
    return {
      kind: "reader",
      content_hash: route.bookId,
      reader_kind: readerKind,
      library_id: libraryId,
    };
  }

  const routeLibraryId = route.libraryId || libraryId || activeLibraryId || undefined;
  let routeFeature: LibraryFeature | undefined;
  if (route.id === "/library/[libraryId]/folder/[folderId]") routeFeature = "folders";
  else if (route.id === "/library/[libraryId]/authors") routeFeature = "authors";
  else if (route.id === "/library/[libraryId]/subjects/[subjectId]")
    routeFeature = "subjects";
  else if (route.id === "/library/[libraryId]/trash") routeFeature = "trash";
  else if (route.id === "/library/[libraryId]") routeFeature = "home";
  if (routeLibraryId && routeFeature) {
    browserSession.remember(
      routeLibraryId,
      routeFeature,
      pathFromRouteId(route.folderId),
      pathFromRouteId(route.subjectId),
    );
  }
  return browserSession.snapshot(routeLibraryId);
}

/**
 * The route a restored session should reopen — the inverse of
 * `sessionFromRoute`. The two must agree: what one writes into the session, the
 * other has to be able to turn back into the same URL.
 */
export function routeFromSession(session: AppUiSession): string {
  if (session.kind === "reader")
    return readerHref(session.reader_kind, session.content_hash, session.library_id);
  const libraryId = session.active_library_id;
  if (!libraryId) return "/library";
  const place = session.places[libraryId] ?? defaultLibraryPlace();
  const locationId =
    place.route === "folders"
      ? place.folder_path.at(-1)?.id
      : place.route === "subjects"
        ? place.subject_path.at(-1)?.id
        : undefined;
  return destinationHref(place.route, libraryId, locationId);
}

/**
 * A locally opened file lives only in this tab's memory, so a session pointing
 * at one would restore into a reader with nothing to read.
 */
export function persistableSession(session: AppUiSession | null): AppUiSession | null {
  if (!session) return null;
  if (session.kind === "reader" && session.content_hash.startsWith("foliate-position:"))
    return null;
  return session;
}

const naturalCollator = new Intl.Collator(undefined, {
  sensitivity: "base",
  numeric: true,
});

export function naturalCompare(left: string, right: string) {
  return naturalCollator.compare(left, right);
}

export function sortedLibraries(entries: LibraryEntry[]) {
  return [...entries].sort((left, right) =>
    naturalCompare(left.library_name, right.library_name),
  );
}
