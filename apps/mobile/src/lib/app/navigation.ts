import type { LibraryFeature, NavigationIcon, UniversalFeature } from "../bindings.ts";
import { FRONTEND_CONTRACT } from "../shared/types.ts";

/// A destination whose contents belong to the selected library.
export type PrimaryDestination = LibraryFeature;
/// A destination that outlives the selected library.
export type UniversalDestination = UniversalFeature;
export type Destination = PrimaryDestination | UniversalDestination;

export interface NavigationItem {
  id: Destination;
  label: string;
  icon: NavigationIcon;
}

/// Library-scoped destinations, in contract order.
export const NAV_ITEMS: readonly NavigationItem[] = FRONTEND_CONTRACT.navigation;

/// App-scoped destinations, in contract order. Drawn as a separate group: the
/// boundary between "this library" and "this app" is a real distinction and
/// navigation should show it.
export const APP_NAV_ITEMS: readonly NavigationItem[] = FRONTEND_CONTRACT.app_navigation;

const UNIVERSAL_IDS: readonly Destination[] = FRONTEND_CONTRACT.app_navigation.map(
  ({ id }) => id,
);

export function isUniversalDestination(
  destination: Destination,
): destination is UniversalDestination {
  return UNIVERSAL_IDS.includes(destination);
}

export function universalDestinationForRoute(
  routeId: string | null,
): UniversalDestination | null {
  if (!routeId?.startsWith("/")) return null;
  const destination = routeId.slice(1) as Destination;
  return isUniversalDestination(destination) ? destination : null;
}

const LIBRARY_ROUTE_DESTINATIONS: Readonly<Record<string, PrimaryDestination>> = {
  "/library/[libraryId]": "home",
  "/library/[libraryId]/folder/[folderId]": "folders",
  "/library/[libraryId]/subjects/[subjectId]": "subjects",
  "/library/[libraryId]/authors": "authors",
  "/library/[libraryId]/trash": "trash",
};

/**
 * The sidebar destination a library route belongs to.
 *
 * Falls back to "home" for anything under a library that is not itself a
 * sidebar destination, such as book detail, so the sidebar keeps a selection.
 */
export function libraryDestinationForRoute(routeId: string | null): PrimaryDestination {
  return (routeId && LIBRARY_ROUTE_DESTINATIONS[routeId]) || "home";
}

export function readerHref(
  readerKind: "epub" | "pdf",
  contentHash: string,
  libraryId?: string,
) {
  const library = libraryId ? `?library=${encodeURIComponent(libraryId)}` : "";
  return `/reader/${readerKind}/${encodeURIComponent(contentHash)}${library}`;
}

export function libraryReaderBookKey(libraryId: string, bookId: string): string {
  return `library:${libraryId}:${bookId}`;
}

export function libraryRootHref(libraryId: string) {
  return `/library/${encodeURIComponent(libraryId)}`;
}

export function libraryFolderHref(libraryId: string, folderId = "__root__") {
  return `${libraryRootHref(libraryId)}/folder/${encodeURIComponent(folderId)}`;
}

export function librarySubjectsHref(libraryId: string, subjectId = "__root__") {
  return `${libraryRootHref(libraryId)}/subjects/${encodeURIComponent(subjectId)}`;
}

export function libraryAuthorsHref(libraryId: string) {
  return `${libraryRootHref(libraryId)}/authors`;
}

export function libraryTrashHref(libraryId: string) {
  return `${libraryRootHref(libraryId)}/trash`;
}

export function libraryBookHref(libraryId: string, contentHash: string) {
  return `${libraryRootHref(libraryId)}/book/${encodeURIComponent(contentHash)}`;
}

/**
 * The canonical route for a sidebar destination.
 *
 * `locationId` names the folder or subject to land in; without it the
 * destination opens at its root.
 */
export function destinationHref(
  destination: Destination,
  libraryId?: string,
  locationId?: string,
) {
  if (destination === "account" || destination === "settings" || destination === "store")
    return `/${destination}`;
  if (!libraryId) return "/library";
  if (destination === "folders") return libraryFolderHref(libraryId, locationId);
  if (destination === "subjects") return librarySubjectsHref(libraryId, locationId);
  if (destination === "authors") return libraryAuthorsHref(libraryId);
  if (destination === "trash") return libraryTrashHref(libraryId);
  return libraryRootHref(libraryId);
}
