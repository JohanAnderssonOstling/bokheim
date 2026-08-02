import { FRONTEND_CONTRACT as GENERATED_FRONTEND_CONTRACT } from "./bindings";
import type { AccountStatus as GeneratedAccountStatus, ApplicationTheme as GeneratedApplicationTheme, AuthorFolderBookSummaries_Serialize, AuthorSummary as GeneratedAuthorSummary, BookSummary_Serialize, BrowsingPreferencePatch as GeneratedBrowsingPreferencePatch, BrowsingPreferences as GeneratedBrowsingPreferences, BrowsingPreferencesContract as GeneratedBrowsingPreferencesContract, Dir as GeneratedDirectory, DirSummaryContent_Serialize, DownloadState as GeneratedDownloadState, FolderDownloadStatus as GeneratedFolderDownloadStatus, FrontendContract as GeneratedFrontendContract, LibraryEntry as GeneratedLibraryEntry, LibrarySyncStatus as GeneratedLibrarySyncStatus, LibraryTransfers as GeneratedLibraryTransfers, NetworkStatus as GeneratedNetworkStatus, ReaderFlow as GeneratedReaderFlow, ReaderFontFamily as GeneratedReaderFontFamily, ReaderPreferencePatch as GeneratedReaderPreferencePatch, ReaderPreferences as GeneratedReaderPreferences, ReaderPreferencesContract as GeneratedReaderPreferencesContract, ReaderTextAlignment as GeneratedReaderTextAlignment, ReaderTheme as GeneratedReaderTheme, TransferPreferencePatch as GeneratedTransferPreferencePatch, TransferPreferences as GeneratedTransferPreferences, UiFontFamily as GeneratedUiFontFamily } from "./bindings";

export type LibraryEntry = GeneratedLibraryEntry;
export type AccountStatus = GeneratedAccountStatus;
export type LibrarySyncStatus = GeneratedLibrarySyncStatus;
export type LibraryTransfers = GeneratedLibraryTransfers;
export type NetworkStatus = GeneratedNetworkStatus;
export type TransferPreferences = GeneratedTransferPreferences;
export type TransferPreferencePatch = GeneratedTransferPreferencePatch;
export type BookSummary = BookSummary_Serialize;
export type DownloadState = GeneratedDownloadState;
export type FolderDownloadStatus = GeneratedFolderDownloadStatus;
export type LibraryDirectory = GeneratedDirectory;
export type DirectoryContent = DirSummaryContent_Serialize;
export type AuthorSummary = GeneratedAuthorSummary;
export type AuthorFolderBooks = AuthorFolderBookSummaries_Serialize;
export type ApplicationTheme = GeneratedApplicationTheme;
export type BrowsingPreferences = GeneratedBrowsingPreferences;
export type BrowsingPreferencePatch = GeneratedBrowsingPreferencePatch;
export type BrowsingPreferencesContract = GeneratedBrowsingPreferencesContract;
export type UiFontFamily = GeneratedUiFontFamily;
export type ReaderTheme = GeneratedReaderTheme;
export type ReaderFlow = GeneratedReaderFlow;
export type ReaderFontFamily = GeneratedReaderFontFamily;
export type ReaderTextAlignment = GeneratedReaderTextAlignment;
export type ReaderPreferences = GeneratedReaderPreferences;
export type ReaderPreferencePatch = GeneratedReaderPreferencePatch;
export type ReaderPreferencesContract = GeneratedReaderPreferencesContract;
export type FrontendContract = GeneratedFrontendContract;
export const FRONTEND_CONTRACT = GENERATED_FRONTEND_CONTRACT;
export const BROWSING_PREFERENCES_CONTRACT = FRONTEND_CONTRACT.browsing_preferences;
export const DEFAULT_BROWSING_PREFERENCES: BrowsingPreferences = { ...BROWSING_PREFERENCES_CONTRACT.defaults };
export const READER_PREFERENCES_CONTRACT = FRONTEND_CONTRACT.reader_preferences;
export const DEFAULT_READER_PREFERENCES: ReaderPreferences = { ...READER_PREFERENCES_CONTRACT.defaults };

export interface ReaderBook {
  key: string;
  file: File;
  title: string;
  author?: string;
  initialCfi?: string | null;
  saveCfi: (cfi: string) => void | Promise<void>;
}

export interface FoliateTocItem {
  label?: string;
  href?: string | null;
  subitems?: FoliateTocItem[] | null;
}

export interface FoliateBook {
  dir?: "ltr" | "rtl";
  toc?: FoliateTocItem[] | null;
}

export interface FoliateSearchExcerpt {
  pre: string;
  match: string;
  post: string;
}

export interface FoliateSearchMatch {
  cfi: string;
  excerpt: FoliateSearchExcerpt;
}

export interface FoliateSearchGroup {
  label?: string;
  subitems: FoliateSearchMatch[];
}

export interface FoliateRelocateDetail {
  cfi?: string;
  fraction?: number;
  tocItem?: {
    href?: string;
    label?: string;
  };
  location?: {
    current?: number;
    next?: number;
    total?: number;
  };
}

export interface FoliateRenderer extends HTMLElement {
  setStyles?: (css: string) => void;
}

export interface FoliateView extends HTMLElement {
  book?: FoliateBook;
  renderer?: FoliateRenderer;
  history?: EventTarget & {
    back: () => void;
    forward: () => void;
    readonly canGoBack: boolean;
    readonly canGoForward: boolean;
  };
  readonly isFixedLayout?: boolean;
  open: (file: File | Blob | string) => Promise<void>;
  init: (options?: { lastLocation?: string; showTextStart?: boolean }) => Promise<void>;
  close: () => void;
  goLeft: () => Promise<void> | void;
  goRight: () => Promise<void> | void;
  goTo: (target: string | number | { fraction: number }) => Promise<unknown>;
  goToFraction: (fraction: number) => Promise<void> | void;
  select: (target: string) => Promise<void>;
  search: (options: {
    query: string;
    matchCase?: boolean;
    matchDiacritics?: boolean;
    matchWholeWords?: boolean;
  }) => AsyncGenerator<FoliateSearchGroup | { progress: number } | string>;
  clearSearch: () => void;
}
