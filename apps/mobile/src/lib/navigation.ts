import type { LibraryFeature, NavigationIcon, UniversalFeature } from "./bindings.ts";
import { FRONTEND_CONTRACT } from "./types.ts";

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

const UNIVERSAL_IDS: readonly Destination[] = FRONTEND_CONTRACT.app_navigation.map(({ id }) => id);

export function isUniversalDestination(destination: Destination): destination is UniversalDestination {
  return UNIVERSAL_IDS.includes(destination);
}

export function universalDestinationForRoute(routeId: string | null): UniversalDestination | null {
  if (!routeId?.startsWith("/")) return null;
  const destination = routeId.slice(1) as Destination;
  return isUniversalDestination(destination) ? destination : null;
}

export function readerRouteCacheKey(libraryId: string, bookId: string): string {
  return `${libraryId}\u0000${bookId}`;
}

export function libraryReaderBookKey(libraryId: string, bookId: string): string {
  return `library:${libraryId}:${bookId}`;
}
