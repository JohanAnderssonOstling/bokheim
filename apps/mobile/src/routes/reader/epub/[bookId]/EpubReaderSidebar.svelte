<script lang="ts">
  import { useReaderBookmarkActions } from "../../../../components/shared/readers/annotations/bookmark-actions.ts";
  import TocPanel from "../../../../components/shared/readers/sidebar/TocPanel.svelte";
  import AnnotationsPanel from "../../../../components/shared/readers/sidebar/AnnotationsPanel.svelte";
  import BookmarkPanel from "../../../../components/shared/readers/sidebar/BookmarkPanel.svelte";
  import SidebarShell from "../../../../components/shared/readers/sidebar/SidebarShell.svelte";
  import { useEpubReaderContext } from "./reader-context";

  type SidebarTab = "contents" | "highlights" | "bookmarks";
  let activeTab = $state<SidebarTab>("contents");
  let reader = useEpubReaderContext();
  let activeContentItemKey = $derived(
    reader.navigation.sidebarContentItems.find((item) => item.data && (item.data as { href?: string }).href === reader.readerLocation.activeHref)?.key ?? "",
  );
  const bookmarkActions = useReaderBookmarkActions();
  const bookmarkItems = $derived(bookmarkActions?.items ?? []);
</script>

<SidebarShell
  {activeTab}
  onTabChange={(tab) => (activeTab = tab)}
  hidden={!reader.navigation.sidebarOpen}
>
  {#snippet contents()}
    <TocPanel
      items={reader.navigation.sidebarContentItems}
      activeItemKey={activeContentItemKey}
      onActivate={reader.openContentItem}
      onToggle={reader.toggleContentItem}
    />
  {/snippet}

  {#snippet highlights()}
    <AnnotationsPanel error={reader.readerState.error} />
  {/snippet}
  {#snippet bookmarks()}
    <BookmarkPanel bookmarks={bookmarkItems} />
  {/snippet}
</SidebarShell>
