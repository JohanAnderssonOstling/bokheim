import { commands } from "./commands";
import { isWebApp } from "./platform";
import { APP_NAME_LOWER } from "./application-name";
import { requestLibraryRefresh } from "./library-events";
import { measureInvoke } from "./performance";
import {
  loadWebBrowsingPreferencesFallback,
  saveWebBrowsingPreferencesFallback,
} from "./app-utils";
import {
  DEFAULT_BROWSING_PREFERENCES,
  DEFAULT_READER_PREFERENCES,
  type BrowsingPreferencePatch,
  type BrowsingPreferences,
  type ReaderPreferencePatch,
  type ReaderPreferences,
} from "../shared/types";

const READER_STORAGE_KEY = `${APP_NAME_LOWER}-reader-preferences`;

// Reader settings apply to the UI (and the on-screen renderer) instantly. Persisting to
// disk is debounced: a stepper held down or a slider dragged across several values would
// otherwise fsync a preferences file to disk on every intermediate tick, which is what made
// settings changes feel slow on Android's storage layer. Only the latest value per key is
// kept, since older intermediate values are moot once a newer one lands.
const READER_SAVE_DEBOUNCE_MS = 400;

function errorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

/**
 * The single owner of both preference documents.
 *
 * There is deliberately one instance for the whole application. The shell reads
 * browsing preferences to apply the theme, the settings and store pages write
 * them, and the reader edits its own — with a copy per page, saving in one
 * place left every other place showing the old value until a reload.
 */
class PreferenceStore {
  #browsing = $state<BrowsingPreferences>({ ...DEFAULT_BROWSING_PREFERENCES });
  #reader = $state<ReaderPreferences>({ ...DEFAULT_READER_PREFERENCES });
  #error = $state("");

  #browsingQueue = Promise.resolve();
  #browsingVersion = 0;
  #readerQueue = Promise.resolve();
  #readerVersion = 0;
  #pendingReaderPatches = new Map<string, ReaderPreferencePatch>();
  #readerDebounce: ReturnType<typeof setTimeout> | undefined;

  get browsing(): BrowsingPreferences {
    return this.#browsing;
  }

  get reader(): ReaderPreferences {
    return this.#reader;
  }

  /** The last failure to persist a browsing preference, for the settings page. */
  get error(): string {
    return this.#error;
  }

  async loadBrowsing(): Promise<void> {
    const local = loadWebBrowsingPreferencesFallback(isWebApp, APP_NAME_LOWER);
    try {
      const database = await measureInvoke("browsing_preferences", () =>
        commands.getBrowsingPreferences(),
      );
      // localStorage is the browser's durable fallback when SQLite has to run
      // in memory, so do not replace it with that database's fresh defaults.
      this.#browsing = local ?? database;
      saveWebBrowsingPreferencesFallback(isWebApp, APP_NAME_LOWER, this.#browsing);
    } catch {
      this.#browsing = local ?? { ...DEFAULT_BROWSING_PREFERENCES };
    }
  }

  async loadReader(): Promise<void> {
    try {
      this.#reader = await measureInvoke("reader_preferences", () =>
        commands.getReaderPreferences(),
      );
    } catch {
      const saved = localStorage.getItem(READER_STORAGE_KEY);
      if (!saved) return;
      try {
        this.#reader = { ...DEFAULT_READER_PREFERENCES, ...JSON.parse(saved) };
      } catch {
        this.#reader = { ...DEFAULT_READER_PREFERENCES };
      }
    }
  }

  /**
   * Applies a browsing preference everywhere at once. Pages that render library
   * content re-read on the refresh signal, so a change to the card layout shows
   * up without leaving the page.
   */
  saveBrowsing(patch: BrowsingPreferencePatch): Promise<void> {
    const previous = this.#browsing;
    this.#error = "";
    const next = {
      ...this.#browsing,
      [patch.preference]: patch.value,
    } as BrowsingPreferences;
    this.#browsing = next;
    saveWebBrowsingPreferencesFallback(isWebApp, APP_NAME_LOWER, next);
    requestLibraryRefresh();
    const version = ++this.#browsingVersion;
    this.#browsingQueue = this.#browsingQueue.then(async () => {
      try {
        const saved = await commands.updateBrowsingPreference(patch);
        if (version !== this.#browsingVersion) return;
        this.#browsing = saved;
        saveWebBrowsingPreferencesFallback(isWebApp, APP_NAME_LOWER, saved);
      } catch (reason) {
        // The browser keeps its localStorage copy, which is authoritative when
        // SQLite is in memory. Native has no such fallback, so it rolls back.
        if (isWebApp) return;
        if (version === this.#browsingVersion) this.#browsing = previous;
        this.#error = `Could not save browsing setting: ${errorMessage(reason)}`;
      }
    });
    return this.#browsingQueue;
  }

  saveReader(patch: ReaderPreferencePatch): Promise<void> {
    this.#reader = {
      ...this.#reader,
      [patch.preference]: patch.value,
    } as ReaderPreferences;
    this.#pendingReaderPatches.set(patch.preference, patch);
    if (this.#readerDebounce) clearTimeout(this.#readerDebounce);
    this.#readerDebounce = setTimeout(() => {
      this.#readerDebounce = undefined;
      void this.flushReader();
    }, READER_SAVE_DEBOUNCE_MS);
    return this.#readerQueue;
  }

  flushReader(): Promise<void> {
    if (this.#readerDebounce) {
      clearTimeout(this.#readerDebounce);
      this.#readerDebounce = undefined;
    }
    if (this.#pendingReaderPatches.size === 0) return this.#readerQueue;
    const patches = [...this.#pendingReaderPatches.values()];
    this.#pendingReaderPatches.clear();
    const snapshot = this.#reader;
    const version = ++this.#readerVersion;
    this.#readerQueue = this.#readerQueue.then(async () => {
      try {
        let saved: ReaderPreferences | undefined;
        for (const patch of patches) saved = await commands.updateReaderPreference(patch);
        if (saved && version === this.#readerVersion) this.#reader = saved;
      } catch {
        localStorage.setItem(READER_STORAGE_KEY, JSON.stringify(snapshot));
      }
    });
    return this.#readerQueue;
  }
}

export const preferences = new PreferenceStore();
