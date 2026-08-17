import type { LibraryEntry } from "../types";

/**
 * Passive catalogue selection shared by the root shell and library host.
 * Backend loading and fallback policy stay in App, matching native's split
 * between LibraryState and the controller that owns it.
 */
export class LibraryStateStore {
  entries = $state<LibraryEntry[]>([]);
  activeLibraryId = $state("");

  get active(): LibraryEntry | undefined {
    return this.entries.find(({ library_id }) => library_id === this.activeLibraryId);
  }

  get configured(): boolean {
    return this.entries.length > 0;
  }

  setEntries(entries: LibraryEntry[]): void {
    this.entries = entries;
  }

  select(libraryId: string): void {
    this.activeLibraryId = libraryId;
  }

  clearSelection(): void {
    this.activeLibraryId = "";
  }
}
