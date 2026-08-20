import { commands } from "../app/commands";
import type {
  BookCardData,
  DownloadState,
  FolderDownloadStatus,
  LibraryDirectory,
} from "../shared/types";
import { toBackendDirectoryId } from "./directory-id";

export function createDownloadController(
  getLibraryId: () => string,
  setError: (message: string) => void,
) {
  let bookStates = $state<Record<string, DownloadState>>({});
  let folderStates = $state<Record<string, FolderDownloadStatus>>({});
  let activeFolders = $state<Record<string, boolean>>({});

  function message(reason: unknown) {
    return reason instanceof Error ? reason.message : String(reason);
  }

  function mergeBooks(books: BookCardData[]) {
    const next = { ...bookStates };
    for (const book of books)
      next[book.content_hash] = book.downloaded
        ? "Downloaded"
        : book.download_requested
          ? "Queued"
          : "NotDownloaded";
    bookStates = next;
  }

  function replaceFolders(folders: LibraryDirectory[]) {
    folderStates = Object.fromEntries(
      folders
        .filter((folder) => folder.id !== "__root__")
        .map((folder) => [
          folder.id,
          {
            dir_id: folder.id,
            downloaded_books: folder.downloaded_book_count,
            total_books: folder.book_count,
          },
        ]),
    );
  }

  function setBookState(contentHash: string, state: DownloadState) {
    bookStates = { ...bookStates, [contentHash]: state };
  }

  function setFolderState(dirId: string, state: FolderDownloadStatus) {
    folderStates = { ...folderStates, [dirId]: state };
  }

  async function downloadBook(book: BookCardData) {
    if (bookStates[book.content_hash] === "Downloaded") return;
    setBookState(book.content_hash, "Queued");
    try {
      const state = await commands.downloadBook(getLibraryId(), book.content_hash);
      if (bookStates[book.content_hash] !== "Downloaded")
        setBookState(book.content_hash, state);
    } catch (reason) {
      setError(message(reason));
      throw reason;
    }
  }

  async function removeLocalBookCopy(book: BookCardData) {
    if (
      !window.confirm(
        `Remove the downloaded copy of “${book.title || "Untitled"}” from this device? You can download it again later.`,
      )
    )
      return;
    try {
      setError("");
      setBookState(
        book.content_hash,
        await commands.removeLocalBookCopy(getLibraryId(), book.content_hash),
      );
    } catch (reason) {
      setError(message(reason));
    }
  }

  async function downloadFolder(folder: LibraryDirectory) {
    if (activeFolders[folder.id]) return;
    activeFolders = { ...activeFolders, [folder.id]: true };
    try {
      setError("");
      folderStates = {
        ...folderStates,
        [folder.id]: await commands.downloadFolder(
          getLibraryId(),
          toBackendDirectoryId(folder.id),
        ),
      };
    } catch (reason) {
      setError(message(reason));
    } finally {
      activeFolders = { ...activeFolders, [folder.id]: false };
    }
  }

  return {
    get bookStates() {
      return bookStates;
    },
    get folderStates() {
      return folderStates;
    },
    get activeFolders() {
      return activeFolders;
    },
    mergeBooks,
    replaceFolders,
    setBookState,
    setFolderState,
    downloadBook,
    removeLocalBookCopy,
    downloadFolder,
    reset: () => {
      bookStates = {};
      folderStates = {};
      activeFolders = {};
    },
  };
}
