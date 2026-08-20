<script lang="ts">
  import { page } from "$app/state";
  import Store from "../Store.svelte";
  import StoreRouteShell from "../../StoreRouteShell.svelte";
  import { createStorePage } from "../../store-page.svelte";

  const route = createStorePage();
</script>

<svelte:head>
  <title>Catalogue · Bokheim</title>
</svelte:head>

<StoreRouteShell
  activeLibraryId={route.state.activeLibraryId}
  libraryName={route.activeLibrary?.library_name ?? "Libraries"}
  preferences={route.browsing()}
  onpreferencechange={route.savePreference}
  onimported={route.imported}
  oncreatelibrary={route.createLibrary}
>
  <Store
    activeLibraryId={route.state.activeLibraryId}
    libraryName={route.activeLibrary?.library_name ?? "Libraries"}
    preferences={route.browsing()}
    catalogSourceId={page.params.sourceId}
    onpreferencechange={route.savePreference}
    onimported={route.imported}
  />
</StoreRouteShell>
