<script lang="ts">
  import type { ReaderAnnotation, ReaderBookmark } from "../../../../lib/shared/types";
  import TocPanel from "../../../../components/shared/readers/sidebar/TocPanel.svelte";
  import AnnotationsPanel from "../../../../components/shared/readers/sidebar/AnnotationsPanel.svelte";
  import BookmarkPanel from "../../../../components/shared/readers/sidebar/BookmarkPanel.svelte";
  import SidebarShell from "../../../../components/shared/readers/sidebar/SidebarShell.svelte";
  import type { ReaderTocPanelItem } from "../../../../components/shared/readers/sidebar/toc-panel-items.ts";

  type SidebarTab = "contents" | "highlights" | "bookmarks";
  type NavigationItem = ReaderTocPanelItem;

  let {
    activeTab,
    onTabChange,
    contentItems,
    activeContentItemKey = "",
    onOpenContentItem,
    onToggleContentItem,
    annotations,
    bookmarks: bookmarkItems,
    annotationError = "",
    bookmarkCfi = "",
    bookmarkLabel = "",
    onActivateAnnotation,
    onOpenBookmark,
  }: {
    activeTab: SidebarTab;
    onTabChange: (tab: SidebarTab) => void;
    contentItems: NavigationItem[];
    activeContentItemKey?: string;
    onOpenContentItem: (item: NavigationItem) => void | Promise<void>;
    onToggleContentItem: (item: NavigationItem) => void;
    annotations: ReaderAnnotation[];
    bookmarks: ReaderBookmark[];
    annotationError?: string;
    bookmarkCfi?: string;
    bookmarkLabel?: string;
    onActivateAnnotation: (annotation: ReaderAnnotation) => void;
    onOpenBookmark: (bookmark: ReaderBookmark) => void;
  } = $props();

  let annotationFilter = $state("");
</script>

<SidebarShell
  {activeTab}
  {onTabChange}
>
  {#snippet contents()}
    <TocPanel
      items={contentItems}
      activeItemKey={activeContentItemKey}
      onActivate={onOpenContentItem}
      onToggle={onToggleContentItem}
    />
  {/snippet}

  {#snippet highlights()}
    <AnnotationsPanel {annotations} error={annotationError} onActivate={onActivateAnnotation} />
  {/snippet}
  {#snippet bookmarks()}
    <BookmarkPanel bookmarks={bookmarkItems} cfi={bookmarkCfi} label={bookmarkLabel} onOpen={onOpenBookmark} />
  {/snippet}
</SidebarShell>
