import { getContext } from "svelte";
import type { ReaderNavigation } from "./reader-navigation.svelte";
import type { ReaderPreferencesController } from "./reader-preferences.svelte";
import type { ReaderTocPanelItem } from "../../../../components/shared/readers/sidebar/toc-panel-items.ts";

export type EpubReaderContext = {
  readerLocation: { activeHref: string };
  readerState: { error: string };
  navigation: ReaderNavigation;
  preferences: ReaderPreferencesController;
  openContentItem(item: ReaderTocPanelItem): void | Promise<void>;
  toggleContentItem(item: ReaderTocPanelItem): void;
};

export function useEpubReaderContext() {
  let reader = getContext<EpubReaderContext>("epub-reader");
  if (!reader) throw new Error("EPUB reader context is unavailable");
  return reader;
}
