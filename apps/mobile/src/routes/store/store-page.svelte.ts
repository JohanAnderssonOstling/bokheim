import { onMount } from "svelte";
import { goto } from "$app/navigation";
import { commands } from "../../lib/app/commands";
import { preferences } from "../../lib/app/preferences.svelte";
import { requestLibraryRefresh } from "../../lib/app/library-events";
import type { BrowsingPreferencePatch, LibraryEntry } from "../../lib/shared/types";

export function createStorePage() {
  const state = $state({
    libraries: [] as LibraryEntry[],
    activeLibraryId: "",
  });
  const activeLibrary = $derived(
    state.libraries.find((library) => library.library_id === state.activeLibraryId),
  );

  async function load() {
    const [entries] = await Promise.all([
      commands.getLibraries().catch(() => []),
      preferences.loadBrowsing(),
    ]);
    state.libraries = [...entries].sort((left, right) =>
      left.library_name.localeCompare(right.library_name, undefined, { numeric: true }),
    );
    state.activeLibraryId ||= state.libraries[0]?.library_id ?? "";
  }

  const savePreference = (patch: BrowsingPreferencePatch) =>
    preferences.saveBrowsing(patch);

  async function imported() {
    await commands.syncAllLibraries().catch(() => undefined);
    await load();
    // The book landed in a library, so every listing of one is now stale.
    requestLibraryRefresh();
  }

  async function createLibrary() {
    const created = await commands.createLibrary(`Library ${state.libraries.length + 1}`);
    state.activeLibraryId = created.library_id;
    await goto(`/library/${encodeURIComponent(created.library_id)}`, {
      replaceState: true,
    });
  }

  onMount(load);
  const browsing = () => preferences.browsing;
  return { state, activeLibrary, browsing, savePreference, imported, createLibrary };
}
