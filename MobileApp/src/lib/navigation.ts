import type { AppFeature } from "./bindings";
import { FRONTEND_CONTRACT } from "./types";

export type Destination = AppFeature;

export interface NavigationItem {
  id: Destination;
  label: string;
  path: string;
}

const PATHS: Record<AppFeature, string> = {
  home: "M3 10.8 12 3l9 7.8v9.7a.5.5 0 0 1-.5.5H15v-6H9v6H3.5a.5.5 0 0 1-.5-.5z",
  library: "M4 4.5A2.5 2.5 0 0 1 6.5 2H20v17H6.5A2.5 2.5 0 0 0 4 21.5zm0 17A2.5 2.5 0 0 1 6.5 19H20v3H6.5A2.5 2.5 0 0 1 4 19.5z",
  authors: "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2m7-10a4 4 0 1 0 0-8 4 4 0 0 0 0 8m13 10v-2a4 4 0 0 0-3-3.87m-2-12a4 4 0 0 1 0 7.75",
  search: "m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0",
  store: "M4 9h16l-1 12H5zM3 9l2-5h14l2 5M9 9V6a3 3 0 0 1 6 0v3",
};

export const NAV_ITEMS: NavigationItem[] = FRONTEND_CONTRACT.navigation.map(({ id, label }) => ({ id, label, path: PATHS[id] }));
