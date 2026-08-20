<script lang="ts">
  import Store from "./catalog/Store.svelte";
  import StoreRouteShell from "./StoreRouteShell.svelte";
  import { createStorePage } from "./store-page.svelte";

  const page = createStorePage();
</script>

<svelte:head>
  <title>Store · Bokheim</title>
</svelte:head>

<StoreRouteShell
  activeLibraryId={page.state.activeLibraryId}
  libraryName={page.activeLibrary?.library_name ?? "Libraries"}
  preferences={page.browsing()}
  onpreferencechange={page.savePreference}
  onimported={page.imported}
  oncreatelibrary={page.createLibrary}
>
  <Store
    activeLibraryId={page.state.activeLibraryId}
    libraryName={page.activeLibrary?.library_name ?? "Libraries"}
    preferences={page.browsing()}
    onpreferencechange={page.savePreference}
    onimported={page.imported}
  />
</StoreRouteShell>
