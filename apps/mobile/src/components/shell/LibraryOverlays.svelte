<script lang="ts">
  import type { LibraryOverlayController } from "../../lib/library/overlay-controller.svelte";
  import CreateLibraryDialog from "../shared/dialogs/CreateLibraryDialog.svelte";
  import LibrarySwitcher from "./LibrarySwitcher.svelte";

  let { controller }: { controller: LibraryOverlayController } = $props();
</script>

{#if controller.switcherOpen}
  <LibrarySwitcher
    libraries={controller.libraries}
    activeLibraryId={controller.activeLibraryId}
    onclose={controller.closeSwitcher}
    onselect={controller.selectLibrary}
    oncreate={controller.openCreateLibrary}
    onopen={controller.openLibrary}
    onimportarchive={controller.importLibraryArchive}
    importingArchive={controller.importingLibraryArchive}
    onremove={controller.removeLibrary}
  />
{/if}
{#if controller.createLibraryOpen}
  <CreateLibraryDialog
    busy={controller.creatingLibrary}
    onclose={controller.closeCreateLibrary}
    oncreate={controller.createLibrary}
  />
{/if}
