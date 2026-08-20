import { page } from "$app/state";

import { commands } from "./commands";
import { destinationHref, libraryRootHref, type Destination } from "./navigation";
import { navigate } from "./navigate";
import { shell } from "./shell-state.svelte";
import { commitAndMeasureRender, measureInvoke } from "./performance";
import { requestLibraryRefresh } from "./library-events";
import { clearThumbnailCache } from "../library/thumbnail-cache";
import { BrowserSessionStore } from "../library/browser-session.svelte";
import { LibraryStateStore } from "../library/state.svelte";
import {
  persistableSession,
  routeFromSession,
  sessionFromRoute,
  sortedLibraries,
} from "./app-utils";
import { readUiSession, writeUiSession, type AppUiSession } from "../session/ui-session";

function errorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

/**
 * Which library is open, and where in it.
 *
 * Selection follows the URL: every library route carries the id, so this holds
 * a stored value only as a fallback for the routes that do not — account,
 * settings and store — and for the moment before the first navigation. The
 * remembered place per library is what lets switching libraries return to the
 * folder it was left in rather than to its root.
 */
function currentRoute() {
  return {
    id: page.route.id,
    libraryId: page.params.libraryId,
    folderId: page.params.folderId,
    subjectId: page.params.subjectId,
    bookId: page.params.bookId,
    readerLibraryId: page.url.searchParams.get("library") ?? undefined,
  };
}

function currentPath() {
  return `${page.url.pathname}${page.url.search}`;
}

function createLibrarySession() {
  const libraries = new LibraryStateStore();
  const browserSession = new BrowserSessionStore();

  const routeLibraryId = $derived(
    page.params.libraryId || (page.url.searchParams.get("library") ?? ""),
  );
  const activeLibraryId = $derived(
    libraries.entries.some((entry) => entry.library_id === routeLibraryId)
      ? routeLibraryId
      : libraries.activeLibraryId,
  );
  const activeLibrary = $derived(
    libraries.entries.find((entry) => entry.library_id === activeLibraryId),
  );

  /** Records where the reader is, and returns the session that describes it. */
  function snapshot(): AppUiSession | null {
    return sessionFromRoute({
      route: currentRoute(),
      activeLibraryId,
      browserSession,
    });
  }

  function persist(): void {
    writeUiSession(persistableSession(snapshot()));
  }

  /** Keeps the fallback in step for routes that carry no library of their own. */
  function followRoute(): void {
    if (routeLibraryId && routeLibraryId !== libraries.activeLibraryId)
      libraries.select(routeLibraryId);
  }

  /**
   * Where a destination was last left, so returning to it resumes rather than
   * restarting. Only folders and subjects have somewhere to be within them.
   */
  function placeLocation(destination: Destination): string | undefined {
    const place = browserSession.place(activeLibraryId);
    if (destination === "folders") return place.folder_path.at(-1)?.id;
    if (destination === "subjects") return place.subject_path.at(-1)?.id;
    return undefined;
  }

  /** The canonical route for a destination, resumed where it was left. */
  function href(destination: Destination): string {
    return destinationHref(destination, activeLibraryId, placeLocation(destination));
  }

  /** Opens a library where the reader last left it. */
  async function select(libraryId: string): Promise<void> {
    // Record the place being left before the URL moves off it.
    if (activeLibraryId && activeLibraryId !== libraryId) snapshot();
    const place = browserSession.place(libraryId);
    libraries.select(libraryId);
    shell.clearError();
    const locationId =
      place.route === "folders"
        ? place.folder_path.at(-1)?.id
        : place.route === "subjects"
          ? place.subject_path.at(-1)?.id
          : undefined;
    try {
      await navigate(destinationHref(place.route, libraryId, locationId));
    } catch (reason) {
      shell.setError(errorMessage(reason));
    }
  }

  async function load(): Promise<void> {
    try {
      const entries = await measureInvoke("libraries", () => commands.getLibraries());
      const sorted = sortedLibraries(entries);
      await commitAndMeasureRender(
        "libraries",
        () => libraries.setEntries(sorted),
        entries.length,
      );
      const route = currentRoute();
      const wanted = route.libraryId || route.readerLibraryId;
      const initial = sorted.find((entry) => entry.library_id === wanted) ?? sorted[0];
      if (initial) libraries.select(initial.library_id);
    } catch (reason) {
      // The standalone browser build still supports opening a local EPUB.
      shell.setError(errorMessage(reason));
    }
  }

  /**
   * Re-reads the library list and tells the mounted route to reload itself. The
   * shell holds no page data, so it cannot refetch a page on its behalf.
   */
  async function refresh(readerOpen: boolean): Promise<void> {
    const entries = sortedLibraries(await commands.getLibraries());
    libraries.setEntries(entries);
    if (!entries.some((entry) => entry.library_id === activeLibraryId)) {
      if (entries[0]) await select(entries[0].library_id);
      return;
    }
    // Reloading the library under an open reader would pull its file away.
    if (readerOpen) return;
    clearThumbnailCache();
    requestLibraryRefresh();
  }

  /** Reconciles against the server for the library list the switcher renders. */
  async function reconcile(): Promise<void> {
    const libraryList = await commands.reconcileLibraries();
    libraries.setEntries(sortedLibraries(libraryList.libraries));
    requestLibraryRefresh();
  }

  /**
   * Reopens where the application was last closed.
   *
   * Only meaningful on a bare entry point: arriving at a real URL means the
   * reader asked for somewhere specific and the session must not override it.
   *
   * A restored reader is pushed on top of its library rather than replacing the
   * entry point, so there is something behind it. Leaving the reader is a
   * history pop — for the reader's own control, for the browser gesture, and
   * for Android's hardware back button — and none of those can invent an entry
   * that was never recorded.
   */
  async function restore(session: AppUiSession | null): Promise<boolean> {
    if (!session) return false;
    if (session.kind === "browse") browserSession.restore(session);
    const routeId = page.route.id;
    if (routeId !== "/" && routeId !== "/library" && routeId !== null) return false;
    const route = routeFromSession(session);
    if (!route || route === currentPath()) return true;

    if (session.kind === "reader") {
      const behind = session.library_id ? libraryRootHref(session.library_id) : "/library";
      if (behind !== currentPath()) await navigate(behind, true);
      await navigate(route);
      return true;
    }

    await navigate(route, true);
    return true;
  }

  return {
    get entries() {
      return libraries.entries;
    },
    get activeLibraryId() {
      return activeLibraryId;
    },
    get activeLibrary() {
      return activeLibrary;
    },
    get store() {
      return libraries;
    },
    followRoute,
    href,
    persist,
    select,
    load,
    refresh,
    reconcile,
    restore,
    read: readUiSession,
  };
}

export const librarySession = createLibrarySession();
