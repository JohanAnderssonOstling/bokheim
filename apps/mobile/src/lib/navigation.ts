import type { LibraryFeature, UniversalFeature } from "./bindings";
import { FRONTEND_CONTRACT } from "./types";

/// A destination whose contents belong to the selected library.
export type PrimaryDestination = LibraryFeature;
/// A destination that outlives the selected library.
export type UniversalDestination = UniversalFeature;
export type Destination = PrimaryDestination | UniversalDestination;

export interface NavigationItem {
  id: Destination;
  label: string;
  path: string;
}

const LIBRARY_PATHS: Record<LibraryFeature, string> = {
  home: "M3 10.8 12 3l9 7.8v9.7a.5.5 0 0 1-.5.5H15v-6H9v6H3.5a.5.5 0 0 1-.5-.5z",
  folders: "M4 4.5A2.5 2.5 0 0 1 6.5 2H20v17H6.5A2.5 2.5 0 0 0 4 21.5zm0 17A2.5 2.5 0 0 1 6.5 19H20v3H6.5A2.5 2.5 0 0 1 4 19.5z",
  subjects: "M20.6 13.6 13.7 20.5a2 2 0 0 1-2.8 0L3.5 13.1a2 2 0 0 1 0-2.8V4h6.3a2 2 0 0 1 1.4.6l9.4 9a1.4 1.4 0 0 1 0 2zM7.2 7.2h.1",
  authors: "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2m7-10a4 4 0 1 0 0-8 4 4 0 0 0 0 8m13 10v-2a4 4 0 0 0-3-3.87m-2-12a4 4 0 0 1 0 7.75",
};

const UNIVERSAL_PATHS: Record<UniversalFeature, string> = {
  store: "M4 9h16l-1 12H5zM3 9l2-5h14l2 5M9 9V6a3 3 0 0 1 6 0v3",
  account: "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8m8 9a8 8 0 1 0-16 0",
  settings: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6m7.4-3a7.4 7.4 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7.4 7.4 0 0 0-2-1.2l-.4-2.6h-4l-.4 2.6a7.4 7.4 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6a7.4 7.4 0 0 0 0 2.4l-2 1.6 2 3.4 2.4-1a7.4 7.4 0 0 0 2 1.2l.4 2.6h4l.4-2.6a7.4 7.4 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6c.07-.4.1-.8.1-1.2",
};

/// Library-scoped destinations, in contract order.
export const NAV_ITEMS: NavigationItem[] = FRONTEND_CONTRACT.navigation.map(({ id, label }) => ({ id, label, path: LIBRARY_PATHS[id] }));

/// App-scoped destinations, in contract order. Drawn as a separate group: the
/// boundary between "this library" and "this app" is a real distinction and
/// navigation should show it.
export const APP_NAV_ITEMS: NavigationItem[] = FRONTEND_CONTRACT.app_navigation.map(({ id, label }) => ({ id, label, path: UNIVERSAL_PATHS[id] }));

const UNIVERSAL_IDS: readonly Destination[] = FRONTEND_CONTRACT.app_navigation.map(({ id }) => id);

export function isUniversalDestination(destination: Destination): destination is UniversalDestination {
  return UNIVERSAL_IDS.includes(destination);
}
