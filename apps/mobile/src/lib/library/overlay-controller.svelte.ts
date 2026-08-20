import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { commands } from "../app/commands";
import { isWebApp } from "../app/platform";
import { measureInvoke } from "../app/performance";
import { sortedLibraries } from "../app/app-utils";
import type { LibraryStateStore } from "./state.svelte";
import type { LibraryEntry } from "../shared/types";

export interface LibraryOverlayControllerOptions {
  libraries: LibraryStateStore;
  /** Opens a library where the reader last left it. */
  selectLibrary: (libraryId: string) => void | Promise<void>;
  /** Leaves for the empty state when the last library is removed. */
  leaveLibraries: () => void | Promise<void>;
  setError: (message: string) => void;
}

function errorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

/**
 * The library switcher, and every operation reachable from it.
 *
 * Creating, opening, importing and removing a library are all things the
 * switcher offers and nothing else does, so they live with it rather than in
 * the shell that renders it.
 */
export function createLibraryOverlayController(options: LibraryOverlayControllerOptions) {
  const state = options.libraries;
  let switcherOpen = $state(false);
  let createLibraryOpen = $state(false);
  let creatingLibrary = $state(false);
  let importingLibraryArchive = $state(false);

  function closeSwitcher() {
    switcherOpen = false;
  }

  async function selectLibrary(libraryId: string) {
    closeSwitcher();
    await options.selectLibrary(libraryId);
  }

  async function createLibrary(name: string) {
    if (creatingLibrary) return;
    creatingLibrary = true;
    options.setError("");
    try {
      const library = await measureInvoke("create_library", () =>
        commands.createLibrary(name),
      );
      state.setEntries(
        sortedLibraries([
          ...state.entries.filter((entry) => entry.library_id !== library.library_id),
          library,
        ]),
      );
      createLibraryOpen = false;
      await options.selectLibrary(library.library_id);
    } catch (reason) {
      options.setError(errorMessage(reason));
    } finally {
      creatingLibrary = false;
    }
  }

  /** Native only: adopts existing library folders from disk. */
  async function openLibrary() {
    closeSwitcher();
    options.setError("");
    const selected = await openDialog({
      title: "Open libraries",
      directory: true,
      multiple: true,
    });
    const paths = typeof selected === "string" ? [selected] : (selected ?? []);
    if (!paths.length) return;
    const opened: LibraryEntry[] = [];
    const failures: string[] = [];
    for (const path of paths) {
      try {
        opened.push(await commands.openLibraryFolder(path));
      } catch (reason) {
        failures.push(`${path}: ${errorMessage(reason)}`);
      }
    }
    if (opened.length) {
      state.setEntries(sortedLibraries(await commands.getLibraries()));
      await options.selectLibrary(opened.at(-1)!.library_id);
    }
    if (failures.length)
      options.setError(`Could not open some libraries: ${failures.join("; ")}`);
  }

  // ZIP import is a browser affordance: native builds pick real files instead.
  // Loading it on demand is what keeps `lib/web` out of the native bundle,
  // which the build enforces.
  async function importLibraryArchive() {
    if (!isWebApp || importingLibraryArchive) return;
    importingLibraryArchive = true;
    options.setError("");
    try {
      const { pickLibraryArchive, importLibraryArchive: importArchive } =
        await import("../web/library-archive");
      const archive = await pickLibraryArchive();
      if (!archive) return;
      const result = await importArchive(archive);
      state.setEntries(sortedLibraries(await commands.getLibraries()));
      closeSwitcher();
      await options.selectLibrary(result.library.library_id);
      if (result.failures.length)
        options.setError(
          `Imported ${result.imported} book${result.imported === 1 ? "" : "s"}; ${result.failures.length} failed. ${result.failures[0]}`,
        );
      // An empty browser library is deliberately not created remotely. Its
      // first import makes it eligible for synchronization.
      const account = await commands.getAccountStatus().catch(() => null);
      if (account?.signed_in) {
        await commands.reconcileLibraries();
        await commands.syncAllLibraries();
      }
    } catch (reason) {
      options.setError(`Could not import ZIP library: ${errorMessage(reason)}`);
    } finally {
      importingLibraryArchive = false;
    }
  }

  async function removeLibrary(libraryId: string) {
    closeSwitcher();
    const library = state.entries.find((entry) => entry.library_id === libraryId);
    if (!library) return;
    if (
      !window.confirm(
        `Delete “${library.library_name}”?\n\nIts database and state will be removed from this device immediately. Server deletion will happen now when possible, or automatically after the next sign-in. Local book files, folders, and stored server blobs will be kept.`,
      )
    )
      return;
    options.setError("");
    try {
      await commands.removeLibrary(libraryId);
      const remaining = sortedLibraries(await commands.getLibraries());
      state.setEntries(remaining);
      if (remaining.length) await options.selectLibrary(remaining[0].library_id);
      else {
        state.clearSelection();
        await options.leaveLibraries();
      }
    } catch (reason) {
      options.setError(`Could not remove library: ${errorMessage(reason)}`);
    }
  }

  return {
    get libraries() {
      return state.entries;
    },
    get activeLibraryId() {
      return state.activeLibraryId;
    },
    get switcherOpen() {
      return switcherOpen;
    },
    get createLibraryOpen() {
      return createLibraryOpen;
    },
    get creatingLibrary() {
      return creatingLibrary;
    },
    get importingLibraryArchive() {
      return importingLibraryArchive;
    },
    openSwitcher: () => (switcherOpen = true),
    closeSwitcher,
    openCreateLibrary: () => {
      closeSwitcher();
      createLibraryOpen = true;
    },
    closeCreateLibrary: () => (createLibraryOpen = false),
    selectLibrary,
    openLibrary,
    importLibraryArchive,
    removeLibrary,
    createLibrary,
  };
}

export type LibraryOverlayController = ReturnType<typeof createLibraryOverlayController>;
