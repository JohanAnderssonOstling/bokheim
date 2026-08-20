import { onMount } from "svelte";
import { preferences } from "../../lib/app/preferences.svelte";
import type { BrowsingPreferencePatch } from "../../lib/shared/types";

export function createSettingsPage() {
  // The shell loads these at startup; re-reading keeps a directly opened
  // /settings honest without giving this page its own copy.
  onMount(() => {
    void preferences.loadBrowsing();
  });

  return {
    get preferences() {
      return preferences.browsing;
    },
    savePreference: (patch: BrowsingPreferencePatch) => preferences.saveBrowsing(patch),
    get error() {
      return preferences.error;
    },
  };
}
