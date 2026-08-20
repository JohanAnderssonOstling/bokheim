import { commands } from "../app/commands";

/**
 * Library-facing command boundary for the UI.
 *
 * Components should depend on this module instead of selecting individual
 * native/web commands. `commands` remains the platform adapter underneath.
 */
export const libraryApi = {
  list: () => commands.getLibraries(),
  home: (libraryId: string, limit = 100) => commands.getLibraryHome(libraryId, limit),
  folder: (
    libraryId: string,
    dirId: string,
    fileType: "all" | "book" | "pdf" | "audiobook" | null = null,
  ) => commands.getLibraryFolder(libraryId, dirId, fileType),
  subjects: (
    libraryId: string,
    subjectPath: string | null,
    fileType: "all" | "book" | "pdf" | "audiobook" | null = null,
  ) => commands.getLibrarySubjects(libraryId, subjectPath, fileType),
  search: (libraryId: string, query: string, limit = 100) =>
    commands.searchLibrary(libraryId, query, limit),
  searchFolder: (
    libraryId: string,
    dirId: string,
    query: string,
    fileType: "all" | "book" | "pdf" | "audiobook" | null = null,
  ) => commands.searchLibraryFolder(libraryId, dirId, query, 100, fileType),
  searchSubject: (
    libraryId: string,
    subjectPath: string | null,
    query: string,
    fileType: "all" | "book" | "pdf" | "audiobook" | null = null,
  ) => commands.searchLibrarySubject(libraryId, subjectPath, query, 100, fileType),
  book: (libraryId: string, contentHash: string) =>
    commands.getBookDetail(libraryId, contentHash),
  authors: (
    libraryId: string,
    selectedAuthorId: string | null,
    sort: "name" | "book_count" | null = null,
  ) => commands.getLibraryAuthors(libraryId, selectedAuthorId, sort),
  thumbnail: (libraryId: string, contentHash: string) =>
    commands.getBookThumbnail(libraryId, contentHash),
  download: (libraryId: string, contentHash: string) =>
    commands.downloadBook(libraryId, contentHash),
};
