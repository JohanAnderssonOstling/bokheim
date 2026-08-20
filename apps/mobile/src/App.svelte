<script lang="ts">
  import type { Snippet } from "svelte";
  import { browsingLayoutStyle as browsingLayoutStyleFrom } from "./lib/shared/layout-tokens.ts";
  import { page } from "$app/state";

  import AudiobookPlayer from "./components/AudiobookPlayer.svelte";
  import { onReaderOpen } from "./lib/app/reader-handoff";
  import { requestSearchFocus } from "./lib/app/search-focus";
  import BrowserRoot from "./components/shell/BrowserRoot.svelte";
  import BrowserContent from "./components/shell/BrowserContent.svelte";
  import LibraryOverlays from "./components/shell/LibraryOverlays.svelte";
  import ApplicationSurface from "./components/shell/ApplicationSurface.svelte";
  import ApkUpdateNotice from "./components/shell/ApkUpdateNotice.svelte";
  import StatusState from "./components/shared/StatusState.svelte";
  import {
    applyApplicationAppearance,
    observeSystemAppearance,
  } from "./lib/app/application-appearance";
  import { commands } from "./lib/app/commands";
  import {
    APP_NAV_ITEMS,
    destinationHref,
    libraryDestinationForRoute,
    NAV_ITEMS,
    universalDestinationForRoute,
    type Destination,
  } from "./lib/app/navigation";
  import { clearThumbnailCache } from "./lib/library/thumbnail-cache";
  import { createLibraryOverlayController } from "./lib/library/overlay-controller.svelte";
  import { readerSession } from "./lib/app/reader-session.svelte";
  import { librarySession } from "./lib/app/library-session.svelte";
  import { navigate } from "./lib/app/navigate";
  import { shell } from "./lib/app/shell-state.svelte";
  import {
    publishBookDownloadStatus,
    publishFolderDownloadStatus,
    requestLibraryRefresh,
  } from "./lib/app/library-events";
  import type { AccountStatus } from "./lib/shared/types";
  import { APP_NAME } from "./lib/app/application-name";
  import { isWebApp } from "./lib/app/platform";
  import { registerAppLifecycle } from "./lib/app/app-lifecycle.svelte";
  import { accountStatus } from "./lib/app/account-status.svelte";
  import { preferences } from "./lib/app/preferences.svelte";
  import { startNativeApplication } from "./lib/app/startup-native";
  import {
    startWebApplication,
    synchronizeForegroundWebApp,
  } from "./lib/app/startup-web";
  import { isReaderRoute } from "./lib/app/app-utils";

  const BOOK_DETAIL_ROUTE = "/library/[libraryId]/book/[bookId]";

  const browsingLayoutStyle = browsingLayoutStyleFrom();

  const libraries = $derived(librarySession.entries);
  const activeLibraryId = $derived(librarySession.activeLibraryId);
  const activeLibrary = $derived(librarySession.activeLibrary);

  let { routeContent }: { routeContent?: Snippet } = $props();
  // Each route sets its own document title. This is only the fallback the shell
  // shows before a route has mounted, and the reader's title while it is open.
  const destinationTitle = $derived(
    [...NAV_ITEMS, ...APP_NAV_ITEMS].find((item) => item.id === activeDestination)
      ?.label || APP_NAME,
  );
  const inBookDetail = $derived(page.route.id === BOOK_DETAIL_ROUTE);

  /**
   * The sidebar selection is the URL, not a second copy of it. SvelteKit owns
   * the current route, so the destination is read from it rather than tracked
   * alongside it and kept in step by hand.
   */
  const activeDestination = $derived(
    universalDestinationForRoute(page.route.id) ??
      libraryDestinationForRoute(page.route.id),
  );

  // `navigate` already ignores a move to where we are, so asking for the
  // destination we are on resumes it rather than resetting it to its root.
  function setDestination(destination: Destination) {
    void navigate(librarySession.href(destination));
  }
  const inReader = $derived(isReaderRoute(page.route.id));
  const audiobookSession = $derived(readerSession.audiobookSession);

  $effect(() =>
    onReaderOpen(
      ({ book, target }) => void readerSession.openBook(activeLibraryId, book, target),
    ),
  );
  const libraryOverlayController = createLibraryOverlayController({
    libraries: librarySession.store,
    selectLibrary: (libraryId) => librarySession.select(libraryId),
    leaveLibraries: () => navigate("/library", true),
    setError: (message) => shell.setError(message),
  });

  $effect(() => {
    applyApplicationAppearance(preferences.browsing);
  });

  // Without this, "follow system" would only be honoured at startup.
  $effect(() =>
    observeSystemAppearance(() => applyApplicationAppearance(preferences.browsing)),
  );

  $effect(() => librarySession.followRoute());

  // A queued download or a book waiting to open belongs to the library it was
  // started in, so changing library abandons it.
  $effect(() => readerSession.useLibrary(activeLibraryId));

  $effect(() => {
    if (shell.routesReady) librarySession.persist();
  });

  async function retryShell() {
    shell.clearError();
    if (readerSession.hasPendingOpen) {
      await readerSession.retryPendingOpen();
      return;
    }
    if (!libraries.length) {
      await librarySession.load();
      return;
    }
    requestLibraryRefresh();
  }

  /** Signed-out devices have no server catalogue to reconcile against. */
  async function reconcileCatalogue() {
    if (accountStatus.signedIn) await librarySession.reconcile();
  }

  // `isWebApp` is a build constant, so the branch the bundle does not need is
  // dropped along with the sequence behind it.
  const startApplication = () =>
    isWebApp ? startWebApplication() : startNativeApplication();

  /**
   * Only the folder and subject pages carry a search field, so the shortcut
   * goes to one first. The field itself claims the request once it exists.
   */
  function focusContextualSearch() {
    if (activeDestination !== "folders" && activeDestination !== "subjects")
      setDestination("folders");
    requestSearchFocus();
  }

  registerAppLifecycle({
    isWebApp,
    getActiveLibraryId: () => activeLibraryId,
    getActiveDestination: () => activeDestination,
    getReaderOpen: () => inReader,
    isSignedIn: () => accountStatus.signedIn,
    startApplication,
    loadServerTransferState: reconcileCatalogue,
    synchronizeForegroundWebApp,
    focusContextualSearch,
    // The backend owns the network verdict; nothing in the shell renders it.
    onNetworkStatus: async (status) => {
      await commands.updateNetworkStatus(status);
    },
    onNativeAudiobookState: (state) => readerSession.restoreNativeAudiobook(state),
    onDownloadStatus: ({ event }) => {
      const change = event.DownloadStatusChanged;
      readerSession.handleDownloadStatus(change.content_hash, change.state);
      publishBookDownloadStatus({
        contentHash: change.content_hash,
        state: change.state,
      });
    },
    onFolderDownloadStatus: ({ event }) => {
      const change = event.FolderDownloadStatusChanged;
      publishFolderDownloadStatus({
        dir_id: change.dir_id,
        downloaded_books: change.downloaded_books,
        total_books: change.total_books,
      });
    },
    setAccountError: (message) => shell.setError(message),
    clearThumbnailCache,
    onDestinationShortcut: setDestination,
  });
</script>

<!-- Reader routes set their own title from the book they opened, and a page
     title set later in the tree wins, so this is only the shell's fallback. -->
<svelte:head><title>{destinationTitle} · {APP_NAME}</title></svelte:head>

<ApplicationSurface
  {isWebApp}
  accountResolved={accountStatus.resolved}
  webAppReady={!isWebApp || shell.webReady}
  readerRoute={inReader}
>
  {#snippet reader()}
    {#if routeContent}{@render routeContent()}{/if}
  {/snippet}
  {#snippet browser()}
    {#snippet pageContent()}
      {#if routeContent}
        {@render routeContent()}
      {:else}
        <StatusState
          kind="loading"
          title="Opening route"
        />
      {/if}
    {/snippet}
    <BrowserRoot
      active={activeDestination}
      hrefFor={librarySession.href}
      libraryName={activeLibrary?.library_name ?? "Libraries"}
      style={browsingLayoutStyle}
      onlibraries={libraryOverlayController.openSwitcher}
    >
      {#snippet content()}
        <BrowserContent
          active={activeDestination}
          detailMode={inBookDetail}
          playerOpen={audiobookSession !== null}
          content={pageContent}
        />
      {/snippet}
    </BrowserRoot>
    <LibraryOverlays controller={libraryOverlayController} />
    {#if shell.error}
      <StatusState
        kind="error"
        variant="compact"
        message={shell.error}
        actionLabel="Retry"
        onaction={retryShell}
      />
    {/if}
  {/snippet}
</ApplicationSurface>
<ApkUpdateNotice />

{#if audiobookSession}
  <AudiobookPlayer
    session={audiobookSession}
    aboveNavigation={!inReader}
    onclose={() => readerSession.closeAudiobook()}
  />
{/if}
