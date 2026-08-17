<script lang="ts">
  import Store from "../store/Store.svelte";
  import EmptyPage from "../shell/EmptyPage.svelte";
  import { BROWSING_STATES_CONTRACT } from "../../lib/types";
  import type { BrowsingPreferencePatch, BrowsingPreferences } from "../../lib/types";
  let { activeLibraryId, libraryName, preferences, onpreferencechange, onimported, oncreatelibrary }: {
    activeLibraryId: string; libraryName: string; preferences: BrowsingPreferences;
    onpreferencechange: (patch: BrowsingPreferencePatch) => void | Promise<void>;
    onimported: () => void | Promise<void>; oncreatelibrary: () => void;
  } = $props();
</script>
{#if !activeLibraryId}
  <EmptyPage title={BROWSING_STATES_CONTRACT.no_libraries_title} description={BROWSING_STATES_CONTRACT.no_libraries_description} primaryLabel={BROWSING_STATES_CONTRACT.create_library_label} onprimary={oncreatelibrary} />
{:else}
  <Store {activeLibraryId} {libraryName} {preferences} {onpreferencechange} {onimported} />
{/if}
