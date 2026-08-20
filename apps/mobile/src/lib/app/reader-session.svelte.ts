import { tick } from "svelte";
import { convertFileSrc } from "@tauri-apps/api/core";

import { commands } from "./commands";
import { isWebApp } from "./platform";
import { libraryReaderBookKey, readerHref } from "./navigation";
import { navigate } from "./navigate";
import { measureInvoke } from "./performance";
import { createDownloadController } from "../library/download-controller.svelte";
import { getThumbnailUrl } from "../library/thumbnail-cache";
import type { MobileAudiobookSession } from "../reader/audiobook";
import type { NativeAudiobookState } from "../reader/native-audiobook";
import type {
  BookCardData,
  DownloadState,
  ReaderBook,
  ReaderBookmark,
} from "../shared/types";
import { closeReaderBookSource, parseAudiobookTarget } from "../../routes/reader/reader";

export type ReaderKind = "epub" | "pdf";

function errorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

/**
 * Everything between "open this book" and a reader route being able to render.
 *
 * Deciding *what* to open — a queued download, a remote session, an audiobook
 * rather than a reader — happens before navigating, because it decides which
 * URL to go to. Opening the file itself happens in the reader route, which is
 * the only thing that renders it.
 *
 * One instance for the application: the audiobook player outlives any route,
 * and a download a reader open is waiting on outlives the page that asked.
 */
class ReaderSession {
  #audiobook = $state<MobileAudiobookSession | null>(null);
  #openingBookId = $state("");
  #error = $state("");

  #audiobookRequest = 0;
  #suppressNativeRestore = false;
  #restoringNativeKey = "";
  #pendingOpen: { book: BookCardData; target?: string } | null = null;
  #libraryId = "";
  #readerBookId = "";

  readonly downloads = createDownloadController(
    () => this.#libraryId,
    (message) => (this.#error = message),
  );

  get audiobookSession(): MobileAudiobookSession | null {
    return this.#audiobook;
  }

  /** The book a tap is currently resolving, so its card can show progress. */
  get openingBookId(): string {
    return this.#openingBookId;
  }

  get error(): string {
    return this.#error;
  }

  clearError(): void {
    this.#error = "";
  }

  useLibrary(libraryId: string): void {
    if (libraryId === this.#libraryId) return;
    this.#libraryId = libraryId;
    // A queued download belongs to the library that queued it.
    this.downloads.reset();
    this.#pendingOpen = null;
  }

  /**
   * Resolves a book to the surface that can show it and goes there: a reader
   * route for a publication, the player in place for an audiobook.
   */
  async openBook(libraryId: string, book: BookCardData, target?: string): Promise<void> {
    this.#openingBookId = book.content_hash;
    this.#error = "";
    try {
      if (book.audiobook) {
        this.#pendingOpen = null;
        await this.#openAudiobook(libraryId, book, target);
        return;
      }
      if (!(await this.#ensureAvailable(libraryId, book, target))) return;
      this.#pendingOpen = null;
      const kind = await this.#readerKind(libraryId, book.content_hash);
      await navigate(readerHref(kind, book.content_hash, libraryId));
    } catch (reason) {
      this.#error = errorMessage(reason);
    } finally {
      if (this.#pendingOpen?.book.content_hash === book.content_hash)
        this.#pendingOpen = null;
      this.#openingBookId = "";
    }
  }

  /**
   * True once the book can be opened locally. A book that has to be fetched
   * first queues a download and reports back through `handleDownloadStatus`.
   */
  async #ensureAvailable(
    libraryId: string,
    book: BookCardData,
    target?: string,
  ): Promise<boolean> {
    const state =
      this.downloads.bookStates[book.content_hash] ??
      (book.downloaded
        ? "Downloaded"
        : book.download_requested
          ? "Queued"
          : "NotDownloaded");
    if (state === "Downloaded" || isWebApp) return true;
    if (!libraryId) throw new Error("No library is selected");
    // The whole file has to be here before it can be read: the reader opens
    // publications from local storage only.
    this.#pendingOpen = { book, target };
    await measureInvoke("download_book", () => this.downloads.downloadBook(book));
    return this.downloads.bookStates[book.content_hash] === "Downloaded";
  }

  async #readerKind(libraryId: string, bookId: string): Promise<ReaderKind> {
    const fileName = await commands.getBookFileName(libraryId, bookId).catch(() => "");
    return /\.pdf$/i.test(fileName) ? "pdf" : "epub";
  }

  /**
   * Opens the file a reader route is showing.
   *
   * The route owns the result: it renders it and closes it again on the way
   * out. Nothing is cached here, so a reader that is mounted twice for the
   * same book opens it twice, which is what keeps the two independent.
   */
  async openForRoute(
    libraryId: string,
    bookId: string,
    target?: string,
  ): Promise<ReaderBook> {
    if (!libraryId) throw new Error("No library is selected");
    this.useLibrary(libraryId);
    this.#readerBookId = bookId;
    const detail = await commands.getBookDetail(libraryId, bookId).catch(() => null);
    return this.#localBook(libraryId, bookId, detail?.book, target);
  }

  async deleteAnnotation(annotationId: string): Promise<void> {
    if (!this.#libraryId) throw new Error("No library is selected");
    await commands.deleteAnnotation(this.#libraryId, annotationId, Date.now());
  }

  async deleteBookmark(bookmarkId: string): Promise<void> {
    if (!this.#libraryId) throw new Error("No library is selected");
    await commands.deleteBookmark(this.#libraryId, bookmarkId, Date.now());
  }

  async addBookmark(cfi: string, label: string): Promise<ReaderBookmark> {
    if (!this.#libraryId || !this.#readerBookId) throw new Error("No book is open");
    const now = Date.now();
    const bookmark: ReaderBookmark = {
      id: crypto.randomUUID(),
      content_hash: this.#readerBookId,
      cfi,
      label,
      created_at: now,
      modified_at: now,
    };
    await commands.createBookmark(this.#libraryId, bookmark);
    return bookmark;
  }

  async closeForRoute(book: ReaderBook | null): Promise<void> {
    if (!book) return;
    if (typeof book.file === "string" && book.file.startsWith("blob:"))
      URL.revokeObjectURL(book.file);
    await closeReaderBookSource(book.file, commands).catch(() => undefined);
  }

  async #localBook(
    libraryId: string,
    bookId: string,
    card: BookCardData | undefined,
    target?: string,
  ): Promise<ReaderBook> {
    let file: ReaderBook["file"] | null = null;
    try {
      const [fileName, initialCfi, initialPdfPosition] = await Promise.all([
        commands.getBookFileName(libraryId, bookId),
        measureInvoke("reading_position", () =>
          commands.getBookReadingPosition(libraryId, bookId),
        ),
        measureInvoke("pdf_reading_position", () =>
          commands.getPdfReadingPosition(libraryId, bookId),
        ),
      ]);
      const pdf = /\.pdf$/i.test(fileName);
      const initialPdfPage = initialPdfPosition?.[0] ?? null;
      const initialPdfFullPagePosition = initialPdfPosition?.[1] ?? null;
      // PDFs are read over an IPC range-read session instead of the asset
      // protocol: some Android WebViews can't give the PDF.js worker direct
      // access to Tauri's asset:// scheme for byte ranges, which left PDFs
      // unable to open on Android.
      file =
        !isWebApp && pdf
          ? await (async () => {
              const session = await measureInvoke("open_pdf_range_session", () =>
                commands.openPdfRangeSession(libraryId, bookId),
              );
              return {
                kind: "native_pdf_range",
                sessionId: session.id,
                size: session.size,
              } as const;
            })()
          : await (async () => {
              const localPath = await measureInvoke("open_book", () =>
                commands.openBook(libraryId, bookId),
              );
              return isWebApp ? localPath : convertFileSrc(localPath);
            })();
      return {
        key: libraryReaderBookKey(libraryId, bookId),
        bookId,
        file,
        fileName,
        title: card?.title || fileName.replace(/\.[^.]+$/, "") || "Untitled",
        author: card?.author,
        initialCfi: pdf ? null : initialCfi,
        initialPdfPage: pdf ? initialPdfPage : null,
        initialPdfFullPagePosition:
          pdf && initialPdfFullPagePosition !== null
            ? Math.max(0, Number(initialPdfFullPagePosition))
            : null,
        initialTarget: target,
        ...this.#persistence(libraryId, bookId),
        ...(isWebApp ? {} : this.#annotations(libraryId, bookId)),
      };
    } catch (reason) {
      if (file) await closeReaderBookSource(file, commands);
      throw reason;
    }
  }

  #persistence(libraryId: string, bookId: string) {
    return {
      saveCfi: async (cfi: string, progress: number) => {
        await measureInvoke("update_reading_position", () =>
          commands.updateReadingPosition(libraryId, bookId, cfi, progress),
        );
      },
      savePdfPage: async (
        pageIndex: number,
        fullPagePosition: number,
        _progress: number,
      ) => {
        await measureInvoke("update_pdf_reading_position", () =>
          commands.updatePdfReadingPosition(
            libraryId,
            bookId,
            pageIndex,
            fullPagePosition,
          ),
        );
      },
    };
  }

  #annotations(libraryId: string, bookId: string) {
    return {
      loadAnnotations: () => commands.getAnnotations(libraryId, bookId),
      upsertAnnotation: async (
        annotation: Parameters<typeof commands.upsertAnnotation>[1],
      ) => {
        await commands.upsertAnnotation(libraryId, annotation);
      },
      deleteAnnotation: async (annotationId: string, modifiedAt: number) => {
        await commands.deleteAnnotation(libraryId, annotationId, modifiedAt);
      },
      loadBookmarks: () => commands.getBookmarks(libraryId, bookId),
      createBookmark: async (bookmark: Parameters<typeof commands.createBookmark>[1]) => {
        await commands.createBookmark(libraryId, bookmark);
      },
      deleteBookmark: async (bookmarkId: string, deletedAt: number) => {
        await commands.deleteBookmark(libraryId, bookmarkId, deletedAt);
      },
    };
  }

  async #openAudiobook(libraryId: string, book: BookCardData, target?: string) {
    if (!libraryId) throw new Error("No library is selected");
    if (
      this.#audiobook?.libraryId === libraryId &&
      this.#audiobook.contentHash === book.content_hash
    )
      return;
    const request = ++this.#audiobookRequest;
    this.#suppressNativeRestore = false;
    const [localPath, initialPositionMs, coverUrl, chapters] = await Promise.all([
      measureInvoke("open_audiobook", () =>
        commands.openBook(libraryId, book.content_hash),
      ),
      commands.getAudiobookReadingPosition(libraryId, book.content_hash),
      getThumbnailUrl(libraryId, book.content_hash),
      commands.getAudiobookChapters(libraryId, book.content_hash),
    ]);
    if (request !== this.#audiobookRequest) return;
    const next: MobileAudiobookSession = {
      key: `${libraryId}:${book.content_hash}:${request}`,
      libraryId,
      contentHash: book.content_hash,
      source: isWebApp ? localPath : convertFileSrc(localPath),
      nativeSource: localPath,
      nativeKey: `${libraryId}:${book.content_hash}`,
      title: book.title || "Untitled audiobook",
      author: book.author,
      coverUrl: coverUrl ?? "",
      initialPositionMs: parseAudiobookTarget(target) ?? initialPositionMs ?? 0,
      expectedDurationMs: book.audiobook_duration_ms,
      chapters,
      savePosition: async (positionMs, progress) => {
        await commands.updateAudiobookReadingPosition(
          libraryId,
          book.content_hash,
          positionMs,
          progress,
        );
      },
    };
    if (this.#audiobook) {
      this.#audiobook = null;
      await tick();
    }
    if (request === this.#audiobookRequest) this.#audiobook = next;
  }

  closeAudiobook(): void {
    this.#audiobookRequest += 1;
    this.#suppressNativeRestore = true;
    this.#audiobook = null;
  }

  async restoreNativeAudiobook(state: NativeAudiobookState): Promise<void> {
    if (!state.active) {
      this.#suppressNativeRestore = false;
      return;
    }
    if (
      this.#suppressNativeRestore ||
      !state.key ||
      !state.libraryId ||
      !state.contentHash ||
      !state.sourcePath
    )
      return;
    if (this.#audiobook?.nativeKey === state.key) return;
    const nativeKey = state.key;
    if (this.#restoringNativeKey === nativeKey) return;
    this.#restoringNativeKey = nativeKey;
    try {
      const [coverUrl, chapters] = await Promise.all([
        getThumbnailUrl(state.libraryId, state.contentHash).catch(() => null),
        commands.getAudiobookChapters(state.libraryId, state.contentHash).catch(() => []),
      ]);
      const current = window.BokheimAndroid?.getAudiobookState?.();
      if (current) {
        try {
          const latest = JSON.parse(current) as { active?: boolean; key?: string };
          if (!latest.active || latest.key !== nativeKey) return;
        } catch {
          return;
        }
      }
      const libraryId = state.libraryId;
      const contentHash = state.contentHash;
      this.#audiobook = {
        key: `${nativeKey}:restored`,
        nativeKey,
        libraryId,
        contentHash,
        source: convertFileSrc(state.sourcePath),
        nativeSource: state.sourcePath,
        title: state.title || "Untitled audiobook",
        author: state.author || "",
        coverUrl: coverUrl ?? "",
        initialPositionMs: state.positionMs,
        expectedDurationMs: state.durationMs,
        chapters,
        savePosition: async (positionMs, progress) => {
          await commands.updateAudiobookReadingPosition(
            libraryId,
            contentHash,
            positionMs,
            progress,
          );
        },
      };
    } finally {
      if (this.#restoringNativeKey === nativeKey) this.#restoringNativeKey = "";
    }
  }

  /**
   * A queued download can be what a reader open is waiting on, so finishing one
   * resumes that open.
   */
  handleDownloadStatus(contentHash: string, state: DownloadState): void {
    this.downloads.setBookState(contentHash, state);
    const pending = this.#pendingOpen;
    if (pending?.book.content_hash !== contentHash) return;
    if (state === "Downloaded" && this.#openingBookId !== contentHash) {
      this.#pendingOpen = null;
      queueMicrotask(
        () => void this.openBook(this.#libraryId, pending.book, pending.target),
      );
    } else if (state === "NotDownloaded") {
      this.#pendingOpen = null;
      this.#error = "Could not download the book.";
    }
  }

  get hasPendingOpen(): boolean {
    return this.#pendingOpen !== null;
  }

  retryPendingOpen(): Promise<void> {
    const pending = this.#pendingOpen;
    if (!pending) return Promise.resolve();
    return this.openBook(this.#libraryId, pending.book, pending.target);
  }
}

export const readerSession = new ReaderSession();
