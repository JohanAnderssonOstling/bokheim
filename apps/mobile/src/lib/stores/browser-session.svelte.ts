import type { LibraryFeature } from "../bindings";
import { defaultLibraryPlace, type BrowseUiSession, type LibraryPlace, type PlaceLevel } from "../ui-session";

/** Reactive owner of the per-library places shared with the native browser. */
export class BrowserSessionStore {
  places = $state<Record<string, LibraryPlace>>({});

  restore(session: BrowseUiSession): void {
    if (!placesEqual(this.places, session.places)) this.places = session.places;
  }

  place(libraryId: string): LibraryPlace {
    return this.places[libraryId] ?? defaultLibraryPlace();
  }

  remember(libraryId: string, route: LibraryFeature, folderPath: PlaceLevel[], subjectPath: PlaceLevel[]): void {
    const previous = this.place(libraryId);
    const next: LibraryPlace = {
      route,
      folder_path: route === "folders" ? [...folderPath] : [...previous.folder_path],
      subject_path: route === "subjects" ? [...subjectPath] : [...previous.subject_path],
    };
    if (placeEqual(previous, next)) return;
    this.places = {
      ...this.places,
      [libraryId]: next,
    };
  }

  snapshot(activeLibraryId?: string): BrowseUiSession {
    return { kind: "browse", active_library_id: activeLibraryId || undefined, places: this.places };
  }
}

function pathEqual(left: PlaceLevel[], right: PlaceLevel[]): boolean {
  return left.length === right.length && left.every((level, index) => level.id === right[index]?.id && level.name === right[index]?.name);
}

function placeEqual(left: LibraryPlace, right: LibraryPlace): boolean {
  return left.route === right.route && pathEqual(left.folder_path, right.folder_path) && pathEqual(left.subject_path, right.subject_path);
}

function placesEqual(left: Record<string, LibraryPlace>, right: Record<string, LibraryPlace>): boolean {
  const leftIds = Object.keys(left);
  const rightIds = Object.keys(right);
  return leftIds.length === rightIds.length && leftIds.every((libraryId) => {
    const rightPlace = right[libraryId];
    return rightPlace !== undefined && placeEqual(left[libraryId], rightPlace);
  });
}
