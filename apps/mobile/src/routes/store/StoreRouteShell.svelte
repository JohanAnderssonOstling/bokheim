<script lang="ts">
  import type { Snippet } from "svelte";
  import EmptyPage from "../../components/shell/EmptyPage.svelte";
  import { BROWSING_STATES_CONTRACT } from "../../lib/shared/types";
  import type {
    BrowsingPreferencePatch,
    BrowsingPreferences,
  } from "../../lib/shared/types";

  let {
    activeLibraryId,
    libraryName,
    preferences,
    onpreferencechange,
    onimported,
    oncreatelibrary,
    children,
  }: {
    activeLibraryId: string;
    libraryName: string;
    preferences: BrowsingPreferences;
    onpreferencechange: (patch: BrowsingPreferencePatch) => void | Promise<void>;
    onimported: () => void | Promise<void>;
    oncreatelibrary: () => void | Promise<void>;
    children: Snippet;
  } = $props();
</script>

{#if !activeLibraryId}
  <EmptyPage
    title={BROWSING_STATES_CONTRACT.no_libraries_title}
    description={BROWSING_STATES_CONTRACT.no_libraries_description}
    primaryLabel={BROWSING_STATES_CONTRACT.create_library_label}
    onprimary={oncreatelibrary}
  />
{:else}
  {@render children()}
{/if}
