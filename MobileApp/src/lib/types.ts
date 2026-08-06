import { FRONTEND_CONTRACT as GENERATED_FRONTEND_CONTRACT } from "./bindings";
import type { AccountContract as GeneratedAccountContract, AccountSessionSummary as GeneratedAccountSessionSummary, AccountStatus as GeneratedAccountStatus, ApplicationTheme as GeneratedApplicationTheme, AuthorSummary as GeneratedAuthorSummary, BookCardRow, BookLink as GeneratedBookLink, BrowsingContentContract as GeneratedBrowsingContentContract, BrowsingPreferencePatch as GeneratedBrowsingPreferencePatch, BrowsingPreferences as GeneratedBrowsingPreferences, BrowsingPreferencesContract as GeneratedBrowsingPreferencesContract, DownloadState as GeneratedDownloadState, FolderDownloadStatus as GeneratedFolderDownloadStatus, FrontendContract as GeneratedFrontendContract, LibraryEntry as GeneratedLibraryEntry, LibraryFolderEntry, LibraryFolderView, LibrarySyncStatus as GeneratedLibrarySyncStatus, LibraryTransfers as GeneratedLibraryTransfers, NetworkStatus as GeneratedNetworkStatus, ReaderFlow as GeneratedReaderFlow, ReaderFontFamily as GeneratedReaderFontFamily, ReaderNavigationAction as GeneratedReaderNavigationAction, ReaderNavigationContract as GeneratedReaderNavigationContract, ReaderNavigationKey as GeneratedReaderNavigationKey, ReaderPreferencePatch as GeneratedReaderPreferencePatch, ReaderPreferences as GeneratedReaderPreferences, ReaderPreferencesContract as GeneratedReaderPreferencesContract, ReaderTextAlignment as GeneratedReaderTextAlignment, ReaderTheme as GeneratedReaderTheme, SettingsContract as GeneratedSettingsContract, TransferPreferencePatch as GeneratedTransferPreferencePatch, TransferPreferences as GeneratedTransferPreferences, UiFontFamily as GeneratedUiFontFamily, WebPreferenceProfile as GeneratedWebPreferenceProfile } from "./bindings";

export type LibraryEntry = GeneratedLibraryEntry;
export type AccountStatus = GeneratedAccountStatus;
export type AccountSessionSummary = GeneratedAccountSessionSummary;
export type AccountContract = GeneratedAccountContract;
export type SettingsContract = GeneratedSettingsContract;
export type LibrarySyncStatus = GeneratedLibrarySyncStatus;
export type LibraryTransfers = GeneratedLibraryTransfers;
export type NetworkStatus = GeneratedNetworkStatus;
export type TransferPreferences = GeneratedTransferPreferences;
export type TransferPreferencePatch = GeneratedTransferPreferencePatch;
export type BookCardData = BookCardRow;
export type BookLink = GeneratedBookLink;
export type BrowsingContentContract = GeneratedBrowsingContentContract;
export type DownloadState = GeneratedDownloadState;
export type FolderDownloadStatus = GeneratedFolderDownloadStatus;
export type LibraryDirectory = LibraryFolderEntry;
export type DirectoryContent = LibraryFolderView;
export type AuthorSummary = GeneratedAuthorSummary;
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
export type ReaderNavigationAction = GeneratedReaderNavigationAction;
export type ReaderNavigationKey = GeneratedReaderNavigationKey;
export type ReaderNavigationContract = GeneratedReaderNavigationContract;
export type WebPreferenceProfile = GeneratedWebPreferenceProfile;
export type FrontendContract = GeneratedFrontendContract;
export const FRONTEND_CONTRACT = GENERATED_FRONTEND_CONTRACT;
export const BOOK_ACTIONS_CONTRACT = FRONTEND_CONTRACT.book_actions;
export const BROWSING_LAYOUT_CONTRACT = FRONTEND_CONTRACT.browsing_layout;
export const LIBRARY_ACTIONS_CONTRACT = FRONTEND_CONTRACT.library_actions;
export const STORE_CONTRACT = FRONTEND_CONTRACT.store;
export const BROWSING_STATES_CONTRACT = FRONTEND_CONTRACT.browsing_states;
export const ACCOUNT_CONTRACT = FRONTEND_CONTRACT.account;
export const SETTINGS_CONTRACT = FRONTEND_CONTRACT.settings;
export const BROWSING_CONTENT_CONTRACT = { ...FRONTEND_CONTRACT.browsing_content, breadcrumb_item_max_characters: 24 } as const;
export const BROWSING_PREFERENCES_CONTRACT = FRONTEND_CONTRACT.browsing_preferences;
export const DEFAULT_BROWSING_PREFERENCES: BrowsingPreferences = { ...BROWSING_PREFERENCES_CONTRACT.defaults };
export const READER_PREFERENCES_CONTRACT = FRONTEND_CONTRACT.reader_preferences;
export const READER_SETTINGS_PRESENTATION_CONTRACT = FRONTEND_CONTRACT.reader_settings;
export const READER_NAVIGATION_CONTRACT = FRONTEND_CONTRACT.reader_navigation;
export const WEB_PREFERENCE_PROFILES_CONTRACT = FRONTEND_CONTRACT.web_preference_profiles;
export const DEFAULT_READER_PREFERENCES: ReaderPreferences = { ...READER_PREFERENCES_CONTRACT.defaults };

export interface ReaderBook {
  key: string;
  bookId?: string;
  file: File;
  title: string;
  author?: string;
  initialCfi?: string | null;
  saveCfi: (cfi: string, progress: number) => void | Promise<void>;
  loadAnnotations?: () => Promise<ReaderAnnotation[]>;
  upsertAnnotation?: (annotation: ReaderAnnotation) => Promise<void>;
  deleteAnnotation?: (annotationId: string, modifiedAt: number) => Promise<void>;
  loadBookmarks?: () => Promise<ReaderBookmark[]>;
  upsertBookmark?: (bookmark: ReaderBookmark) => Promise<void>;
  deleteBookmark?: (bookmarkId: string, modifiedAt: number) => Promise<void>;
}

export type ReaderAnnotationStyle = "highlight" | "underline" | "squiggly" | "strikethrough";

export interface PdfAnnotationRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export type AnnotationAnchor =
  | { kind: "epub_cfi"; cfi: string }
  | { kind: "pdf"; page_index: number; rects: PdfAnnotationRect[]; fallback_cfi: string | null };

export interface ReaderAnnotation {
  id: string;
  book_id: string;
  anchor: AnnotationAnchor;
  exact_text: string;
  prefix: string | null;
  suffix: string | null;
  style: ReaderAnnotationStyle;
  color: string;
  note: string;
  created_at: number;
  modified_at: number;
}

export interface ReaderBookmark {
  id: string;
  book_id: string;
  cfi: string;
  label: string;
  created_at: number;
  modified_at: number;
}

export interface FoliateTocItem {
  id?: number;
  label?: string;
  href?: string | null;
  subitems?: FoliateTocItem[] | null;
}

export interface FoliateBook {
  dir?: "ltr" | "rtl";
  toc?: FoliateTocItem[] | null;
  landmarks?: FoliateTocItem[] | null;
  pageList?: FoliateTocItem[] | null;
  metadata?: {
    title?: unknown;
    author?: unknown;
    language?: unknown;
    publisher?: unknown;
    description?: unknown;
    identifier?: unknown;
    published?: unknown;
  };
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
  section?: {
    current?: number;
    total?: number;
  };
  time?: {
    section?: number;
    total?: number;
  };
  pageItem?: {
    label?: string;
  };
}

export interface FoliateRenderer extends HTMLElement {
  setStyles?: (css: string) => void;
  getContents?: () => Array<{ doc: Document; index: number }>;
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
  prev: (distance?: number) => Promise<void>;
  next: (distance?: number) => Promise<void>;
  goTo: (target: string | number | { fraction: number }) => Promise<unknown>;
  goToFraction: (fraction: number) => Promise<void> | void;
  select: (target: string) => Promise<void>;
  search: (options: {
    query: string;
    index?: number;
    matchCase?: boolean;
    matchDiacritics?: boolean;
    matchWholeWords?: boolean;
  }) => AsyncGenerator<FoliateSearchGroup | FoliateSearchMatch | { progress: number } | string>;
  clearSearch: () => void;
  getCFI: (index: number, range?: Range) => string;
  addAnnotation: (annotation: { value: string; color?: string; [key: string]: unknown }) => Promise<{ index: number; label: string } | undefined>;
  deleteAnnotation: (annotation: { value: string; [key: string]: unknown }) => Promise<unknown>;
  showAnnotation: (annotation: { value: string; [key: string]: unknown }) => Promise<void>;
  initTTS: (granularity?: "word" | "sentence", highlight?: (range: Range) => void) => Promise<void>;
  tts?: {
    start: () => string | undefined;
    resume: () => string | undefined;
    prev: (paused?: boolean) => string | undefined;
    next: (paused?: boolean) => string | undefined;
  };
  mediaOverlay?: { start: (index: number) => Promise<unknown>; pause: () => void; resume: () => void; stop: () => void; prev: () => void; next: () => void; setRate: (rate: number) => void };
  startMediaOverlay?: () => Promise<unknown>;
}
