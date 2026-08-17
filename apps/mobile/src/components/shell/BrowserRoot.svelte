<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Destination } from "../../lib/navigation";
  import type { LibraryEntry } from "../../lib/types";
  import AdaptiveNavigation from "./AdaptiveNavigation.svelte";
  import AppTopbar from "./AppTopbar.svelte";
  import LibrarySwitcher from "./LibrarySwitcher.svelte";
  import CreateFolderDialog from "../library/CreateFolderDialog.svelte";
  import CreateLibraryDialog from "../library/CreateLibraryDialog.svelte";

  let {
    active, pageTitle, libraryName, libraries, activeLibraryId, breadcrumbs, style, detailMode = false, playerOpen = false,
    addingBook, importingFolderArchive, showLibrarySwitcher, showCreateFolder, showCreateLibrary, creatingLibrary, importingLibraryArchive,
    content, onnavigate, onlibraries, onaccount, onsettings, onaddbook, onnewfolder, onimportfolder, onbreadcrumb,
    onselectlibrary, oncreatelibrary, onopenlibrary, onimportlibraryarchive, onremovelibrary, oncloselibraryswitcher,
    onclosecreatefolder, oncreatefolder, onclosecreatelibrary, oncreate,
  }: {
    active: Destination; pageTitle: string; libraryName: string; libraries: LibraryEntry[]; activeLibraryId: string;
    breadcrumbs: { id: string; name: string }[]; style: string; detailMode?: boolean; playerOpen?: boolean; addingBook: boolean; importingFolderArchive: boolean;
    showLibrarySwitcher: boolean; showCreateFolder: boolean; showCreateLibrary: boolean; creatingLibrary: boolean;
    importingLibraryArchive: boolean;
    content: Snippet; onnavigate: (destination: Destination) => void; onlibraries: () => void; onaccount: () => void; onsettings: () => void;
    onaddbook: () => void | Promise<void>; onnewfolder: () => void; onimportfolder: () => void | Promise<void>; onbreadcrumb: (level: number) => void | Promise<void>;
    onselectlibrary: (libraryId: string) => void | Promise<void>; oncreatelibrary: () => void; onopenlibrary: () => void | Promise<void>;
    onimportlibraryarchive: () => void | Promise<void>;
    onremovelibrary: (libraryId: string) => void | Promise<void>; oncloselibraryswitcher: () => void;
    onclosecreatefolder: () => void; oncreatefolder: (name: string) => Promise<void>;
    onclosecreatelibrary: () => void; oncreate: (name: string) => void | Promise<void>;
  } = $props();
</script>

<div class="adaptive-shell" {style}>
  <AdaptiveNavigation {active} {libraryName} {onnavigate} {onlibraries} {onaccount} {onsettings} />
  <div class="app-column">
    {#if !detailMode}<AppTopbar
      {pageTitle} {libraryName} folders={breadcrumbs}
      showBreadcrumb={active === "folders" || active === "subjects"}
      showLibraryActions={active === "folders"}
      {addingBook} importingFolder={importingFolderArchive} {onlibraries} {onaddbook}
      onnewfolder={onnewfolder} {onimportfolder} onfolder={onbreadcrumb}
    />{/if}
    <main class:player-open={playerOpen} class:detail-view={detailMode} class:library-view={!detailMode && (active === "folders" || active === "subjects")} class:authors-view={!detailMode && active === "authors"} class:store-view={!detailMode && active === "store"} class="content">
      {@render content()}
    </main>
  </div>

  {#if showLibrarySwitcher}
    <LibrarySwitcher {libraries} {activeLibraryId} onclose={oncloselibraryswitcher} onselect={onselectlibrary} oncreate={oncreatelibrary} onopen={onopenlibrary} onimportarchive={onimportlibraryarchive} importingArchive={importingLibraryArchive} onremove={onremovelibrary} />
  {/if}
  {#if showCreateFolder}<CreateFolderDialog onclose={onclosecreatefolder} oncreate={oncreatefolder} />{/if}
  {#if showCreateLibrary}<CreateLibraryDialog busy={creatingLibrary} onclose={onclosecreatelibrary} oncreate={oncreate} />{/if}
</div>

<style>
  .adaptive-shell { --page-gutter: 1rem; height: 100dvh; min-height: 100dvh; overflow: hidden; background: var(--color-surface-bg); }
  .app-column { display: flex; flex-direction: column; min-width: 0; min-height: 0; height: 100dvh; }
  .content { flex: 1; min-height: 0; width: 100%; max-width: none; margin: 0 auto; padding: 1.1rem var(--page-gutter) calc(4.9rem + env(safe-area-inset-bottom)); padding-right: max(var(--page-gutter), env(safe-area-inset-right)); padding-left: max(var(--page-gutter), env(safe-area-inset-left)); overflow-y: scroll; overscroll-behavior-y: contain; scrollbar-gutter: stable; scrollbar-color: var(--color-border) transparent; scrollbar-width: thin; }
  .content::-webkit-scrollbar { width: .55rem; } .content::-webkit-scrollbar-track { background: transparent; } .content::-webkit-scrollbar-thumb { border: .14rem solid transparent; border-radius: 0; background: var(--color-border); background-clip: padding-box; }
  .content.library-view { padding-top: 0; } .content.authors-view { display: flex; flex-direction: column; overflow: hidden; }
  .content.detail-view { padding-top:1.1rem; }
  .content.player-open { padding-bottom:calc(9.8rem + env(safe-area-inset-bottom)); }
  @media (min-width: 45rem) { .adaptive-shell { --page-gutter: var(--layout-content-padding); display: grid; grid-template-columns: var(--layout-sidebar-width) minmax(0, 1fr); } .content { padding-top: var(--layout-content-padding); padding-right: max(var(--page-gutter), env(safe-area-inset-right)); padding-bottom: 3rem; padding-left: max(var(--page-gutter), env(safe-area-inset-left)); } .content.player-open { padding-bottom:7.5rem; } .content.authors-view { padding-bottom: 1.5rem; } }
  @media (min-width: 88rem) { .adaptive-shell { --page-gutter: 2rem; } }
</style>
