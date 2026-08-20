import {
  tocPanelItems,
  initialExpandedKeys,
  toggleExpandedKey,
} from "../../../../components/shared/readers/sidebar/toc-panel-items.ts";
import { initialTocState, persistTocState } from "./reader-ui-prefs";
import type { TocItem } from "./reader-types";

export interface ReaderNavigationOptions {
  getActiveTocHref: () => string;
  loadAnnotationsEnabled: () => boolean;
}

/**
 * Owns which reader panel (sidebar/search/settings) is open, the table of
 * contents and which entries are expanded — the state every
 * chrome toggle in the reader ultimately reads or flips.
 */
export function createReaderNavigation({
  getActiveTocHref,
  loadAnnotationsEnabled,
}: ReaderNavigationOptions) {
  let sidebarOpen = $state(false);
  let settingsOpen = $state(false);
  let searchOpen = $state(false);
  let tocItems = $state<TocItem[]>([]);
  let expandedTocKeys = $state<Set<string>>(new Set());

  const sidebarContentItems = $derived(tocPanelItems(tocItems, expandedTocKeys));
  const sidebarActiveContentItemKey = $derived(
    tocItems.find((item) => item.href === getActiveTocHref())?.key ?? "",
  );

  function restoreTocState() {
    sidebarOpen = initialTocState();
  }

  function setToc(nextTocItems: TocItem[]) {
    tocItems = nextTocItems;
    expandedTocKeys = initialExpandedKeys(nextTocItems);
  }

  function setSidebarOpen(next: boolean) {
    sidebarOpen = next;
    persistTocState(next);
  }

  function toggleTocItem(item: TocItem) {
    expandedTocKeys = toggleExpandedKey(expandedTocKeys, item.key);
  }

  function expandActiveTocAncestors(href: string) {
    const item = tocItems.find((candidate) => candidate.href === href);
    if (!item?.parentKeys.length) return;
    const next = new Set(expandedTocKeys);
    for (const key of item.parentKeys) next.add(key);
    expandedTocKeys = next;
  }

  function hideSidebar() { setSidebarOpen(false); }

  function closePanels() {
    settingsOpen = false;
    hideSidebar();
    searchOpen = false;
  }

  function togglePanel(panel: "search" | "settings") {
    const next = panel === "search" ? !searchOpen : !settingsOpen;
    closePanels();
    if (panel === "search") searchOpen = next;
    if (panel === "settings") settingsOpen = next;
  }

  function showReaderSidebar() {
    settingsOpen = false;
    searchOpen = false;
    sidebarOpen = true;
    persistTocState(true);
  }

  function toggleReaderSidebar() {
    if (sidebarOpen) {
      hideSidebar();
      return;
    }
    showReaderSidebar();
  }

  function dismissSettingsFromOutside(event: PointerEvent) {
    if (!settingsOpen) return;
    const target = event.target;
    if (
      target instanceof Element &&
      target.closest(".settings-panel, .reader-settings-trigger")
    )
      return;
    settingsOpen = false;
  }

  return {
    get sidebarOpen() {
      return sidebarOpen;
    },
    get tocItems() {
      return tocItems;
    },
    get settingsOpen() {
      return settingsOpen;
    },
    set settingsOpen(value: boolean) {
      settingsOpen = value;
    },
    get searchOpen() {
      return searchOpen;
    },
    get sidebarContentItems() {
      return sidebarContentItems;
    },
    get sidebarActiveContentItemKey() {
      return sidebarActiveContentItemKey;
    },
    restoreTocState,
    setToc,
    setSidebarOpen,
    toggleTocItem,
    expandActiveTocAncestors,
    hideSidebar,
    closePanels,
    togglePanel,
    showReaderSidebar,
    toggleReaderSidebar,
    dismissSettingsFromOutside,
  };
}

export type ReaderNavigation = ReturnType<typeof createReaderNavigation>;
